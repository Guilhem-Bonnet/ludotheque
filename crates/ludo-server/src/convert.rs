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
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

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
}

impl ConvertConfig {
    pub const DEFAULT_MAX_BYTES: u64 = 80 * 1_000_000_000;

    pub fn new(cache: PathBuf) -> Self {
        Self { cache, max_bytes: Self::DEFAULT_MAX_BYTES, keys: None, command: "nsz".into(), timeout: Duration::from_secs(3 * 3600) }
    }
}

/// Réponse à une demande de fichier converti.
#[derive(Debug)]
pub enum Outcome {
    /// Conversion disponible : chemin, taille, identifiant stable (pour l'ETag), format.
    Ready { path: PathBuf, len: u64, key: String, format: &'static str },
    /// En cours ou en attente.
    Pending(ConvertProgress),
    /// Échec (message pour l'utilisateur) ; rendu une fois, la demande suivante relance.
    Failed(String),
    /// Pas de clés configurées.
    Unavailable,
}

#[derive(Debug)]
enum State {
    Queued,
    Running,
    Failed(String),
}

#[derive(Debug)]
struct Job {
    state: State,
    since: Instant,
    source_bytes: u64,
    percent: Arc<AtomicU64>,
}

const NO_PERCENT: u64 = u64::MAX;

#[derive(Debug)]
pub struct Converter {
    cfg: ConvertConfig,
    jobs: Mutex<HashMap<String, Job>>,
    one_at_a_time: Arc<tokio::sync::Semaphore>,
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
        Ok(Arc::new(Self { cfg, jobs: Mutex::new(HashMap::new()), one_at_a_time: Arc::new(tokio::sync::Semaphore::new(1)) }))
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

    /// Identifiant d'une conversion : change si le fichier source change (taille, date) ou de format.
    fn key_of(src: &Path, len: u64, mtime: u64, target: &str) -> String {
        let mut h = Sha256::new();
        h.update(src.as_os_str().as_encoded_bytes());
        h.update(format!("\0{len}\0{mtime}\0{target}"));
        hex::encode(&h.finalize()[..16])
    }

    /// Conversion prête, s'il y en a une ; son accès est noté (LRU).
    fn ready(&self, key: &str, target: &'static str) -> Option<Outcome> {
        let path = self.ready_dir(key).join(format!("file.{target}"));
        let f = std::fs::OpenOptions::new().write(true).open(&path).ok()?;
        let len = f.metadata().ok()?.len();
        let _ = f.set_modified(std::time::SystemTime::now());
        Some(Outcome::Ready { path, len, key: key.to_string(), format: target })
    }

    /// Demande le fichier `src` (compressé, `source_len` octets, modifié à `mtime`) converti en
    /// `target` : prêt, ou lance la conversion (une seule, quel que soit le nombre de demandes).
    /// À appeler depuis le runtime tokio.
    pub fn request(self: &Arc<Self>, src: &Path, source_len: u64, mtime: u64, target: &'static str) -> Outcome {
        if !self.available() {
            return Outcome::Unavailable;
        }
        let key = Self::key_of(src, source_len, mtime, target);
        let mut jobs = self.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(job) = jobs.get(&key) {
            if let State::Failed(msg) = &job.state {
                let msg = msg.clone();
                jobs.remove(&key);
                return Outcome::Failed(msg);
            }
            let p = self.progress_of(&key, job);
            return Outcome::Pending(p);
        }
        if let Some(r) = self.ready(&key, target) {
            return r;
        }
        let percent = Arc::new(AtomicU64::new(NO_PERCENT));
        let job = Job { state: State::Queued, since: Instant::now(), source_bytes: source_len, percent: percent.clone() };
        let first = self.progress_of(&key, &job);
        jobs.insert(key.clone(), job);
        drop(jobs);
        let me = self.clone();
        let src = src.to_path_buf();
        tokio::spawn(async move { me.run(key, src, source_len, target, percent).await });
        Outcome::Pending(first)
    }

    fn progress_of(&self, key: &str, job: &Job) -> ConvertProgress {
        let p = job.percent.load(Ordering::Relaxed);
        ConvertProgress {
            state: if matches!(job.state, State::Queued) { "queued" } else { "converting" }.into(),
            elapsed_s: job.since.elapsed().as_secs(),
            written_bytes: dir_bytes(&self.work_dir(key).join("out")),
            source_bytes: job.source_bytes,
            percent: (p != NO_PERCENT).then_some(p.min(100) as u8),
        }
    }

    fn set_state(&self, key: &str, state: State) {
        if let Some(j) = self.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get_mut(key) {
            j.state = state;
        }
    }

    async fn run(self: Arc<Self>, key: String, src: PathBuf, source_len: u64, target: &'static str, percent: Arc<AtomicU64>) {
        let _permit = self.one_at_a_time.clone().acquire_owned().await.ok();
        self.set_state(&key, State::Running);
        let me = self.clone();
        let res = self.convert(&key, &src, source_len, target, &percent).await;
        match res {
            Ok(()) => {
                // Terminé : la table des tâches n'a plus rien à dire, `ready/` fait foi.
                me.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&key);
            }
            Err(msg) => {
                tracing::error!(erreur = %msg, fichier = %src.display(), "conversion échouée");
                let _ = tokio::fs::remove_dir_all(self.work_dir(&key)).await;
                me.set_state(&key, State::Failed("Conversion échouée sur le serveur ; nouvel essai possible".into()));
            }
        }
    }

    async fn convert(&self, key: &str, src: &Path, source_len: u64, target: &'static str, percent: &AtomicU64) -> Result<(), String> {
        let work = self.work_dir(key);
        let out = work.join("out");
        let cfg = &self.cfg;
        let prep = {
            let (cache, work, out) = (cfg.cache.clone(), work.clone(), out.clone());
            let (max, need) = (cfg.max_bytes, source_len.saturating_mul(2));
            let keep = key.to_string();
            tokio::task::spawn_blocking(move || {
                // Place pour la nouvelle conversion avant de commencer (estimation : 2 × la source).
                evict(&cache.join("ready"), max, need, &keep);
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
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| format!("lancement de {} : {e}", cfg.command))?;
        let stdout = child.stdout.take().ok_or("sortie standard absente")?;
        let mut stderr = child.stderr.take().ok_or("sortie d'erreur absente")?;
        // Pourcentage : lignes « 42% étape » (séparées par \n ou \r).
        let pct = async {
            let mut r = BufReader::new(stdout);
            let mut line = Vec::new();
            loop {
                line.clear();
                match r.read_until(b'\n', &mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        for part in String::from_utf8_lossy(&line).split(['\r', '\n']) {
                            if let Some(n) = part.trim().split('%').next().and_then(|n| n.trim().parse::<u64>().ok()).filter(|_| part.contains('%')) {
                                percent.store(n.min(100), Ordering::Relaxed);
                            }
                        }
                    }
                }
            }
        };
        let err_tail = async {
            let mut buf = Vec::new();
            let _ = (&mut stderr).take(64 * 1024).read_to_end(&mut buf).await;
            String::from_utf8_lossy(&buf).chars().rev().take(500).collect::<Vec<_>>().into_iter().rev().collect::<String>()
        };
        let waited = tokio::time::timeout(cfg.timeout, async {
            let (status, _, tail) = tokio::join!(child.wait(), pct, err_tail);
            (status, tail)
        })
        .await;
        let (status, tail) = match waited {
            Ok(v) => v,
            Err(_) => {
                let _ = child.kill().await;
                return Err(format!("délai dépassé ({} s)", cfg.timeout.as_secs()));
            }
        };
        let status = status.map_err(|e| format!("attente du convertisseur : {e}"))?;
        if !status.success() {
            return Err(format!("{} a échoué ({status}) : {}", cfg.command, tail.trim()));
        }
        let (work2, ready, key2, max) = (work.clone(), self.ready_dir(key), key.to_string(), cfg.max_bytes);
        let ready_root = cfg.cache.join("ready");
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let produced = find_output(&out, target).ok_or_else(|| format!("aucun .{target} produit"))?;
            std::fs::create_dir_all(&ready).map_err(|e| e.to_string())?;
            let dest = ready.join(format!("file.{target}"));
            std::fs::rename(&produced, &dest).map_err(|e| format!("mise en cache : {e}"))?;
            let _ = std::fs::remove_dir_all(&work2);
            evict(&ready_root, max, 0, &key2);
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

/// Premier fichier `*.<ext>` d'un dossier.
fn find_output(dir: &Path, ext: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.filter_map(Result::ok).map(|e| e.path()).find(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)))
}

/// Somme des tailles des fichiers d'un dossier (un niveau).
fn dir_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir).map(|rd| rd.filter_map(Result::ok).filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum()).unwrap_or(0)
}

/// Supprime les conversions les moins récemment servies tant que `total + needed > max`. La
/// conversion `keep` n'est jamais supprimée.
pub fn evict(ready_root: &Path, max: u64, needed: u64, keep: &str) {
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
        if p.file_name().is_some_and(|n| n == keep) {
            continue;
        }
        if std::fs::remove_dir_all(&p).is_ok() {
            tracing::info!(dossier = %p.display(), "conversion évincée du cache");
            total = total.saturating_sub(len);
        }
    }
}
