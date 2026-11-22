//! Lancement d'un jeu depuis l'interface : même déroulé que `ludo run` (verrou de partie,
//! synchronisation avant, émulateur, synchronisation après avec le temps de jeu, libération du
//! verrou), plus l'installation du jeu s'il manque et des étapes que l'interface peut afficher.
//!
//! Aucune sauvegarde n'est écrite en dehors du moteur de synchronisation (`Engine::sync` et
//! `Engine::resolve` : compare-and-swap, versions écartées archivées). Le journal n'est verrouillé
//! que le temps de chaque synchronisation, jamais pendant la partie.

use crate::api::{ApiError, Client};
use crate::config::{self, Config, GameConf};
use crate::engine::{Engine, Resolution, SyncOutcome};
use crate::journal::Journal;
use ludo_proto::{CatalogGame, FileRole, LeaseOutcome, SaveKey, SaveRevision};
use std::path::{Path, PathBuf};

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

/// Demande d'arrêt d'un fil : `wait` dort jusqu'à l'arrêt OU l'échéance, et se réveille aussitôt
/// qu'on appelle `request` (Condvar) — aucune relecture périodique d'un drapeau.
#[derive(Default)]
pub struct Stop {
    stopped: Mutex<bool>,
    cv: std::sync::Condvar,
}

impl Stop {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request(&self) {
        *self.stopped.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.cv.notify_all();
    }

    /// `true` si l'arrêt a été demandé avant l'échéance `d`.
    pub fn wait(&self, d: Duration) -> bool {
        let g = self.stopped.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (g, _) = self.cv.wait_timeout_while(g, d, |s| !*s).unwrap_or_else(std::sync::PoisonError::into_inner);
        *g
    }
}

/// Demande l'arrêt à la destruction : le fil de renouvellement s'arrête même si la partie panique.
pub struct StopOnDrop<'a>(pub &'a Stop);

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.request();
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

#[cfg(test)]
mod tests_demarrage {
    use super::*;

    #[test]
    fn jeu_extrait_demarre_sur_la_mise_a_jour_la_plus_recente() {
        let t = tempfile::tempdir().unwrap();
        let g = t.path().join("Pokemon Pokopia [0400A3A01EE4C000]");
        for d in ["base/exefs", "updates/65536/exefs", "updates/327680/exefs"] {
            std::fs::create_dir_all(g.join(d)).unwrap();
            std::fs::write(g.join(d).join("main"), b"x").unwrap();
        }
        assert_eq!(boot_file(&g), g.join("updates/327680/exefs/main"));
        let base_seule = t.path().join("Jeu [0400000000000000]");
        std::fs::create_dir_all(base_seule.join("base/exefs")).unwrap();
        std::fs::write(base_seule.join("base/exefs/main"), b"x").unwrap();
        assert_eq!(boot_file(&base_seule), base_seule.join("base/exefs/main"));
        let f = t.path().join("Zelda [0400486024DEC000][v131072].nup");
        std::fs::write(&f, b"x").unwrap();
        assert_eq!(boot_file(&f), f, "un fichier est rendu tel quel");
    }
}

#[cfg(test)]
mod tests_flux {
    use super::*;

    /// Les essais de signal partagent l'indicateur d'arrêt du processus : un à la fois.
    static SERIE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn nom_d_appareil_en_flux() {
        assert_eq!(stream_device("pc", "AYN Thor"), "pc-via-thor");
        assert_eq!(stream_device("pc", "Steam Deck"), "pc-via-deck");
        assert_eq!(stream_device("pc", "iPhone de G"), "pc-via-iphone-de-g");
        assert_eq!(stream_device("pc", "  "), "pc");
    }

    #[test]
    fn signal_d_arret_termine_l_emulateur_puis_rend_la_main() {
        let _serie = SERIE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // Faux émulateur : dort longtemps ; on simule le SIGTERM reçu par `ludo` depuis un fil.
        let t = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(300));
            // SAFETY : signal envoyé à ce processus de test ; le gestionnaire ne fait qu'une écriture atomique.
            unsafe { libc::kill(libc::getpid(), libc::SIGTERM) };
        });
        let started = Instant::now();
        let (status, stopped) = run_emulator("sleep", &["30".to_string()], Duration::from_secs(5)).unwrap();
        t.join().unwrap();
        assert!(stopped, "arrêt demandé par signal");
        assert!(!status.success());
        assert!(started.elapsed() < Duration::from_secs(5), "émulateur arrêté par SIGTERM, sans attendre SIGKILL");
    }

    #[test]
    fn emulateur_qui_ignore_sigterm_recoit_sigkill() {
        let _serie = SERIE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let t = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(300));
            unsafe { libc::kill(libc::getpid(), libc::SIGTERM) };
        });
        let started = Instant::now();
        let (_, stopped) = run_emulator("sh", &["-c".to_string(), "trap '' TERM; sleep 30".to_string()], Duration::from_millis(800)).unwrap();
        t.join().unwrap();
        assert!(stopped);
        assert!(started.elapsed() < Duration::from_secs(5), "SIGKILL après le délai de grâce");
    }
}

/// Délai laissé à l'émulateur pour écrire sa sauvegarde et se fermer après SIGTERM, avant SIGKILL.
pub const EMULATOR_GRACE: Duration = Duration::from_secs(20);

/// Lance l'émulateur dans son propre groupe de processus et attend sa fin. Si `ludo` reçoit SIGTERM,
/// SIGINT ou SIGHUP pendant la partie (fin du flux Sunshine, Ctrl+C), l'émulateur (tout son groupe)
/// reçoit SIGTERM, puis SIGKILL s'il n'est pas sorti après `grace`. Renvoie le statut et `true` si la
/// fin a été demandée par un signal.
///
/// Événementiel : un fil attend la sortie de l'émulateur (`wait`), un autre les signaux
/// (`signal-hook`) ; les deux écrivent dans un canal sur lequel on dort. Seul délai : la garde
/// unique de SIGKILL.
pub fn run_emulator(bin: &str, args: &[String], grace: Duration) -> std::io::Result<(std::process::ExitStatus, bool)> {
    enum Ev {
        Exited(std::io::Result<std::process::ExitStatus>),
        Stop,
    }
    let mut signals = signal_hook::iterator::Signals::new([libc::SIGTERM, libc::SIGINT, libc::SIGHUP])?;
    let signals_handle = signals.handle();
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args);
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd.spawn()?;
    let pgid = i32::try_from(child.id()).unwrap_or(0);
    let (tx, rx) = std::sync::mpsc::channel::<Ev>();
    let tx_sig = tx.clone();
    let sig_thread = std::thread::spawn(move || {
        if signals.forever().next().is_some() {
            let _ = tx_sig.send(Ev::Stop);
        }
    });
    std::thread::spawn(move || {
        let _ = tx.send(Ev::Exited(child.wait()));
    });
    let finish = |status: std::io::Result<std::process::ExitStatus>, stopped: bool| {
        signals_handle.close();
        let _ = sig_thread.join();
        status.map(|s| (s, stopped))
    };
    match rx.recv() {
        Ok(Ev::Exited(status)) => finish(status, false),
        Ok(Ev::Stop) => {
            // SAFETY : kill(2) sur le groupe de processus créé pour cet émulateur (pgid = pid).
            unsafe { libc::kill(-pgid, libc::SIGTERM) };
            // Garde unique : SIGKILL si l'émulateur n'a pas quitté dans le délai.
            let status = match rx.recv_timeout(grace) {
                Ok(Ev::Exited(status)) => status,
                _ => {
                    // SAFETY : idem.
                    unsafe { libc::kill(-pgid, libc::SIGKILL) };
                    loop {
                        match rx.recv() {
                            Ok(Ev::Exited(status)) => break status,
                            Ok(Ev::Stop) => continue,
                            Err(e) => break Err(std::io::Error::other(e)),
                        }
                    }
                }
            };
            finish(status, true)
        }
        Err(e) => finish(Err(std::io::Error::other(e)), false),
    }
}

/// Nom d'appareil du verrou pour une partie jouée en flux (Moonlight → Sunshine) : `pc-via-thor`.
/// Sunshine fournit le nom du client dans `SUNSHINE_CLIENT_NAME` (ex. « AYN Thor ») ; un nom qui
/// contient « thor » ou « deck » est abrégé pour que l'interface reconnaisse l'appareil.
pub fn stream_device(appareil: &str, client: &str) -> String {
    let lower = client.to_lowercase();
    let short = if lower.contains("thor") {
        "thor".to_string()
    } else if lower.contains("deck") {
        "deck".to_string()
    } else {
        lower.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>().trim_matches('-').to_string()
    };
    if short.is_empty() { appareil.to_string() } else { format!("{appareil}-via-{short}") }
}

/// Le verrou expire au bout de 120 s : il est renouvelé tant que la partie dure.
const HEARTBEAT: Duration = Duration::from_secs(40);

/// Renouvelle le bail toutes les 40 s (exigé par le protocole : TTL 120 s côté serveur, pour libérer
/// une partie si l'appareil disparaît) ; s'arrête dès que `stop` est demandé, sans attendre l'échéance.
pub fn heartbeat(client: &Client, key: &SaveKey, device: &str, stop: &Stop) {
    while !stop.wait(HEARTBEAT) {
        let _ = client.acquire_lease(key, device, false);
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

/// ROM de base présente sur l'appareil : le fichier du catalogue, ou sa version convertie par le
/// serveur (NSZ → NSP, XCZ → XCI) pour un émulateur qui ne lit pas les formats compressés (Eden).
pub fn installed_rom(roms: &Path, catalog_rel: &str) -> Option<PathBuf> {
    let original = roms.join(catalog_rel);
    if original.exists() {
        return Some(original);
    }
    crate::install::converted_rel(catalog_rel).map(|r| roms.join(r)).filter(|p| p.exists())
}

/// Fichier à donner à l'émulateur. Un jeu EXTRAIT (dossier, ex. Switch 2 « Pokopia [id]/ ») démarre
/// sur `exefs/main` : celui de la mise à jour la plus récente (`updates/<version>/`), sinon de
/// `base/`, sinon du dossier lui-même. Un fichier est rendu tel quel.
pub fn boot_file(rom: &Path) -> PathBuf {
    if !rom.is_dir() {
        return rom.to_path_buf();
    }
    let main_of = |d: &Path| Some(d.join("exefs/main")).filter(|m| m.is_file());
    let mut updates: Vec<(u64, PathBuf)> = std::fs::read_dir(rom.join("updates"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some((e.file_name().to_str()?.parse::<u64>().ok()?, e.path())))
        .collect();
    updates.sort();
    updates
        .iter()
        .rev()
        .find_map(|(_, d)| main_of(d))
        .or_else(|| main_of(&rom.join("base")))
        .or_else(|| main_of(rom))
        .unwrap_or_else(|| rom.to_path_buf())
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

/// Émulateur et title_id du jeu : section `[jeux]` de l'appareil, sinon émulateur par défaut de la
/// console avec le title_id du catalogue du serveur (lu seulement si le jeu n'est pas configuré).
fn game_conf(cfg: &Config, key: &SaveKey) -> Result<GameConf, LaunchError> {
    if let Some(g) = cfg.game_conf(&key.game, &key.profile, None) {
        return Ok(g);
    }
    if cfg.jeux.contains_key(&key.game) {
        return Err(fail("Jeu non configuré sur cet appareil pour ce profil"));
    }
    let catalog = Client::new(&cfg.serveur).games().map_err(fail)?;
    let game = catalog.iter().find(|g| g.id == key.game);
    cfg.game_conf(&key.game, &key.profile, game).ok_or_else(|| {
        fail(match game {
            None => "Jeu introuvable sur le serveur".to_string(),
            Some(g) => match cfg.default_emulator(&g.console) {
                None => format!("Aucun émulateur pour la console {} sur cet appareil", g.console),
                Some(_) => "Identifiant du jeu (title_id) inconnu : à renseigner dans catalogue.toml".to_string(),
            },
        })
    })
}

/// Lance un jeu : verrou, installation si besoin, synchronisation avant, émulateur, attente de sa
/// fermeture, synchronisation après (temps de jeu = tête du serveur + durée de la partie).
///
/// Conflit avant la partie : rien n'est lancé, le verrou est rendu, la sauvegarde n'est pas touchée.
/// Émulateur introuvable ou en échec au démarrage : erreur, verrou rendu.
pub fn launch(env: &Env, key: &SaveKey, obs: &mut dyn Observer) -> Result<LaunchOutcome, LaunchError> {
    let cfg = env.cfg;
    obs.step(Step::Profil, StepState::EnCours, None);
    let conf = &game_conf(cfg, key)?;
    // Adaptateur impossible à construire : erreur avant de toucher au serveur.
    let prepared = config::prepare(cfg, &conf.emulateur, &key.profile, &conf.title_id).map_err(fail)?;
    let client = Client::new(&cfg.serveur);
    let device = app_device(&cfg.appareil);
    let lease = take_lease(&client, key, &device)?;
    obs.step(Step::Profil, StepState::Fait, None);

    let stop = Stop::new();
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
            // Dossier de la Ludothèque, sinon collection déjà présente (lancée sur place, jamais copiée).
            let local = crate::local::LocalIndex::scan(cfg, &crate::detect::Host::current());
            match installed_rom(&roms, &base.path).or_else(|| local.find(&game.console, &base.path).cloned()) {
                Some(rom) => {
                    obs.step(Step::Telechargement, StepState::Fait, Some("Jeu installé".into()));
                    rom
                }
                None => {
                    obs.install(&game).map_err(LaunchError::Failed)?;
                    let rom = installed_rom(&roms, &base.path).ok_or_else(|| fail("Le jeu n'est pas présent après l'installation"))?;
                    obs.step(Step::Telechargement, StepState::Fait, None);
                    rom
                }
            }
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
    let argv = config::launch_command(prepared, &boot_file(&rom)).map_err(fail)?;
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
    let conf = &game_conf(cfg, key)?;
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
    let conf = &game_conf(cfg, key)?;
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

#[cfg(test)]
mod tests_rom_convertie {
    use super::installed_rom;

    #[test]
    fn version_convertie_trouvee() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("switch")).unwrap();
        // Catalogue : NSZ ; appareil (Eden) : NSP converti par le serveur.
        std::fs::write(d.path().join("switch/Jeu [0100000000010000].nsp"), b"x").unwrap();
        assert_eq!(
            installed_rom(d.path(), "switch/Jeu [0100000000010000].nsz"),
            Some(d.path().join("switch/Jeu [0100000000010000].nsp"))
        );
        // L'original présent a la priorité ; rien d'installé : None.
        std::fs::write(d.path().join("switch/Jeu [0100000000010000].nsz"), b"y").unwrap();
        assert_eq!(installed_rom(d.path(), "switch/Jeu [0100000000010000].nsz"), Some(d.path().join("switch/Jeu [0100000000010000].nsz")));
        assert_eq!(installed_rom(d.path(), "switch/Autre.nsz"), None);
    }
}

/// Arguments que Sunshine (Linux) passe réellement au programme d'une appli de `apps.json` : sans
/// shell, la commande est découpée par boost.process v1 (`build_args`), reproduit ici à l'identique :
/// coupure sur l'espace hors guillemets, chaque `"` bascule l'état « entre guillemets » (même un
/// `\"`), la paire de `"` qui encadre tout le mot est retirée, puis `\"` devient `"`. Une apostrophe
/// reste un caractère ordinaire : `--rom '/a b.nsp'` donne `'/a` et `b.nsp'`.
pub fn sunshine_argv(cmd: &str) -> Vec<String> {
    fn entry(word: &str) -> String {
        let inner = if word.len() >= 2 && word.starts_with('"') && word.ends_with('"') { &word[1..word.len() - 1] } else { word };
        inner.replace("\\\"", "\"")
    }
    let mut out = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    let mut prev = None;
    for (i, c) in cmd.char_indices() {
        if c == '"' {
            quoted = !quoted;
        }
        if !quoted && c == ' ' {
            if i != 0 && prev != Some(' ') {
                out.push(entry(&cmd[start..i]));
            }
            start = i + 1;
        }
        prev = Some(c);
    }
    if start < cmd.len() {
        out.push(entry(&cmd[start..]));
    }
    out
}

/// Vrai si la commande lance `ludo run` : un mot dont le nom de fichier est exactement `ludo`, suivi
/// plus loin de la sous-commande `run` (options globales possibles entre les deux, ex. `--config`).
pub fn is_ludo_run(argv: &[String]) -> bool {
    argv.iter()
        .position(|w| Path::new(w).file_name().is_some_and(|n| n == "ludo"))
        .is_some_and(|i| argv[i + 1..].iter().any(|w| w == "run"))
}

/// Problème d'une commande d'appli Sunshine `ludo run`, vue comme Sunshine la découpe : mot qui
/// commence par une apostrophe (chemin cité « à la shell », coupé aux espaces) ou guillemet double
/// jamais refermé. `ludo` refuserait alors ses arguments et quitterait au démarrage du flux, sans
/// lancer le jeu (18/11 : « App exited » dès la connexion Moonlight).
pub fn sunshine_cmd_problem(cmd: &str) -> Option<String> {
    let argv = sunshine_argv(cmd);
    if !is_ludo_run(&argv) {
        return None;
    }
    if cmd.matches('"').count() % 2 == 1 {
        return Some("guillemet double non refermé".to_string());
    }
    argv.iter()
        .find(|w| w.starts_with('\''))
        .map(|w| format!("mot cité par des apostrophes ({w}) : Sunshine ne les comprend pas, utiliser des guillemets doubles"))
}

#[cfg(test)]
mod tests_sunshine {
    use super::*;

    #[test]
    fn guillemets_doubles_groupent_les_mots() {
        let cmd = r#"/bin/ludo run --rom "/mnt/Rom/Luigi's Mansion 3 [x].nsp" --lanceur "env A=1 /bin/ryu-stream""#;
        assert_eq!(sunshine_argv(cmd), ["/bin/ludo", "run", "--rom", "/mnt/Rom/Luigi's Mansion 3 [x].nsp", "--lanceur", "env A=1 /bin/ryu-stream"]);
        // Une apostrophe À L'INTÉRIEUR d'un mot cité (Luigi's) n'est pas un problème.
        assert_eq!(sunshine_cmd_problem(cmd), None);
    }

    #[test]
    fn regle_boost_guillemets_au_milieu_espaces_multiples_et_echappement() {
        // Guillemets au milieu d'un mot : conservés (seule la paire qui encadre le mot est retirée).
        assert_eq!(sunshine_argv(r#"a --rom="/x y" b"#), ["a", r#"--rom="/x y""#, "b"]);
        // Espaces multiples : aucun mot vide.
        assert_eq!(sunshine_argv("a   b "), ["a", "b"]);
        // `\"` devient `"` après découpage…
        assert_eq!(sunshine_argv(r#"a \"b\" c"#), ["a", r#""b""#, "c"]);
        // …mais bascule aussi l'état (bizarrerie de boost, reproduite) : coupure inattendue.
        assert_eq!(sunshine_argv(r#"a "x \" y" b"#), ["a", r#"x \"#, r#"y" b"#]);
    }

    #[test]
    fn apostrophes_coupent_le_chemin_et_sont_signalees() {
        // Commande réelle de l'appli « Mario Kart 8 Deluxe · Ludothèque » au 18/11.
        let cmd = "/home/u/.local/bin/ludo run --profil guilhem --jeu switch-0100152000022000 --rom '/mnt/Rom/Mario Kart 8 Deluxe [0100152000022000][v0].nsp' --lanceur \"env LSFG_MULT=2 /bin/ryu-stream\"";
        let argv = sunshine_argv(cmd);
        assert!(argv.contains(&"Kart".to_string()));
        assert!(sunshine_cmd_problem(cmd).is_some());
        assert_eq!(sunshine_cmd_problem(&cmd.replace('\'', "\"")), None);
    }

    #[test]
    fn guillemet_non_referme_signale() {
        assert!(sunshine_cmd_problem(r#"/bin/ludo run --rom "/a b.nsp"#).is_some());
    }

    #[test]
    fn autre_appli_jamais_signalee() {
        assert_eq!(sunshine_cmd_problem("/bin/ryu-stream '/a b.nsp'"), None);
        assert_eq!(sunshine_cmd_problem("/usr/bin/studio run '/a b'"), None);
        assert_eq!(sunshine_cmd_problem("/usr/bin/pseudo run '/a b'"), None);
        assert_eq!(sunshine_cmd_problem(""), None);
    }

    #[test]
    fn ludo_run_avec_options_globales() {
        let argv = sunshine_argv("/bin/ludo --config /c.toml run --jeu x");
        assert!(is_ludo_run(&argv));
        assert!(!is_ludo_run(&sunshine_argv("/bin/ludo watch")));
    }
}
