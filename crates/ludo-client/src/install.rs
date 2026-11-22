//! Installation de jeux sur l'appareil : téléchargement d'un fichier du catalogue sous
//! `<roms>/<chemin relatif>`, avec reprise, contrôle de taille et renommage atomique ; extraction
//! sûre du tar d'un dossier (Wii U, Switch 2 extrait) ; désinstallation (ROM seulement).
//!
//! Jamais de sauvegarde ici : seuls les chemins du catalogue, sous le dossier des ROM de l'appareil,
//! sont lus, écrits ou supprimés. Aucun lien symbolique n'est suivi sous le dossier des ROM.
//!
//! Le corps de la réponse est lu par un fil dédié, relayé par un canal : l'installateur peut ainsi
//! abandonner sur un délai d'inactivité ou une annulation, même si le serveur ne dit plus rien.

use crate::api::{ApiError, Client};
use ludo_proto::{conversion_target, ConvertProgress, GameFile, CONVERTIBLE};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Délai sans aucun octet reçu avant d'abandonner (le `.part` est gardé pour reprendre).
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("{0}")]
    Api(#[from] ApiError),
    #[error("chemin refusé dans le catalogue : {0}")]
    BadPath(String),
    #[error("lien symbolique refusé sur l'appareil : {0}")]
    SymLink(String),
    #[error("espace disque insuffisant : {} nécessaires, {} libres", human(*needed), human(*free))]
    NoSpace { needed: u64, free: u64 },
    #[error("taille inattendue après téléchargement : {} reçus, {} attendus (le catalogue du serveur a peut-être changé)", human(*got), human(*expected))]
    SizeMismatch { expected: u64, got: u64 },
    #[error("téléchargement interrompu ({0}) : reprise au même point à la prochaine tentative")]
    Interrupted(String),
    /// Conversion côté serveur (NSZ → NSP) refusée ou échouée ; le message vient du serveur.
    #[error("{0}")]
    Convert(String),
    #[error("Installation annulée")]
    Cancelled,
    #[error("archive refusée : {0}")]
    Archive(String),
    #[error("disque : {0}")]
    Io(String),
}

fn io(ctx: &str, e: std::io::Error) -> InstallError {
    InstallError::Io(format!("{ctx} : {}", io_reason(&e)))
}

/// Raison lisible d'une erreur d'entrée-sortie (jamais le texte anglais du système).
pub fn io_reason(e: &std::io::Error) -> &'static str {
    use std::io::ErrorKind as K;
    match e.kind() {
        K::NotFound => "introuvable",
        K::PermissionDenied => "accès refusé",
        K::AlreadyExists => "existe déjà",
        K::StorageFull => "disque plein",
        K::ReadOnlyFilesystem => "disque en lecture seule",
        K::IsADirectory => "dossier à la place d'un fichier",
        K::NotADirectory => "fichier à la place d'un dossier",
        K::DirectoryNotEmpty => "dossier non vide",
        K::InvalidFilename => "nom de fichier refusé",
        K::UnexpectedEof => "fichier tronqué",
        K::Interrupted | K::TimedOut => "opération interrompue",
        _ => "erreur d'entrée-sortie",
    }
}

/// Taille lisible (« 6,8 Go »).
pub fn human(bytes: u64) -> String {
    for (unit, name) in [(1_000_000_000u64, "Go"), (1_000_000, "Mo")] {
        if bytes >= unit {
            let tenths = (u128::from(bytes) * 10 + u128::from(unit) / 2) / u128::from(unit);
            return format!("{},{} {name}", tenths / 10, tenths % 10);
        }
    }
    if bytes >= 1_000 { format!("{} Ko", (bytes + 500) / 1_000) } else { format!("{bytes} o") }
}

/// Chemin relatif du fichier une fois converti par le serveur (`x.nsz` → `x.nsp`, `x.xcz` → `x.xci`) ;
/// `None` pour tout autre format. Sert à retrouver, sur l'appareil, un jeu installé converti.
pub fn converted_rel(rel: &str) -> Option<String> {
    let p = Path::new(rel);
    let ext = p.extension()?.to_str()?;
    let (_, to) = CONVERTIBLE.iter().find(|(from, _)| from.eq_ignore_ascii_case(ext))?;
    Some(p.with_extension(to).to_string_lossy().into_owned())
}

/// Résultat d'une installation de fichier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installed {
    /// Téléchargé (ou repris) puis mis en place.
    Downloaded,
    /// Déjà présent avec la bonne taille : rien à faire.
    AlreadyPresent,
}

/// Chemin du catalogue vérifié : relatif, composants normaux seulement (ni `..`, ni absolu), au
/// moins deux composants (`console/fichier`) : jamais le dossier d'une console lui-même.
pub fn checked_rel(rel: &str) -> Result<&Path, InstallError> {
    let p = Path::new(rel);
    if rel.is_empty() || !p.components().all(|c| matches!(c, Component::Normal(_))) || p.components().count() < 2 {
        return Err(InstallError::BadPath(rel.into()));
    }
    Ok(p)
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

/// Refuse tout lien symbolique entre `roms` (exclu) et `full` (inclus). Les composants absents
/// ne sont pas un problème (ils seront créés par nous, donc jamais des liens).
fn guard(roms: &Path, full: &Path) -> Result<(), InstallError> {
    let rel = full.strip_prefix(roms).map_err(|_| InstallError::BadPath(full.display().to_string()))?;
    let mut cur = roms.to_path_buf();
    for c in rel.components() {
        cur.push(c);
        match fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() => return Err(InstallError::SymLink(cur.display().to_string())),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(io("lecture du dossier des jeux", e)),
        }
    }
    Ok(())
}

/// Supprime un fichier ou un dossier (jamais en suivant un lien : `symlink_metadata`).
fn remove_any(p: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(p),
        Ok(_) => fs::remove_file(p),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Octets libres sur le disque qui porte `path` (ou son plus proche ancêtre existant). `None` si
/// la mesure est impossible (le contrôle est alors sauté).
pub fn free_space(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let mut p = path;
        while !p.exists() {
            p = p.parent()?;
        }
        let c = std::ffi::CString::new(p.as_os_str().as_bytes()).ok()?;
        let mut st = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY : `c` est une chaîne C valide ; `st` n'est lu que si statvfs renvoie 0 (initialisé).
        let ok = unsafe { libc::statvfs(c.as_ptr(), st.as_mut_ptr()) } == 0;
        if !ok {
            return None;
        }
        let st = unsafe { st.assume_init() };
        // Les types de statvfs varient selon la plateforme (u64 sur Linux x86_64) : conversion voulue.
        #[allow(clippy::useless_conversion)]
        let free = Some(u64::from(st.f_bavail).saturating_mul(u64::from(st.f_frsize)));
        free
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

fn check_space(dest: &Path, needed: u64) -> Result<(), InstallError> {
    match free_space(dest) {
        Some(free) if free < needed => Err(InstallError::NoSpace { needed, free }),
        _ => Ok(()),
    }
}

/// Appels de progression espacés (au plus tous les 100 ms), plus un appel final garanti.
struct Ticker<'a> {
    cb: &'a mut dyn FnMut(u64, u64),
    total: u64,
    last: Instant,
}

impl<'a> Ticker<'a> {
    fn new(cb: &'a mut dyn FnMut(u64, u64), total: u64) -> Self {
        let last = Instant::now().checked_sub(Duration::from_secs(1)).unwrap_or_else(Instant::now);
        Self { cb, total, last }
    }
    fn set_total(&mut self, total: u64) {
        self.total = total;
    }
    fn tick(&mut self, done: u64) {
        if self.last.elapsed() >= Duration::from_millis(100) {
            self.last = Instant::now();
            (self.cb)(done.min(self.total), self.total);
        }
    }
    fn finish(&mut self, done: u64) {
        (self.cb)(done.min(self.total), self.total);
    }
}

/// Annulation d'une installation : réveille aussitôt les attentes (`Condvar`) et les lectures
/// bloquées (message d'erreur poussé dans leur canal). Aucun drapeau relu en boucle.
#[derive(Clone, Default)]
pub struct Cancel(Arc<CancelInner>);

#[derive(Default)]
struct CancelInner {
    state: Mutex<CancelState>,
    cv: std::sync::Condvar,
}

#[derive(Default)]
struct CancelState {
    cancelled: bool,
    /// Réveils à faire à l'annulation (lecture bloquée, requête en attente).
    wakers: Vec<Box<dyn FnOnce() + Send>>,
}

impl std::fmt::Debug for Cancel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cancel").field("cancelled", &self.is_cancelled()).finish()
    }
}

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, CancelState> {
        self.0.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Annule : les lectures en attente reçoivent une erreur, les attentes se terminent.
    pub fn cancel(&self) {
        let mut s = self.lock();
        s.cancelled = true;
        for wake in s.wakers.drain(..) {
            wake();
        }
        self.0.cv.notify_all();
    }

    pub fn is_cancelled(&self) -> bool {
        self.lock().cancelled
    }

    /// Dort au plus `d` ; `true` si l'annulation est arrivée entre-temps (réveil immédiat).
    pub fn wait(&self, d: Duration) -> bool {
        let g = self.lock();
        let (g, _) = self.0.cv.wait_timeout_while(g, d, |s| !s.cancelled).unwrap_or_else(std::sync::PoisonError::into_inner);
        g.cancelled
    }

    /// Inscrit un réveil exécuté à l'annulation (tout de suite si elle a déjà eu lieu).
    fn on_cancel(&self, wake: impl FnOnce() + Send + 'static) {
        let mut s = self.lock();
        if s.cancelled {
            drop(s);
            wake();
        } else {
            s.wakers.push(Box::new(wake));
        }
    }
}

/// Corps de réponse lu par un fil dédié ; la lecture rend la main sur annulation ou inactivité.
struct Stream {
    rx: mpsc::Receiver<std::io::Result<Vec<u8>>>,
    buf: Vec<u8>,
    pos: usize,
    idle: Duration,
    last: Instant,
    cancel: Cancel,
}

impl Stream {
    fn new(resp: ureq::http::Response<ureq::Body>, idle: Duration, cancel: Cancel) -> Self {
        let (tx, rx) = mpsc::sync_channel(4);
        let wake = tx.clone();
        // Canal plein = des données arrivent : la lecture verra l'annulation au morceau suivant.
        cancel.on_cancel(move || {
            let _ = wake.try_send(Err(std::io::Error::other("annulé")));
        });
        std::thread::spawn(move || {
            let mut body = resp.into_body();
            let mut reader = body.as_reader();
            loop {
                let mut chunk = vec![0u8; 64 * 1024];
                match reader.read(&mut chunk) {
                    // Fin explicite (morceau vide) : le canal reste ouvert tant que l'annulation peut
                    // y écrire, la fin ne peut donc pas se lire à sa fermeture.
                    Ok(0) => {
                        let _ = tx.send(Ok(Vec::new()));
                        break;
                    }
                    Ok(n) => {
                        chunk.truncate(n);
                        if tx.send(Ok(chunk)).is_err() {
                            break; // l'installateur a abandonné
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                }
            }
        });
        Self { rx, buf: Vec::new(), pos: 0, idle, last: Instant::now(), cancel }
    }
}

impl Read for Stream {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if self.cancel.is_cancelled() {
                return Err(std::io::Error::other("annulé"));
            }
            if self.pos < self.buf.len() {
                let n = out.len().min(self.buf.len() - self.pos);
                out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
                self.pos += n;
                return Ok(n);
            }
            // Une seule échéance : la fin du délai d'inactivité ; l'annulation arrive par le canal.
            match self.rx.recv_timeout(self.idle.saturating_sub(self.last.elapsed())) {
                Ok(Ok(chunk)) if chunk.is_empty() => return Ok(0),
                Ok(Ok(chunk)) => {
                    self.buf = chunk;
                    self.pos = 0;
                    self.last = Instant::now();
                }
                Ok(Err(e)) => return Err(e),
                Err(RecvTimeoutError::Timeout) => {
                    if self.last.elapsed() >= self.idle {
                        return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, format!("aucun octet reçu depuis {:.0?}", self.idle)));
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return Ok(0),
            }
        }
    }
}

/// Erreur de lecture du flux : annulation demandée, ou interruption (reprise possible).
fn read_err(e: &std::io::Error, cancel: &Cancel) -> InstallError {
    if cancel.is_cancelled() { InstallError::Cancelled } else { InstallError::Interrupted(e.to_string()) }
}

pub struct Installer<'a> {
    pub client: &'a Client,
    /// Dossier des ROM de l'appareil.
    pub roms: &'a Path,
    /// Délai sans octet reçu avant d'abandonner.
    pub idle: Duration,
    /// Mis à `true` (depuis un autre fil) pour annuler l'installation en cours.
    pub cancel: Cancel,
    /// Formats que l'émulateur sait lire (`nsp`, `xci`…) : le serveur convertit un NSZ / XCZ si besoin.
    /// Vide = fichier servi tel quel.
    pub formats: Vec<String>,
    /// Appelé à chaque réponse « conversion en cours » du serveur (interface : « Préparation sur le serveur… »).
    on_prepare: Option<PrepareCb<'a>>,
    /// Plus longue attente entre deux demandes pendant une conversion (le serveur propose 5 s).
    pub retry_cap: Duration,
    /// Abandon si la conversion n'avance plus du tout depuis ce délai (elle continue côté serveur).
    pub prepare_stall: Duration,
}

type PrepareCb<'a> = Box<dyn Fn(&ConvertProgress) + 'a>;

/// Plus longue attente entre deux demandes pendant une conversion.
pub const RETRY_CAP: Duration = Duration::from_secs(30);
/// Durée sans aucun changement de la conversion avant d'abandonner.
pub const PREPARE_STALL: Duration = Duration::from_secs(600);

impl<'a> Installer<'a> {
    pub fn new(client: &'a Client, roms: &'a Path) -> Self {
        Self { client, roms, idle: IDLE_TIMEOUT, cancel: Cancel::new(), formats: Vec::new(), on_prepare: None, retry_cap: RETRY_CAP, prepare_stall: PREPARE_STALL }
    }

    /// Formats lus par l'émulateur du jeu (fiche : `formats`).
    pub fn with_formats(mut self, formats: Vec<String>) -> Self {
        self.formats = formats;
        self
    }

    pub fn with_prepare(mut self, f: impl Fn(&ConvertProgress) + 'a) -> Self {
        self.on_prepare = Some(Box::new(f));
        self
    }

    pub fn with_retry_cap(mut self, cap: Duration) -> Self {
        self.retry_cap = cap;
        self
    }

    pub fn with_prepare_stall(mut self, stall: Duration) -> Self {
        self.prepare_stall = stall;
        self
    }

    /// Chemin local d'un fichier du catalogue : converti (`.nsp` / `.xci`) si le serveur doit le convertir.
    pub fn local_rel(&self, file: &GameFile) -> Result<PathBuf, InstallError> {
        let rel = checked_rel(&file.path)?;
        Ok(match conversion_target(&file.format, &self.formats) {
            Some(to) => rel.with_extension(to),
            None => rel.to_path_buf(),
        })
    }

    /// Demande le fichier ; tant que le serveur répond « conversion en cours » (202), attend le
    /// délai demandé puis redemande (annulation prise en compte). Abandonne si rien ne bouge.
    fn fetch(&self, game: &str, file: &GameFile, from: u64, if_range: Option<&str>) -> Result<ureq::http::Response<ureq::Body>, InstallError> {
        let mut last: Option<(String, u64, Option<u8>)> = None;
        let mut since = Instant::now();
        loop {
            if self.cancel.is_cancelled() {
                return Err(InstallError::Cancelled);
            }
            // Requête (éventuellement longue côté serveur) dans un fil : une annulation rend la main
            // tout de suite, sans attendre la réponse (le fil se termine seul, réponse ignorée).
            let (tx, rx) = mpsc::channel();
            let on_cancel = tx.clone();
            self.cancel.on_cancel(move || {
                let _ = on_cancel.send(None);
            });
            let (c, g, fid, fm, ir) = (self.client.clone(), game.to_string(), file.id, self.formats.clone(), if_range.map(str::to_string));
            std::thread::spawn(move || {
                let _ = tx.send(Some(c.download_as(&g, fid, from, ir.as_deref(), &fm)));
            });
            let Ok(Some(answer)) = rx.recv() else { return Err(InstallError::Cancelled) };
            let mut resp = match answer {
                Err(ApiError::Http { status: 500 | 503, body }) if conversion_target(&file.format, &self.formats).is_some() => return Err(InstallError::Convert(body)),
                r => r?,
            };
            if resp.status().as_u16() != 202 {
                return Ok(resp);
            }
            let wait = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok()).map_or(5, |s| s.max(1));
            // Serveur à requête longue (`x-ludo-attente: 1`) : il a déjà attendu le changement, on
            // redemande aussitôt ; sinon (serveur ancien) on respecte Retry-After.
            let long_poll = resp.headers().get("x-ludo-attente").is_some_and(|v| v.as_bytes() == b"1");
            let wait = if long_poll { Duration::ZERO } else { Duration::from_secs(wait).min(self.retry_cap) };
            let body = resp.body_mut().read_to_string().unwrap_or_default();
            let p: ConvertProgress = serde_json::from_str(&body).unwrap_or(ConvertProgress { state: "converting".into(), elapsed_s: 0, written_bytes: 0, source_bytes: 0, percent: None });
            if let Some(cb) = &self.on_prepare {
                cb(&p);
            }
            let sig = (p.state.clone(), p.written_bytes, p.percent);
            if p.state == "queued" {
                // En file derrière une autre conversion : rien à reprocher à la conversion, le délai ne court pas.
                last = None;
                since = Instant::now();
            } else if last.as_ref() != Some(&sig) {
                last = Some(sig);
                since = Instant::now();
            } else if since.elapsed() >= self.prepare_stall {
                return Err(InstallError::Interrupted(format!("préparation sur le serveur à l'arrêt depuis {:.0?} ; toujours en cours côté serveur, reprise à la prochaine tentative", self.prepare_stall)));
            }
            // Attente demandée par le serveur (Retry-After), interrompue aussitôt par une annulation.
            if self.cancel.wait(wait) {
                return Err(InstallError::Cancelled);
            }
        }
    }

    pub fn with_cancel(mut self, cancel: Cancel) -> Self {
        self.cancel = cancel;
        self
    }

    pub fn with_idle(mut self, idle: Duration) -> Self {
        self.idle = idle;
        self
    }

    /// Installe un fichier du catalogue. `progress(octets reçus, total du catalogue)`.
    pub fn install(&self, game: &str, file: &GameFile, progress: &mut dyn FnMut(u64, u64)) -> Result<Installed, InstallError> {
        if self.cancel.is_cancelled() {
            return Err(InstallError::Cancelled);
        }
        let dest = self.roms.join(self.local_rel(file)?);
        for p in [dest.clone(), with_suffix(&dest, ".part"), with_suffix(&dest, ".part.etag"), with_suffix(&dest, ".old")] {
            guard(self.roms, &p)?;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| io("création du dossier", e))?;
        }
        let mut ticker = Ticker::new(progress, file.size);
        if file.format == "dir" {
            self.install_dir(game, file, &dest, &mut ticker)
        } else {
            self.install_file(game, file, &dest, conversion_target(&file.format, &self.formats).is_some(), &mut ticker)
        }
    }

    /// `converted` : le serveur convertit le fichier ; sa taille n'est connue qu'à la réponse (celle du
    /// catalogue est celle du fichier compressé), et le fichier présent est jugé complet (il n'est
    /// mis en place qu'une fois entier).
    fn install_file(&self, game: &str, file: &GameFile, dest: &Path, converted: bool, t: &mut Ticker) -> Result<Installed, InstallError> {
        if fs::symlink_metadata(dest).is_ok_and(|m| m.is_file() && (if converted { m.len() > 0 } else { m.len() == file.size })) {
            let size = fs::metadata(dest).map_or(file.size, |m| m.len());
            t.set_total(size);
            t.finish(size);
            return Ok(Installed::AlreadyPresent);
        }
        let mut expected = file.size;
        let part = with_suffix(dest, ".part");
        let etag_path = with_suffix(dest, ".part.etag");
        let discard = || {
            let _ = remove_any(&part);
            let _ = remove_any(&etag_path);
        };
        let mut start = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let saved_etag = fs::read_to_string(&etag_path).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        // Reprise seulement avec un validateur connu : sinon on ne sait pas si le fichier a changé.
        if (!converted && start > file.size) || (start > 0 && saved_etag.is_none()) {
            discard();
            start = 0;
        }
        let mut retried = false;
        loop {
            if !converted && start == file.size && start > 0 {
                break; // le partiel est déjà complet (interruption juste avant le renommage)
            }
            if !converted {
                check_space(dest, file.size - start)?;
            }
            let resp = self.fetch(game, file, start, saved_etag.as_deref())?;
            let status = resp.status().as_u16();
            if converted && status == 416 && start > 0 && content_range_total(&resp) == Some(start) {
                expected = start;
                break; // le partiel converti est déjà complet
            }
            if converted && matches!(status, 200 | 206) {
                // Le serveur a-t-il vraiment converti ? Sinon un .nsz porterait le nom d'un .nsp.
                let Some(total) = served_total(&resp, status) else {
                    return Err(InstallError::Convert("réponse du serveur sans taille du fichier converti".into()));
                };
                if resp.headers().get("x-ludo-format").is_none() {
                    return Err(InstallError::Convert("le serveur n'a pas converti le fichier (version trop ancienne ?)".into()));
                }
                expected = total;
                t.set_total(total);
                check_space(dest, if status == 206 { total.saturating_sub(start) } else { total })?;
            }
            let restart = match status {
                416 => true,
                206 => !content_range_starts_at(&resp, start),
                _ => false,
            };
            if restart && start > 0 && !retried {
                // Reprise refusée ou incohérente : on repart de zéro, une seule fois.
                retried = true;
                discard();
                start = 0;
                continue;
            }
            if restart || is_tar(&resp) {
                return Err(InstallError::Api(ApiError::Http { status, body: "réponse de téléchargement inattendue".into() }));
            }
            // 200 : le serveur renvoie tout depuis le début (fichier changé, ou reprise non gérée).
            let resume = status == 206;
            let mut out = if resume {
                OpenOptions::new().append(true).open(&part)
            } else {
                let etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).map(String::from);
                match etag {
                    Some(v) => fs::write(&etag_path, v).map_err(|e| io("écriture du validateur", e))?,
                    None => {
                        let _ = remove_any(&etag_path);
                    }
                }
                File::create(&part)
            }
            .map_err(|e| io("ouverture du fichier partiel", e))?;
            let mut done = if resume { start } else { 0 };
            if !resume {
                check_space(dest, expected)?;
            }
            let mut stream = Stream::new(resp, self.idle, self.cancel.clone());
            let mut buf = vec![0u8; 256 * 1024];
            loop {
                let n = match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) => return Err(read_err(&e, &self.cancel)),
                };
                out.write_all(&buf[..n]).map_err(|e| io("écriture", e))?;
                done += n as u64;
                if done > expected {
                    drop(out);
                    discard();
                    return Err(InstallError::SizeMismatch { expected, got: done });
                }
                t.tick(done);
            }
            out.sync_all().map_err(|e| io("écriture", e))?;
            break;
        }
        let got = fs::metadata(&part).map_err(|e| io("lecture du fichier partiel", e))?.len();
        if got > expected {
            discard();
            return Err(InstallError::SizeMismatch { expected, got });
        }
        if got < expected {
            // Flux plus court que prévu : on garde le partiel, une reprise est possible.
            return Err(InstallError::Interrupted(format!("{} reçus sur {}", human(got), human(expected))));
        }
        fs::rename(&part, dest).map_err(|e| io("mise en place", e))?;
        let _ = remove_any(&etag_path);
        t.finish(expected);
        Ok(Installed::Downloaded)
    }

    fn install_dir(&self, game: &str, file: &GameFile, dest: &Path, t: &mut Ticker) -> Result<Installed, InstallError> {
        if dir_size(dest).is_some_and(|s| s == file.size) {
            t.finish(file.size);
            return Ok(Installed::AlreadyPresent);
        }
        // Un dossier existant est gardé jusqu'à la mise en place du nouveau : deux fois la place.
        let replacing = fs::symlink_metadata(dest).is_ok();
        check_space(dest, if replacing { file.size.saturating_mul(2) } else { file.size })?;
        let part = with_suffix(dest, ".part");
        remove_any(&part).map_err(|e| io("nettoyage du dossier partiel", e))?;
        let resp = self.client.download(game, file.id, 0, None)?;
        if resp.status().as_u16() != 200 || !is_tar(&resp) {
            return Err(InstallError::Archive("le serveur n'a pas envoyé d'archive pour ce dossier".into()));
        }
        // Taille réelle annoncée par le serveur (somme des fichiers au moment de l'envoi).
        let expected = resp.headers().get("x-ludo-total-bytes").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok()).unwrap_or(file.size);
        fs::create_dir_all(&part).map_err(|e| io("création du dossier partiel", e))?;
        let stream = Stream::new(resp, self.idle, self.cancel.clone());
        let got = match extract_tar(stream, &part, t, expected, &self.cancel) {
            Ok(n) => n,
            Err(e) => {
                let _ = remove_any(&part);
                return Err(e);
            }
        };
        if got != expected {
            let _ = remove_any(&part);
            return Err(InstallError::SizeMismatch { expected, got });
        }
        // Remplacement : l'ancienne version est mise de côté, puis supprimée une fois la nouvelle en place.
        let old = with_suffix(dest, ".old");
        if replacing {
            remove_any(&old).map_err(|e| io("nettoyage de l'ancienne version", e))?;
            fs::rename(dest, &old).map_err(|e| io("remplacement", e))?;
        }
        if let Err(e) = fs::rename(&part, dest) {
            if replacing {
                let _ = fs::rename(&old, dest);
            }
            return Err(io("mise en place", e));
        }
        if replacing {
            let _ = remove_any(&old);
        }
        t.finish(file.size);
        Ok(Installed::Downloaded)
    }
}

/// Taille totale du fichier converti d'après la réponse : `Content-Range` (206) ou `X-Ludo-Total-Bytes` / `Content-Length` (200).
fn served_total(resp: &ureq::http::Response<ureq::Body>, status: u16) -> Option<u64> {
    if status == 206 {
        return content_range_total(resp);
    }
    let h = |n: &str| resp.headers().get(n).and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok());
    h("x-ludo-total-bytes").or_else(|| h("content-length"))
}

/// Total d'un `Content-Range` (`bytes a-b/total` ou `bytes */total`).
fn content_range_total(resp: &ureq::http::Response<ureq::Body>) -> Option<u64> {
    resp.headers().get("content-range")?.to_str().ok()?.rsplit('/').next()?.trim().parse().ok()
}

fn is_tar(resp: &ureq::http::Response<ureq::Body>) -> bool {
    resp.headers().get("x-ludo-archive").is_some_and(|v| v == "tar")
}

/// `Content-Range: bytes <start>-<fin>/<total>` commence-t-il bien à `start` ?
fn content_range_starts_at(resp: &ureq::http::Response<ureq::Body>, start: u64) -> bool {
    resp.headers()
        .get("content-range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes "))
        .and_then(|v| v.split('-').next())
        .and_then(|v| v.trim().parse::<u64>().ok())
        == Some(start)
}

/// Taille cumulée des fichiers réguliers d'un dossier (liens symboliques ignorés) ; `None` s'il n'existe pas.
fn dir_size(dir: &Path) -> Option<u64> {
    if !fs::symlink_metadata(dir).ok()?.is_dir() {
        return None;
    }
    let (mut total, mut stack) = (0, vec![dir.to_path_buf()]);
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).ok()?.filter_map(Result::ok) {
            let ft = e.file_type().ok()?;
            if ft.is_dir() {
                stack.push(e.path());
            } else if ft.is_file() {
                total += e.metadata().ok()?.len();
            }
        }
    }
    Some(total)
}

/// Extrait un tar dans `into` (dossier vide) : seuls fichiers réguliers et dossiers sont acceptés ;
/// chemins absolus, `..`, liens (symboliques ou durs) et autres types sont refusés. S'arrête dès
/// que les octets extraits dépassent `max`. Renvoie le nombre d'octets de fichiers extraits.
fn extract_tar(reader: impl Read, into: &Path, t: &mut Ticker, max: u64, cancel: &Cancel) -> Result<u64, InstallError> {
    let mut ar = tar::Archive::new(reader);
    let mut done = 0u64;
    let entries = ar.entries().map_err(|e| read_err(&e, cancel))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| read_err(&e, cancel))?;
        let raw = entry.path().map_err(|e| InstallError::Archive(format!("nom illisible : {e}")))?.into_owned();
        let shown = raw.display().to_string();
        // Composants normaux seulement (« ./ » initial toléré) : ni absolu, ni « .. ».
        let mut rel = PathBuf::new();
        for c in raw.components() {
            match c {
                Component::Normal(n) => rel.push(n),
                Component::CurDir => {}
                _ => return Err(InstallError::Archive(format!("chemin dangereux : {shown}"))),
            }
        }
        if rel.as_os_str().is_empty() {
            continue; // l'entrée « . » elle-même
        }
        let target = into.join(&rel);
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            fs::create_dir_all(&target).map_err(|e| io("création de dossier", e))?;
        } else if kind.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| io("création de dossier", e))?;
            }
            // `create_new` : jamais d'écriture à travers un lien ou par-dessus un fichier existant.
            let mut out = OpenOptions::new().write(true).create_new(true).open(&target).map_err(|e| io(&format!("création de {shown}"), e))?;
            let mut buf = vec![0u8; 256 * 1024];
            loop {
                let n = entry.read(&mut buf).map_err(|e| read_err(&e, cancel))?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n]).map_err(|e| io("écriture", e))?;
                done += n as u64;
                if done > max {
                    return Err(InstallError::SizeMismatch { expected: max, got: done });
                }
                t.tick(done);
            }
        } else {
            return Err(InstallError::Archive(format!("type d'entrée refusé (lien ou spécial) : {shown}")));
        }
    }
    Ok(done)
}

/// Supprime de l'appareil les fichiers de jeu du catalogue (base, mises à jour, DLC), ainsi que
/// leurs restes de téléchargement (`.part`, `.part.etag`, `.old`). Ne touche à rien d'autre que
/// ces chemins sous `roms` (jamais de sauvegarde) et refuse, avant de supprimer quoi que ce soit,
/// tout chemin dont un composant est un lien symbolique. Retire ensuite les dossiers intermédiaires
/// devenus vides (le dossier de la console reste). Renvoie le nombre d'éléments supprimés.
pub fn uninstall(roms: &Path, files: &[GameFile]) -> Result<usize, InstallError> {
    let mut plan = Vec::new();
    for f in files {
        let path = roms.join(checked_rel(&f.path)?);
        // Le fichier peut avoir été installé converti par le serveur (`.nsz` → `.nsp`) : mêmes restes.
        let mut variants = vec![path.clone()];
        if let Some(c) = converted_rel(&f.path) {
            variants.push(roms.join(checked_rel(&c)?));
        }
        for v in variants {
            let targets = [v.clone(), with_suffix(&v, ".part"), with_suffix(&v, ".part.etag"), with_suffix(&v, ".old")];
            for t in &targets {
                guard(roms, t)?;
            }
            plan.push((v, targets));
        }
    }
    let mut removed = 0;
    for (path, targets) in plan {
        for target in targets {
            if fs::symlink_metadata(&target).is_err() {
                continue;
            }
            remove_any(&target).map_err(|e| io(&format!("suppression de {}", target.display()), e))?;
            removed += 1;
        }
        // Dossiers parents vides, sous le dossier de la console (profondeur ≥ 2).
        let mut parent = path.parent();
        while let Some(p) = parent {
            let depth = p.strip_prefix(roms).map(|r| r.components().count()).unwrap_or(0);
            if depth < 2 || fs::remove_dir(p).is_err() {
                break;
            }
            parent = p.parent();
        }
    }
    Ok(removed)
}
