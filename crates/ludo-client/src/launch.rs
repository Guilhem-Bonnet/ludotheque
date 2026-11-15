//! Lancement d'un jeu depuis l'interface : même déroulé que `ludo run` (verrou de partie,
//! synchronisation avant, émulateur, synchronisation après avec le temps de jeu, libération du
//! verrou), plus l'installation du jeu s'il manque et des étapes que l'interface peut afficher.
//!
//! Aucune sauvegarde n'est écrite en dehors du moteur de synchronisation (`Engine::sync` et
//! `Engine::resolve` : compare-and-swap, versions écartées archivées). Le journal n'est verrouillé
//! que le temps de chaque synchronisation, jamais pendant la partie.

use crate::api::{ApiError, Client};
use crate::config::{self, Config};
use crate::engine::{Engine, Resolution, SyncOutcome};
use crate::journal::Journal;
use ludo_proto::{CatalogGame, FileRole, LeaseOutcome, SaveKey, SaveRevision};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Étapes d'un lancement, dans l'ordre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Profil,
    Telechargement,
    Sauvegarde,
    Lancement,
    EnJeu,
    Envoi,
    Termine,
}

impl Step {
    /// Identifiant échangé avec l'interface.
    pub fn id(self) -> &'static str {
        match self {
            Step::Profil => "profil",
            Step::Telechargement => "telechargement",
            Step::Sauvegarde => "sauvegarde",
            Step::Lancement => "lancement",
            Step::EnJeu => "en_jeu",
            Step::Envoi => "envoi",
            Step::Termine => "termine",
        }
    }
}

/// État d'une étape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepState {
    EnCours,
    Fait,
}

impl StepState {
    pub fn id(self) -> &'static str {
        match self {
            StepState::EnCours => "en_cours",
            StepState::Fait => "fait",
        }
    }
}

/// Ce que l'appelant fournit : affichage des étapes et installation du jeu.
pub trait Observer {
    fn step(&mut self, step: Step, state: StepState, detail: Option<String>);
    /// Installe le jeu (appelé seulement s'il manque sur l'appareil). Erreur : message en français.
    fn install(&mut self, game: &CatalogGame) -> Result<(), String>;
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LaunchError {
    /// Une partie est déjà en cours sur cet appareil (identifiant de l'appareil qui tient le verrou).
    #[error("En cours sur {0}")]
    HeldElsewhere(String),
    #[error("{0}")]
    Failed(String),
    /// La partie est jouée mais l'envoi de la sauvegarde a échoué : il se réessaie sans relancer le
    /// jeu (`send`), avec ce temps de jeu cumulé.
    #[error("{message}")]
    SendFailed { message: String, playtime_s: u64, exit_ok: bool },
}

fn fail(e: impl std::fmt::Display) -> LaunchError {
    LaunchError::Failed(e.to_string())
}

/// Un conflit de sauvegarde à trancher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictInfo {
    /// Tête actuelle du serveur.
    pub server: SaveRevision,
    /// Dernière version commune (base de la copie locale), si elle est connue du serveur.
    pub base: Option<SaveRevision>,
    /// Le conflit est apparu à l'envoi, après une partie jouée (le jeu ne doit pas être relancé).
    pub played: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchOutcome {
    /// Partie jouée et sauvegarde synchronisée (ou en attente d'envoi si hors ligne).
    Played {
        before: SyncOutcome,
        after: SyncOutcome,
        /// Temps de jeu cumulé envoyé avec la sauvegarde.
        playtime_s: u64,
        elapsed_s: u64,
        /// L'émulateur s'est terminé sans erreur.
        exit_ok: bool,
    },
    /// Rien n'a été lancé (ou la partie est jouée mais l'envoi est en conflit) : le joueur tranche.
    Conflict(ConflictInfo),
}

/// Contexte de l'appareil.
pub struct Env<'a> {
    pub cfg: &'a Config,
    pub journal_path: &'a Path,
    /// Verrou du journal du processus (une synchronisation à la fois).
    pub journal_lock: &'a Mutex<()>,
}

impl Env<'_> {
    fn locked<T>(&self, f: impl FnOnce(&mut Journal) -> Result<T, LaunchError>) -> Result<T, LaunchError> {
        let _guard = self.journal_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut journal = Journal::open(self.journal_path).map_err(fail)?;
        f(&mut journal)
    }
}

/// Verrou de partie rendu à la destruction.
struct Lease<'a> {
    client: &'a Client,
    key: &'a SaveKey,
    device: String,
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        let _ = self.client.release_lease(self.key, &self.device);
    }
}

/// Prend le verrou. Serveur injoignable : `None`, on continue hors ligne.
fn take_lease<'a>(client: &'a Client, key: &'a SaveKey, device: &str) -> Result<Option<Lease<'a>>, LaunchError> {
    match client.acquire_lease(key, device, false) {
        Ok(LeaseOutcome::HeldElsewhere { lease }) => Err(LaunchError::HeldElsewhere(lease.device)),
        Ok(LeaseOutcome::Granted { .. }) => Ok(Some(Lease { client, key, device: device.into() })),
        Err(_) => Ok(None),
    }
}

/// Pose `stop` à la destruction : le fil de renouvellement s'arrête même si la partie panique.
pub struct StopOnDrop<'a>(pub &'a AtomicBool);

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Identifiant sous lequel l'app prend le verrou d'une partie : distinct de celui de `ludo run`
/// (`<appareil>`) et de celui des opérations de l'interface (`<appareil>-ui`), pour que deux
/// lancements sur le même appareil s'excluent.
pub fn app_device(appareil: &str) -> String {
    format!("{appareil}-app")
}

/// Appareil lisible d'un identifiant de verrou (sans suffixe `-app` ni `-ui`).
pub fn holder_device(id: &str) -> &str {
    id.strip_suffix("-app").or_else(|| id.strip_suffix("-ui")).unwrap_or(id)
}

/// Le verrou expire au bout de 120 s : il est renouvelé tant que la partie dure.
const HEARTBEAT: Duration = Duration::from_secs(40);

pub fn heartbeat(client: &Client, key: &SaveKey, device: &str, stop: &AtomicBool) {
    let mut last = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(250));
        if last.elapsed() >= HEARTBEAT {
            let _ = client.acquire_lease(key, device, false);
            last = Instant::now();
        }
    }
}

/// Temps de jeu cumulé de la tête du serveur : `Some(0)` s'il n'y a pas encore de tête, `None` si
/// le serveur est injoignable (valeur inconnue, à ne jamais confondre avec 0).
pub fn head_playtime(client: &Client, key: &SaveKey) -> Option<u64> {
    client.head(key).ok().map(|h| h.map_or(0, |h| h.playtime_s))
}

/// Temps de jeu à envoyer après une partie : le plus grand cumul connu (avant la partie, ou relu
/// juste avant l'envoi) plus la durée jouée. Jamais moins que la tête connue ; cumul inconnu des
/// deux côtés (hors ligne) : la durée seule, l'envoi restant en attente.
pub fn playtime_after(before: Option<u64>, head_now: Option<u64>, elapsed_s: u64) -> u64 {
    before.max(head_now).unwrap_or(0).saturating_add(elapsed_s)
}

/// Retrouve la ROM de base d'un jeu sans le catalogue (serveur injoignable) : fichier `nsp`/`xci`
/// dont le nom contient `[<identifiant du titre>]`, hors dossier des mises à jour et DLC.
pub fn find_rom_offline(roms: &Path, title_id: &str) -> Option<PathBuf> {
    let tag = format!("[{}]", title_id.to_lowercase());
    let mut found: Vec<(u8, String, PathBuf)> = Vec::new();
    for console in std::fs::read_dir(roms).ok()?.flatten() {
        let Ok(files) = std::fs::read_dir(console.path()) else { continue };
        for f in files.flatten() {
            let p = f.path();
            let name = f.file_name().to_string_lossy().to_lowercase();
            // Les mises à jour et DLC vivent dans un sous-dossier (`MAJ-DLC`), jamais des fichiers directs.
            let rank = if name.ends_with(".nsp") { 0 } else if name.ends_with(".xci") { 1 } else { continue };
            if p.is_file() && name.contains(&tag) {
                found.push((rank, p.to_string_lossy().into_owned(), p));
            }
        }
    }
    // Choix déterministe : .nsp avant .xci, puis ordre des noms.
    found.sort();
    found.into_iter().next().map(|(_, _, p)| p)
}

pub fn conflict_info(env: &Env, client: &Client, key: &SaveKey, server: SaveRevision, played: bool) -> ConflictInfo {
    let base_rev = env.locked(|j| Ok(j.get(key).base_rev)).unwrap_or(0);
    let base = (base_rev > 0).then(|| client.history(key).ok().and_then(|h| h.into_iter().find(|r| r.rev == base_rev))).flatten();
    ConflictInfo { server, base, played }
}

fn outcome_detail(o: &SyncOutcome) -> Option<String> {
    match o {
        SyncOutcome::Offline { pending_upload: true } => Some("Hors ligne : envoi en attente".into()),
        SyncOutcome::Offline { pending_upload: false } => Some("Hors ligne : sauvegarde locale utilisée".into()),
        SyncOutcome::Downloaded { .. } => Some("Sauvegarde récupérée".into()),
        SyncOutcome::Uploaded { .. } => Some("Sauvegarde envoyée".into()),
        SyncOutcome::LocalMissing => Some("Dossier de sauvegarde introuvable".into()),
        _ => None,
    }
}

/// Lance un jeu : verrou, installation si besoin, synchronisation avant, émulateur, attente de sa
/// fermeture, synchronisation après (temps de jeu = tête du serveur + durée de la partie).
///
/// Conflit avant la partie : rien n'est lancé, le verrou est rendu, la sauvegarde n'est pas touchée.
/// Émulateur introuvable ou en échec au démarrage : erreur, verrou rendu.
pub fn launch(env: &Env, key: &SaveKey, obs: &mut dyn Observer) -> Result<LaunchOutcome, LaunchError> {
    let cfg = env.cfg;
    obs.step(Step::Profil, StepState::EnCours, None);
    let conf = cfg
        .jeux
        .get(&key.game)
        .filter(|g| g.concerns(&key.profile))
        .ok_or_else(|| fail("Jeu non configuré sur cet appareil pour ce profil"))?;
    // Adaptateur impossible à construire : erreur avant de toucher au serveur.
    let prepared = config::prepare(cfg, &conf.emulateur, &key.profile, &conf.title_id).map_err(fail)?;
    let client = Client::new(&cfg.serveur);
    let device = app_device(&cfg.appareil);
    let lease = take_lease(&client, key, &device)?;
    obs.step(Step::Profil, StepState::Fait, None);

    let stop = AtomicBool::new(false);
    let result = std::thread::scope(|s| {
        let _stop = StopOnDrop(&stop);
        if lease.is_some() {
            s.spawn(|| heartbeat(&client, key, &device, &stop));
        }
        play(env, &client, key, conf, &prepared, obs)
    });
    drop(lease);
    result
}

fn play(
    env: &Env,
    client: &Client,
    key: &SaveKey,
    conf: &config::GameConf,
    prepared: &config::Prepared,
    obs: &mut dyn Observer,
) -> Result<LaunchOutcome, LaunchError> {
    let cfg = env.cfg;
    let roms = cfg.roms.clone().ok_or_else(|| fail("Dossier des jeux non configuré sur cet appareil (option roms)"))?;

    // Installation : le catalogue donne le fichier de base ; sans serveur, on cherche la ROM sur place.
    obs.step(Step::Telechargement, StepState::EnCours, None);
    let rom = match client.games() {
        Ok(games) => {
            let game = games.into_iter().find(|g| g.id == key.game).ok_or_else(|| fail("Jeu introuvable sur le serveur"))?;
            let base = game.files.iter().find(|f| f.role == FileRole::Base).ok_or_else(|| fail("Aucun fichier de base pour ce jeu sur le serveur"))?;
            let rom = roms.join(&base.path);
            if rom.exists() {
                obs.step(Step::Telechargement, StepState::Fait, Some("Jeu installé".into()));
            } else {
                obs.install(&game).map_err(LaunchError::Failed)?;
                if !rom.exists() {
                    return Err(fail("Le jeu n'est pas présent après l'installation"));
                }
                obs.step(Step::Telechargement, StepState::Fait, None);
            }
            rom
        }
        Err(ApiError::Offline(_)) => {
            let rom = find_rom_offline(&roms, &conf.title_id).ok_or_else(|| fail("Jeu non installé et serveur injoignable"))?;
            obs.step(Step::Telechargement, StepState::Fait, Some("Jeu installé".into()));
            rom
        }
        Err(e) => return Err(fail(e)),
    };

    // Synchronisation avant : le temps de jeu reste celui du serveur (inconnu = 0, relu après la partie).
    obs.step(Step::Sauvegarde, StepState::EnCours, None);
    let known_playtime = head_playtime(client, key);
    let base_playtime = known_playtime.unwrap_or(0);
    let sync = |playtime_s: u64| sync_save(env, client, key, conf, prepared, playtime_s);
    let before = sync(base_playtime)?;
    match &before {
        SyncOutcome::Conflict { server } => return Ok(LaunchOutcome::Conflict(conflict_info(env, client, key, server.clone(), false))),
        SyncOutcome::LocalMissing => return Err(fail("Dossier de sauvegarde introuvable : rien n'a été modifié")),
        _ => {}
    }
    obs.step(Step::Sauvegarde, StepState::Fait, outcome_detail(&before));

    // Émulateur, dans son propre groupe de processus (il peut être terminé proprement si l'app s'arrête).
    obs.step(Step::Lancement, StepState::EnCours, None);
    let argv = config::launch_command(prepared, &rom).map_err(fail)?;
    let (bin, args) = argv.split_first().ok_or_else(|| fail("commande de lancement vide"))?;
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args).stdin(std::process::Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd.spawn().map_err(|e| fail(format!("Échec du lancement de l'émulateur : {e}")))?;
    let _running = Running::register(child.id());
    let started = Instant::now();
    obs.step(Step::Lancement, StepState::Fait, None);
    obs.step(Step::EnJeu, StepState::EnCours, None);
    let status = child.wait().map_err(|e| fail(format!("Émulateur : attente impossible : {e}")))?;
    let elapsed_s = started.elapsed().as_secs();
    drop(_running);
    let exit_ok = status.success();
    obs.step(Step::EnJeu, StepState::Fait, None);

    // Synchronisation après, même si l'émulateur a terminé en erreur : la sauvegarde peut être valide.
    // Le temps de jeu de départ était peut-être inconnu (hors ligne) : la tête est relue maintenant,
    // et jamais un temps inférieur à celui de la tête connue n'est envoyé.
    obs.step(Step::Envoi, StepState::EnCours, None);
    let base_now = head_playtime(client, key).map_or(base_playtime, |h| h.max(base_playtime));
    let playtime_s = base_now + elapsed_s;
    let after = match sync(playtime_s) {
        Ok(a) => a,
        Err(e) => return Err(LaunchError::SendFailed { message: e.to_string(), playtime_s, exit_ok }),
    };
    if let SyncOutcome::Conflict { server } = &after {
        return Ok(LaunchOutcome::Conflict(conflict_info(env, client, key, server.clone(), true)));
    }
    obs.step(Step::Envoi, StepState::Fait, outcome_detail(&after));
    obs.step(Step::Termine, StepState::Fait, None);
    Ok(LaunchOutcome::Played { before, after, playtime_s, elapsed_s, exit_ok })
}

fn sync_save(env: &Env, client: &Client, key: &SaveKey, conf: &config::GameConf, prepared: &config::Prepared, playtime_s: u64) -> Result<SyncOutcome, LaunchError> {
    env.locked(|journal| {
        let mut engine = Engine { client, journal, device: env.cfg.appareil.clone(), emulator: conf.emulateur.clone(), playtime_s };
        engine.sync(key, prepared.layout.as_ref()).map_err(fail)
    })
}

/// Groupes de processus des émulateurs en cours (un par partie).
static RUNNING: Mutex<Vec<u32>> = Mutex::new(Vec::new());

struct Running(u32);

impl Running {
    fn register(pid: u32) -> Self {
        RUNNING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(pid);
        Self(pid)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).retain(|p| *p != self.0);
    }
}

/// Une partie est-elle en cours (émulateur lancé par ce processus) ?
pub fn game_running() -> bool {
    !RUNNING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_empty()
}

/// Termine proprement (SIGTERM) les émulateurs en cours : à appeler quand l'app s'arrête pour de
/// bon. La sauvegarde reste locale ; elle sera synchronisée au prochain lancement.
pub fn terminate_running() {
    #[cfg(unix)]
    for pid in RUNNING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter() {
        if let Ok(pgid) = i32::try_from(*pid) {
            // SAFETY : kill(2) sur le groupe de processus créé pour cet émulateur (pgid = pid).
            unsafe { libc::kill(-pgid, libc::SIGTERM) };
        }
    }
}

/// Suite d'un choix de conflit ou d'un renvoi de sauvegarde.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settled {
    Done(SyncOutcome),
    /// Le conflit demeure (ou la tête a bougé depuis l'affichage) : à retrancher avec cette version.
    Conflict(ConflictInfo),
}

/// Applique le choix du joueur après un conflit (`Engine::resolve` : l'autre version est archivée
/// comme écartée côté serveur). Le choix porte sur la version du serveur affichée (`shown_rev`) : si
/// la tête a changé depuis, rien n'est appliqué et le conflit à jour est renvoyé.
/// Le verrou est pris sous `<appareil>-ui` : une partie en cours, même ici, refuse l'opération.
pub fn resolve(env: &Env, key: &SaveKey, choice: Resolution, shown_rev: u64) -> Result<Settled, LaunchError> {
    let cfg = env.cfg;
    let conf = cfg
        .jeux
        .get(&key.game)
        .filter(|g| g.concerns(&key.profile))
        .ok_or_else(|| fail("Jeu non configuré sur cet appareil pour ce profil"))?;
    let prepared = config::prepare(cfg, &conf.emulateur, &key.profile, &conf.title_id).map_err(fail)?;
    let client = Client::new(&cfg.serveur);
    let ui_device = format!("{}-ui", cfg.appareil);
    let _lease = take_lease(&client, key, &ui_device)?;
    if let Some(head) = client.head(key).map_err(fail)? {
        if head.rev != shown_rev {
            return Ok(Settled::Conflict(conflict_info(env, &client, key, head, false)));
        }
    }
    let playtime_s = head_playtime(&client, key).unwrap_or(0);
    let out = env.locked(|journal| {
        let mut engine = Engine { client: &client, journal, device: cfg.appareil.clone(), emulator: conf.emulateur.clone(), playtime_s };
        engine.resolve(key, prepared.layout.as_ref(), choice).map_err(fail)
    })?;
    Ok(match out {
        SyncOutcome::Conflict { server } => Settled::Conflict(conflict_info(env, &client, key, server, false)),
        o => Settled::Done(o),
    })
}

/// Renvoie la sauvegarde d'une partie déjà jouée, sans relancer le jeu (après un échec d'envoi).
/// Le temps de jeu n'est jamais inférieur à celui de la tête du serveur.
pub fn send(env: &Env, key: &SaveKey, playtime_s: u64) -> Result<Settled, LaunchError> {
    let cfg = env.cfg;
    let conf = cfg
        .jeux
        .get(&key.game)
        .filter(|g| g.concerns(&key.profile))
        .ok_or_else(|| fail("Jeu non configuré sur cet appareil pour ce profil"))?;
    let prepared = config::prepare(cfg, &conf.emulateur, &key.profile, &conf.title_id).map_err(fail)?;
    let client = Client::new(&cfg.serveur);
    let _lease = take_lease(&client, key, &app_device(&cfg.appareil))?;
    let playtime_s = head_playtime(&client, key).map_or(playtime_s, |h| h.max(playtime_s));
    Ok(match sync_save(env, &client, key, conf, &prepared, playtime_s)? {
        SyncOutcome::Conflict { server } => Settled::Conflict(conflict_info(env, &client, key, server, true)),
        o => Settled::Done(o),
    })
}

#[cfg(test)]
mod tests_temps_de_jeu {
    use super::playtime_after;

    #[test]
    fn jamais_moins_que_le_cumul_connu() {
        // Partie commencée hors ligne (cumul inconnu), serveur revenu : tête relue + durée.
        assert_eq!(playtime_after(None, Some(180_000), 2_400), 182_400);
        // Cumul connu avant, tête avancée ailleurs pendant la partie : la plus grande valeur.
        assert_eq!(playtime_after(Some(100), Some(500), 60), 560);
        assert_eq!(playtime_after(Some(500), Some(100), 60), 560);
        // Hors ligne avant et après : la durée seule (l'envoi reste en attente).
        assert_eq!(playtime_after(None, None, 60), 60);
        assert_eq!(playtime_after(Some(u64::MAX), None, 10), u64::MAX);
    }
}
