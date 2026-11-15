//! Installation de jeux sur l'appareil : téléchargement d'un fichier du catalogue sous
//! `<roms>/<chemin relatif>`, avec reprise, contrôle de taille et renommage atomique ; extraction
//! sûre du tar d'un dossier (Wii U, Switch 2 extrait) ; désinstallation (ROM seulement).
//!
//! Jamais de sauvegarde ici : seuls les chemins du catalogue, sous le dossier des ROM de l'appareil,
//! sont lus, écrits ou supprimés.

use crate::api::{ApiError, Client};
use ludo_proto::GameFile;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("{0}")]
    Api(#[from] ApiError),
    #[error("chemin refusé dans le catalogue : {0}")]
    BadPath(String),
    #[error("espace disque insuffisant : {} nécessaires, {} libres", human(*needed), human(*free))]
    NoSpace { needed: u64, free: u64 },
    #[error("taille inattendue après téléchargement : {} reçus, {} attendus (le catalogue du serveur a peut-être changé)", human(*got), human(*expected))]
    SizeMismatch { expected: u64, got: u64 },
    #[error("téléchargement interrompu ({0}) ; il reprendra là où il s'est arrêté")]
    Interrupted(String),
    #[error("archive refusée : {0}")]
    Archive(String),
    #[error("disque : {0}")]
    Io(String),
}

fn io(ctx: &str, e: std::io::Error) -> InstallError {
    InstallError::Io(format!("{ctx} : {e}"))
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

/// Résultat d'une installation de fichier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installed {
    /// Téléchargé (ou repris) puis mis en place.
    Downloaded,
    /// Déjà présent avec la bonne taille : rien à faire.
    AlreadyPresent,
}

/// Chemin du catalogue vérifié : relatif, composants normaux seulement (ni `..`, ni absolu).
pub fn checked_rel(rel: &str) -> Result<&Path, InstallError> {
    let p = Path::new(rel);
    if rel.is_empty() || !p.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(InstallError::BadPath(rel.into()));
    }
    Ok(p)
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
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

pub struct Installer<'a> {
    pub client: &'a Client,
    /// Dossier des ROM de l'appareil.
    pub roms: &'a Path,
}

impl Installer<'_> {
    /// Installe un fichier du catalogue. `progress(octets reçus, total du catalogue)`.
    pub fn install(&self, game: &str, file: &GameFile, progress: &mut dyn FnMut(u64, u64)) -> Result<Installed, InstallError> {
        let dest = self.roms.join(checked_rel(&file.path)?);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| io("création du dossier", e))?;
        }
        let mut ticker = Ticker::new(progress, file.size);
        if file.format == "dir" {
            self.install_dir(game, file, &dest, &mut ticker)
        } else {
            self.install_file(game, file, &dest, &mut ticker)
        }
    }

    fn install_file(&self, game: &str, file: &GameFile, dest: &Path, t: &mut Ticker) -> Result<Installed, InstallError> {
        if fs::metadata(dest).is_ok_and(|m| m.is_file() && m.len() == file.size) {
            t.finish(file.size);
            return Ok(Installed::AlreadyPresent);
        }
        let part = with_suffix(dest, ".part");
        let mut start = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if start > file.size {
            fs::remove_file(&part).map_err(|e| io("nettoyage du fichier partiel", e))?;
            start = 0;
        }
        let mut retried = false;
        loop {
            if start == file.size {
                break; // le partiel est déjà complet (interruption juste avant le renommage)
            }
            check_space(dest, file.size - start)?;
            let mut resp = self.client.download(game, file.id, start)?;
            let status = resp.status().as_u16();
            let restart = match status {
                416 => true,
                206 => !content_range_starts_at(&resp, start),
                _ => false,
            };
            if restart && start > 0 && !retried {
                // Reprise refusée ou incohérente : on repart de zéro, une seule fois.
                retried = true;
                let _ = fs::remove_file(&part);
                start = 0;
                continue;
            }
            if restart || is_tar(&resp) {
                return Err(InstallError::Api(ApiError::Http { status, body: "réponse de téléchargement inattendue".into() }));
            }
            // 200 : le serveur renvoie tout depuis le début, même si une reprise était demandée.
            let resume = status == 206;
            let mut out = if resume {
                OpenOptions::new().append(true).open(&part)
            } else {
                File::create(&part)
            }
            .map_err(|e| io("ouverture du fichier partiel", e))?;
            let mut done = if resume { start } else { 0 };
            if !resume {
                check_space(dest, file.size)?;
            }
            let mut buf = vec![0u8; 256 * 1024];
            let mut body = resp.body_mut().as_reader();
            loop {
                let n = match body.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) => return Err(InstallError::Interrupted(e.to_string())),
                };
                out.write_all(&buf[..n]).map_err(|e| io("écriture", e))?;
                done += n as u64;
                t.tick(done);
            }
            out.sync_all().map_err(|e| io("écriture", e))?;
            break;
        }
        let got = fs::metadata(&part).map_err(|e| io("lecture du fichier partiel", e))?.len();
        if got != file.size {
            let _ = fs::remove_file(&part);
            return Err(InstallError::SizeMismatch { expected: file.size, got });
        }
        fs::rename(&part, dest).map_err(|e| io("mise en place", e))?;
        t.finish(file.size);
        Ok(Installed::Downloaded)
    }

    fn install_dir(&self, game: &str, file: &GameFile, dest: &Path, t: &mut Ticker) -> Result<Installed, InstallError> {
        if dir_size(dest).is_some_and(|s| s == file.size) {
            t.finish(file.size);
            return Ok(Installed::AlreadyPresent);
        }
        check_space(dest, file.size)?;
        let part = with_suffix(dest, ".part");
        if part.exists() {
            fs::remove_dir_all(&part).map_err(|e| io("nettoyage du dossier partiel", e))?;
        }
        let mut resp = self.client.download(game, file.id, 0)?;
        if resp.status().as_u16() != 200 || !is_tar(&resp) {
            return Err(InstallError::Archive("le serveur n'a pas envoyé d'archive pour ce dossier".into()));
        }
        fs::create_dir_all(&part).map_err(|e| io("création du dossier partiel", e))?;
        let got = match extract_tar(resp.body_mut().as_reader(), &part, t) {
            Ok(n) => n,
            Err(e) => {
                let _ = fs::remove_dir_all(&part);
                return Err(e);
            }
        };
        if got != file.size {
            let _ = fs::remove_dir_all(&part);
            return Err(InstallError::SizeMismatch { expected: file.size, got });
        }
        // Remplacement : l'ancienne version est mise de côté, puis supprimée une fois la nouvelle en place.
        let old = with_suffix(dest, ".old");
        let had_old = dest.exists();
        if had_old {
            let _ = fs::remove_dir_all(&old);
            fs::rename(dest, &old).map_err(|e| io("remplacement", e))?;
        }
        if let Err(e) = fs::rename(&part, dest) {
            if had_old {
                let _ = fs::rename(&old, dest);
            }
            return Err(io("mise en place", e));
        }
        if had_old {
            let _ = fs::remove_dir_all(&old);
        }
        t.finish(file.size);
        Ok(Installed::Downloaded)
    }
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

/// Taille cumulée des fichiers d'un dossier (liens symboliques ignorés) ; `None` s'il n'existe pas.
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

/// Copie un flux vers un fichier en comptant les octets écrits (progression).
fn copy_counting(src: &mut impl Read, dst: &mut File, done: &mut u64, t: &mut Ticker) -> std::io::Result<()> {
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = src.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        dst.write_all(&buf[..n])?;
        *done += n as u64;
        t.tick(*done);
    }
}

/// Extrait un tar dans `into` (dossier vide) : seuls fichiers réguliers et dossiers sont acceptés ;
/// chemins absolus, `..`, liens (symboliques ou durs) et autres types sont refusés. Renvoie le
/// nombre d'octets de fichiers extraits.
fn extract_tar(reader: impl Read, into: &Path, t: &mut Ticker) -> Result<u64, InstallError> {
    let mut ar = tar::Archive::new(reader);
    let mut done = 0u64;
    let entries = ar.entries().map_err(|e| InstallError::Interrupted(e.to_string()))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| InstallError::Interrupted(e.to_string()))?;
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
            copy_counting(&mut entry, &mut out, &mut done, t).map_err(|e| InstallError::Interrupted(e.to_string()))?;
        } else {
            return Err(InstallError::Archive(format!("type d'entrée refusé (lien ou spécial) : {shown}")));
        }
    }
    Ok(done)
}

/// Supprime de l'appareil les fichiers de jeu du catalogue (base, mises à jour, DLC), ainsi que
/// leurs restes de téléchargement (`.part`). Ne touche à rien d'autre que ces chemins sous `roms`
/// (jamais de sauvegarde), puis retire les dossiers intermédiaires devenus vides (le dossier de la
/// console reste). Renvoie le nombre d'éléments supprimés.
pub fn uninstall(roms: &Path, files: &[GameFile]) -> Result<usize, InstallError> {
    let mut removed = 0;
    for f in files {
        let rel = checked_rel(&f.path)?;
        let path = roms.join(rel);
        for target in [path.clone(), with_suffix(&path, ".part"), with_suffix(&path, ".old")] {
            let Ok(meta) = fs::symlink_metadata(&target) else { continue };
            let res = if meta.is_dir() { fs::remove_dir_all(&target) } else { fs::remove_file(&target) };
            res.map_err(|e| io(&format!("suppression de {}", target.display()), e))?;
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
