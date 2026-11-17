//! Conversion côté serveur des Switch compressés (NSZ → NSP, XCZ → XCI) pour les émulateurs qui
//! ne savent pas les lire (Eden). Conception 4.5 : pas de copie décompressée permanente, seulement
//! un petit cache des derniers jeux servis, dans un dossier propre (hors volume sauvegardé).
//!
//! Disposition du cache (`LUDO_CACHE`) :
//! - `ready/<clé>/file.<ext>` : conversions terminées (date de modification = dernier accès, pour
//!   l'éviction LRU) ;
//! - `work/<clé>/` : conversion en cours, supprimée en cas d'échec et au démarrage (aucun fichier
//!   partiel ne reste jamais dans `ready/`) ;
//! - `home/`, `tmp/` : HOME et TMPDIR du convertisseur (la racine du conteneur est en lecture seule).
//!
//! Une seule conversion à la fois (sémaphore) ; une même conversion demandée plusieurs fois n'est
//! lancée qu'une fois (table des tâches).

use ludo_proto::ConvertProgress;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;

/// Réglages de la conversion.
#[derive(Debug, Clone)]
pub struct ConvertConfig {
    /// Dossier du cache (volume dédié, non sauvegardé).
    pub cache: PathBuf,
    /// Taille maximale des conversions gardées (octets).
    pub max_bytes: u64,
    /// Fichier de clés de la console (`prod.keys`) ; absent = conversion indisponible.
    pub keys: Option<PathBuf>,
    /// Programme de conversion, appelé `<programme> -D --keys <clés> -o <dossier> <fichier>` (nsz) ;
    /// les tests le remplacent par un petit script.
    pub command: String,
    /// Durée maximale d'une conversion.
    pub timeout: Duration,
    /// Après un échec, durée pendant laquelle la même conversion n'est pas relancée (une cause
    /// déterministe — clés invalides… — ne doit pas coûter des heures de CPU à chaque demande).
    pub failure_backoff: Duration,
    /// Durée maximale d'une requête longue (`?attendre=1`) sans changement de la conversion.
    pub long_poll: Duration,
}

impl ConvertConfig {
    pub const DEFAULT_MAX_BYTES: u64 = 80 * 1_000_000_000;

    pub fn new(cache: PathBuf) -> Self {
        Self { cache, max_bytes: Self::DEFAULT_MAX_BYTES, keys: None, command: "nsz".into(), timeout: Duration::from_secs(3 * 3600), failure_backoff: Duration::from_secs(600), long_poll: Duration::from_secs(25) }
    }
}

/// Utilisation d'une conversion du cache (envoi en cours) : tant qu'elle existe, l'éviction l'épargne.
#[derive(Debug)]
pub struct UseGuard {
    key: String,
    uses: Arc<Mutex<HashMap<String, usize>>>,
}

impl Drop for UseGuard {
    fn drop(&mut self) {
        let mut u = self.uses.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(n) = u.get_mut(&self.key) {
            *n -= 1;
            if *n == 0 {
                u.remove(&self.key);
            }
        }
    }
}

/// Réponse à une demande de fichier converti.
#[derive(Debug)]
pub enum Outcome {
    /// Conversion disponible, déjà ouverte (une éviction ne peut plus la faire disparaître en cours
    /// d'envoi) : taille, identifiant stable (pour l'ETag), format, et la garde à garder pendant l'envoi.
    Ready { file: std::fs::File, len: u64, key: String, format: &'static str, guard: UseGuard },
    /// En cours ou en attente.
    Pending(ConvertProgress),
    /// Échec récent (message pour l'utilisateur) ; rendu tant que dure le délai d'attente de `failure_backoff`.
    Failed(String),
    /// Pas de clés configurées.
    Unavailable,
}

#[derive(Debug, Clone)]
enum State {
    Queued,
    Running,
    Failed(String),
}

#[derive(Debug)]
struct Job {
    state: State,
    /// Début de la tâche ; pour un échec : instant de l'échec (départ du délai d'attente).
    since: Instant,
    source_bytes: u64,
    percent: Arc<AtomicU64>,
}

const NO_PERCENT: u64 = u64::MAX;

#[derive(Debug)]
pub struct Converter {
    cfg: ConvertConfig,
    jobs: Mutex<HashMap<String, Job>>,
    uses: Arc<Mutex<HashMap<String, usize>>>,
    one_at_a_time: Arc<tokio::sync::Semaphore>,
    /// Compteur incrémenté à chaque changement d'une tâche (création, démarrage, pourcentage,
    /// fin, échec) : les requêtes longues (`?attendre=1`) dorment dessus au lieu d'être relancées.
    changes: tokio::sync::watch::Sender<u64>,
}

impl Converter {
    /// Prépare le cache : crée les dossiers et vide `work/` (conversions interrompues par un arrêt).
    pub fn new(cfg: ConvertConfig) -> std::io::Result<Arc<Self>> {
        for d in ["ready", "home", "tmp"] {
            std::fs::create_dir_all(cfg.cache.join(d))?;
        }
        let work = cfg.cache.join("work");
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work)?;
        Ok(Arc::new(Self { cfg, jobs: Mutex::new(HashMap::new()), uses: Arc::default(), one_at_a_time: Arc::new(tokio::sync::Semaphore::new(1)), changes: tokio::sync::watch::channel(0).0 }))
    }

    /// Clés présentes ? Le fichier est testé à chaque demande : un secret monté en optionnel peut
    /// apparaître (ou manquer) après le démarrage.
    pub fn available(&self) -> bool {
        self.cfg.keys.as_ref().is_some_and(|k| k.is_file())
    }

    fn ready_dir(&self, key: &str) -> PathBuf {
        self.cfg.cache.join("ready").join(key)
    }

    fn work_dir(&self, key: &str) -> PathBuf {
        self.cfg.cache.join("work").join(key)
    }

    /// Signale un changement de l'état des conversions.
    fn bump(&self) {
        self.changes.send_modify(|n| *n = n.wrapping_add(1));
    }

    /// Durée maximale d'une requête longue.
    pub fn long_poll(&self) -> Duration {
        self.cfg.long_poll
    }

    /// Abonnement aux changements (requête longue).
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.changes.subscribe()
    }

    fn jobs(&self) -> std::sync::MutexGuard<'_, HashMap<String, Job>> {
        self.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Identifiant d'une conversion : change si le fichier source change (taille, date) ou de format.
    fn key_of(src: &Path, len: u64, mtime: u64, target: &str) -> String {
        let mut h = Sha256::new();
        h.update(src.as_os_str().as_encoded_bytes());
        h.update(format!("\0{len}\0{mtime}\0{target}"));
        hex::encode(&h.finalize()[..16])
    }

    /// Conversion prête, ouverte, s'il y en a une ; son accès est noté (LRU) et son utilisation
    /// enregistrée (l'éviction l'épargne) avant l'ouverture. E/S bloquantes : à appeler hors du runtime.
    fn ready(&self, key: &str, target: &'static str) -> Option<Outcome> {
        let path = self.ready_dir(key).join(format!("file.{target}"));
        let guard = {
            let mut u = self.uses.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            *u.entry(key.to_string()).or_insert(0) += 1;
            UseGuard { key: key.to_string(), uses: self.uses.clone() }
        };
        let f = std::fs::OpenOptions::new().read(true).write(true).open(&path).ok()?;
        let len = f.metadata().ok()?.len();
        let _ = f.set_modified(std::time::SystemTime::now());
        Some(Outcome::Ready { file: f, len, key: key.to_string(), format: target, guard })
    }

    /// Demande le fichier `src` (compressé, `source_len` octets, modifié à `mtime`) converti en
    /// `target` : prêt, ou lance la conversion (une seule, quel que soit le nombre de demandes).
    /// Aucune E/S disque n'est faite sous le verrou ni sur le runtime.
    pub async fn request(self: &Arc<Self>, src: &Path, source_len: u64, mtime: u64, target: &'static str) -> Outcome {
        if !self.available() {
            return Outcome::Unavailable;
        }
        let key = Self::key_of(src, source_len, mtime, target);
        // 1. Tâche connue ? (verrou court, mémoire seulement)
        if let Some(out) = self.known_job(&key) {
            return match out {
                Err(msg) => Outcome::Failed(msg),
                Ok(snap) => Outcome::Pending(self.progress_of(&key, snap).await),
            };
        }
        // 2. Déjà converti ?
        let me = self.clone();
        let k = key.clone();
        if let Ok(Some(r)) = tokio::task::spawn_blocking(move || me.ready(&k, target)).await {
            return r;
        }
        // 3. Nouvelle tâche (une autre demande a pu la créer entre-temps).
        let percent = Arc::new(AtomicU64::new(NO_PERCENT));
        let snap = {
            let mut jobs = self.jobs();
            if let Some(job) = jobs.get(&key) {
                if let State::Failed(msg) = &job.state {
                    return Outcome::Failed(msg.clone());
                }
                Snapshot::of(job)
            } else {
                let job = Job { state: State::Queued, since: Instant::now(), source_bytes: source_len, percent: percent.clone() };
                let snap = Snapshot::of(&job);
                jobs.insert(key.clone(), job);
        drop(jobs);
        self.bump();
                let me = self.clone();
                let (key, src) = (key.clone(), src.to_path_buf());
                tokio::spawn(async move { me.run(key, src, source_len, target, percent).await });
                snap
            }
        };
        Outcome::Pending(self.progress_of(&key, snap).await)
    }

    /// État d'une tâche connue : `Err(message)` pour un échec encore dans son délai d'attente (les
    /// échecs plus anciens, et ceux que personne n'a relus, sont purgés ici), `Ok` sinon.
    fn known_job(&self, key: &str) -> Option<Result<Snapshot, String>> {
        let backoff = self.cfg.failure_backoff;
        let mut jobs = self.jobs();
        jobs.retain(|_, j| !matches!(j.state, State::Failed(_)) || j.since.elapsed() < backoff);
        let job = jobs.get(key)?;
        Some(match &job.state {
            State::Failed(msg) => Err(msg.clone()),
            _ => Ok(Snapshot::of(job)),
        })
    }

    async fn progress_of(self: &Arc<Self>, key: &str, snap: Snapshot) -> ConvertProgress {
        let dir = self.work_dir(key).join("out");
        let written = tokio::task::spawn_blocking(move || dir_bytes(&dir)).await.unwrap_or(0);
        let p = snap.percent;
        ConvertProgress {
            state: if snap.queued { "queued" } else { "converting" }.into(),
            elapsed_s: snap.elapsed_s,
            written_bytes: written,
            source_bytes: snap.source_bytes,
            percent: (p != NO_PERCENT).then_some(p.min(100) as u8),
        }
    }

    fn set_state(&self, key: &str, state: State) {
        if let Some(j) = self.jobs().get_mut(key) {
            if matches!(state, State::Failed(_)) {
                j.since = Instant::now();
            }
            j.state = state;
        }
        self.bump();
    }

    async fn run(self: Arc<Self>, key: String, src: PathBuf, source_len: u64, target: &'static str, percent: Arc<AtomicU64>) {
        let _permit = self.one_at_a_time.clone().acquire_owned().await.ok();
        // Une demande a pu arriver entre « pas prêt » et la création de la tâche : déjà converti ?
        let (me, k) = (self.clone(), key.clone());
        if tokio::task::spawn_blocking(move || me.ready_dir(&k).join(format!("file.{target}")).is_file()).await.unwrap_or(false) {
            self.jobs().remove(&key);
            return;
        }
        self.set_state(&key, State::Running);
        match self.convert(&key, &src, source_len, target, &percent).await {
            Ok(()) => {
                // Terminé : la table des tâches n'a plus rien à dire, `ready/` fait foi.
                self.jobs().remove(&key);
                self.bump();
            }
            Err(msg) => {
                tracing::error!(erreur = %msg, fichier = %src.display(), "conversion échouée");
                let _ = tokio::fs::remove_dir_all(self.work_dir(&key)).await;
                self.set_state(&key, State::Failed("Conversion échouée sur le serveur ; nouvel essai possible dans quelques minutes".into()));
            }
        }
    }

    async fn convert(&self, key: &str, src: &Path, source_len: u64, target: &'static str, percent: &Arc<AtomicU64>) -> Result<(), String> {
        let work = self.work_dir(key);
        let out = work.join("out");
        let cfg = &self.cfg;
        let prep = {
            let (cache, work, out) = (cfg.cache.clone(), work.clone(), out.clone());
            let (max, need) = (cfg.max_bytes, source_len.saturating_mul(2));
            let (keep, uses) = (key.to_string(), self.uses.clone());
            tokio::task::spawn_blocking(move || {
                // Place pour la nouvelle conversion avant de commencer (estimation : 2 × la source).
                evict(&cache.join("ready"), max, need, &|k| k == keep || in_use(&uses, k));
                let _ = std::fs::remove_dir_all(&work);
                std::fs::create_dir_all(&out)
            })
            .await
        };
        prep.map_err(|e| e.to_string())?.map_err(|e| format!("dossier de travail : {e}"))?;
        let keys = cfg.keys.as_ref().ok_or("clés absentes")?;
        let mut cmd = tokio::process::Command::new(&cfg.command);
        cmd.args(["-D", "--minimal-output", "--keys"])
            .arg(keys)
            .arg("-o")
            .arg(&out)
            .arg(src)
            .env("HOME", cfg.cache.join("home"))
            .env("TMPDIR", cfg.cache.join("tmp"))
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            // Groupe de processus propre : nsz lance des processus fils (multiprocessing) ; on tue le groupe entier.
            .process_group(0)
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| format!("lancement de {} : {e}", cfg.command))?;
        let group = child.id().map(GroupKill);
        let stdout = child.stdout.take().ok_or("sortie standard absente")?;
        let stderr = child.stderr.take().ok_or("sortie d'erreur absente")?;
        // Les deux sorties sont lues en continu jusqu'à la fin (un tube plein bloquerait le convertisseur).
        let pct = percent.clone();
        let out_task = tokio::spawn(drain(stdout, Some((pct, self.changes.clone()))));
        let err_task = tokio::spawn(drain(stderr, None));
        let waited = tokio::time::timeout(cfg.timeout, child.wait()).await;
        let status = match waited {
            Ok(r) => r,
            Err(_) => {
                if let Some(g) = &group {
                    g.kill();
                }
                let _ = child.wait().await;
                out_task.abort();
                err_task.abort();
                return Err(format!("délai dépassé ({} s)", cfg.timeout.as_secs()));
            }
        };
        // Fin normale ou non : plus aucun descendant ne doit survivre (nsz peut laisser des fils).
        if let Some(g) = &group {
            g.kill();
        }
        let tail = match tokio::time::timeout(Duration::from_secs(5), async { (out_task.await, err_task.await) }).await {
            Ok((_, Ok(e))) => e,
            _ => String::new(),
        };
        let status = status.map_err(|e| format!("attente du convertisseur : {e}"))?;
        if !status.success() {
            return Err(format!("{} a échoué ({status}) : {}", cfg.command, tail.trim()));
        }
        let (work2, ready, key2, max) = (work.clone(), self.ready_dir(key), key.to_string(), cfg.max_bytes);
        let ready_root = cfg.cache.join("ready");
        let uses = self.uses.clone();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let produced = find_output(&out, target).ok_or_else(|| format!("aucun .{target} produit"))?;
            std::fs::create_dir_all(&ready).map_err(|e| e.to_string())?;
            let dest = ready.join(format!("file.{target}"));
            std::fs::rename(&produced, &dest).map_err(|e| format!("mise en cache : {e}"))?;
            let _ = std::fs::remove_dir_all(&work2);
            evict(&ready_root, max, 0, &|k| k == key2 || in_use(&uses, k));
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

/// Copie des champs d'une tâche utiles à la réponse (pour ne pas tenir le verrou pendant les E/S).
#[derive(Debug, Clone, Copy)]
struct Snapshot {
    queued: bool,
    elapsed_s: u64,
    source_bytes: u64,
    percent: u64,
}

impl Snapshot {
    fn of(j: &Job) -> Self {
        Self { queued: matches!(j.state, State::Queued), elapsed_s: j.since.elapsed().as_secs(), source_bytes: j.source_bytes, percent: j.percent.load(Ordering::Relaxed) }
    }
}

/// Tue tout le groupe de processus du convertisseur (SIGKILL) ; sans effet si le groupe a disparu.
struct GroupKill(u32);

impl GroupKill {
    fn kill(&self) {
        // SAFETY : appel système sans pointeur ; le pid est celui du processus que nous avons lancé
        // comme chef de son propre groupe (`process_group(0)`), jamais le nôtre.
        unsafe {
            libc::killpg(self.0 as libc::pid_t, libc::SIGKILL);
        }
    }
}

impl Drop for GroupKill {
    fn drop(&mut self) {
        // Arrêt du serveur ou tâche abandonnée : pas de convertisseur orphelin.
        self.kill();
    }
}

/// Lit un tube jusqu'à sa fin. Découpe sur `\r` et `\n` (nsz redessine sa barre avec `\r`), tampon borné ;
/// publie le dernier pourcentage `N%` vu (si `percent`). Renvoie la FIN du flux (2 Kio) pour le journal.
async fn drain(mut r: impl tokio::io::AsyncRead + Unpin, percent: Option<(Arc<AtomicU64>, tokio::sync::watch::Sender<u64>)>) -> String {
    const TAIL: usize = 2048;
    const MAX_SEGMENT: usize = 4096;
    let mut tail: VecDeque<u8> = VecDeque::with_capacity(TAIL);
    let mut seg: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = match r.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        for &b in &buf[..n] {
            if tail.len() == TAIL {
                tail.pop_front();
            }
            tail.push_back(b);
            if b == b'\r' || b == b'\n' {
                if let (Some((p, tx)), Some(v)) = (&percent, parse_percent(&seg)) {
                    if p.swap(v, Ordering::Relaxed) != v {
                        tx.send_modify(|n| *n = n.wrapping_add(1));
                    }
                }
                seg.clear();
            } else if seg.len() < MAX_SEGMENT {
                seg.push(b);
            }
        }
    }
    // Dernier segment sans fin de ligne.
    if let (Some((p, tx)), Some(v)) = (&percent, parse_percent(&seg)) {
        if p.swap(v, Ordering::Relaxed) != v {
            tx.send_modify(|n| *n = n.wrapping_add(1));
        }
    }
    let (a, b) = tail.as_slices();
    String::from_utf8_lossy(&[a, b].concat()).into_owned()
}

/// Pourcentage d'un segment de la sortie de nsz (`| 42% Decompress`, `100% Done`) : les chiffres
/// qui précèdent le premier `%`, après les éventuels caractères de toupie (`| / - \`).
pub fn parse_percent(seg: &[u8]) -> Option<u64> {
    let s = std::str::from_utf8(seg).ok()?;
    let (before, _) = s.split_once('%')?;
    let digits: String = before.trim_end().chars().rev().take_while(char::is_ascii_digit).collect::<Vec<_>>().into_iter().rev().collect();
    digits.parse::<u64>().ok().filter(|v| *v <= 100)
}

fn in_use(uses: &Mutex<HashMap<String, usize>>, key: &str) -> bool {
    uses.lock().unwrap_or_else(std::sync::PoisonError::into_inner).contains_key(key)
}

/// Premier fichier `*.<ext>` d'un dossier.
fn find_output(dir: &Path, ext: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.filter_map(Result::ok).map(|e| e.path()).find(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)))
}

/// Somme des tailles des fichiers d'un dossier (un niveau).
fn dir_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir).map(|rd| rd.filter_map(Result::ok).filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum()).unwrap_or(0)
}

/// Supprime les conversions les moins récemment servies tant que `total + needed > max`. Les
/// conversions pour lesquelles `keep` renvoie vrai (en cours d'envoi, ou celle qu'on vient de produire)
/// ne sont jamais supprimées.
pub fn evict(ready_root: &Path, max: u64, needed: u64, keep: &dyn Fn(&str) -> bool) {
    let Ok(rd) = std::fs::read_dir(ready_root) else { return };
    let mut entries: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
    for dir in rd.filter_map(Result::ok) {
        let p = dir.path();
        let Ok(files) = std::fs::read_dir(&p) else { continue };
        let (mut len, mut newest) = (0, std::time::UNIX_EPOCH);
        for f in files.filter_map(Result::ok).filter_map(|f| f.metadata().ok()) {
            len += f.len();
            newest = newest.max(f.modified().unwrap_or(std::time::UNIX_EPOCH));
        }
        entries.push((newest, len, p));
    }
    let mut total: u64 = entries.iter().map(|e| e.1).sum();
    entries.sort_by_key(|e| e.0);
    for (_, len, p) in entries {
        if total.saturating_add(needed) <= max {
            break;
        }
        if p.file_name().and_then(|n| n.to_str()).is_some_and(keep) {
            continue;
        }
        if std::fs::remove_dir_all(&p).is_ok() {
            tracing::info!(dossier = %p.display(), "conversion évincée du cache");
            total = total.saturating_sub(len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pourcentage_de_la_sortie_reelle_de_nsz() {
        // Forme mesurée : séparateurs « \r », un seul « \n » final, toupie devant le pourcentage.
        let reelle = "| 0% Decompress\r/ 1% Decompress\r- 2% Decompress\r\\ 3% Decompress\r| 99% Decompress\r100% Decompress\r100% Done\n";
        let vus: Vec<u64> = reelle.split(['\r', '\n']).filter_map(|s| parse_percent(s.as_bytes())).collect();
        assert_eq!(vus, [0, 1, 2, 3, 99, 100, 100]);
        assert_eq!(parse_percent(b"pas de pourcentage"), None);
        assert_eq!(parse_percent(b"150% trop"), None);
    }

    #[tokio::test]
    async fn drain_decoupe_sur_cr_et_borne_le_tampon() {
        let p = Arc::new(AtomicU64::new(NO_PERCENT));
        let mut data = b"| 0% Decompress\r/ 41% Decompress\r".to_vec();
        data.extend(std::iter::repeat_n(b'x', 1 << 20)); // 1 Mio sans fin de ligne
        data.extend(b"\r- 77% Decompress");
        let (tx, rx) = tokio::sync::watch::channel(0u64);
        let tail = drain(std::io::Cursor::new(data), Some((p.clone(), tx))).await;
        assert_eq!(p.load(Ordering::Relaxed), 77);
        // Chaque pourcentage NOUVEAU publie un changement (0, 41, 77) : réveil des requêtes longues.
        assert_eq!(*rx.borrow(), 3);
        assert!(tail.len() <= 2048 && tail.ends_with("77% Decompress"));
    }
}
