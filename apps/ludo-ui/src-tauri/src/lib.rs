//! Coque Tauri de l'interface Ludothèque : expose au front les commandes du client `ludo`.
//!
//! Commandes branchées : `device`, `rumble` (vibrations natives), `profiles`, `saveProfile`,
//! `history`, `restore`, `sync`, `library`, `recent` (catalogue du serveur), `installGame`,
//! `installContent`, `uninstallGame` (installation réelle sur l'appareil ; progression par
//! l'événement `ludo://progression`), `launchGame` (lancement réel : verrou, sauvegarde, émulateur,
//! envoi ; étapes par l'événement `ludo://passage`) et `resolveConflict`. Les autres (`updates`, `prepare`) arriveront avec la suite du
//! lot P1c ; d'ici là, le front utilise sa source de démonstration pour chaque commande absente.

pub mod rumble;

use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike};
use ludo_client::api::Client;
use ludo_client::config::{self, Config};
use ludo_client::engine::{Engine, Resolution, SyncOutcome};
use ludo_client::install::{self, Installer};
use ludo_client::journal::{Entry, Journal};
use ludo_client::launch::{self, ConflictInfo, LaunchError, LaunchOutcome, Observer, Settled, Step, StepState};
use ludo_proto::{CatalogGame, FileRole, GameArt, GameFile, LeaseOutcome, Profile, SaveKey, SaveSummary};
use serde::Serialize;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    name: String,
    kind: &'static str,
    default_profile: Option<String>,
    server: &'static str,
    free_space: String,
    /// Adresse du serveur (Réglages).
    server_url: String,
    /// Version de l'app.
    version: &'static str,
}

/// Type d'appareil déduit de son identifiant dans la configuration.
pub fn device_kind(id: &str) -> &'static str {
    let id = id.to_lowercase();
    if id.contains("deck") {
        "deck"
    } else if id.contains("thor") {
        "thor"
    } else if id.contains("tv") {
        "tv"
    } else {
        "pc"
    }
}

/// Nom affiché d'un appareil (« steam-deck » → « Steam Deck »).
pub fn device_name(id: &str) -> String {
    match device_kind(id) {
        "deck" => "Steam Deck".into(),
        "thor" => "Ayn Thor".into(),
        "pc" => "PC".into(),
        _ => id.to_string(),
    }
}

#[tauri::command]
async fn device() -> Result<DeviceInfo, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let cfg = Config::load(&Config::default_path()).map_err(|e| e.to_string())?;
        let server = if Client::new(&cfg.serveur).health().is_ok() { "connecte" } else { "hors_ligne" };
        Ok(DeviceInfo {
            name: device_name(&cfg.appareil),
            kind: device_kind(&cfg.appareil),
            default_profile: cfg.profil_defaut.clone(),
            server,
            free_space: cfg
                .roms
                .as_deref()
                .and_then(ludo_client::install::free_space)
                .map(|b| format!("{} libres", size_label(b)))
                .unwrap_or_default(),
            server_url: cfg.serveur.clone(),
            version: env!("CARGO_PKG_VERSION"),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Émulateur connu (fiche) et ce qui en a été trouvé sur l'appareil (`Emulator` de `types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiEmulator {
    id: String,
    name: String,
    consoles: Vec<String>,
    /// Émulateur par défaut de ces consoles sur l'appareil.
    default_for: Vec<String>,
    found: bool,
    /// « Flatpak io.github… », « Réglage de l'appareil », « Chemin de la fiche »… ; vide si absent.
    source: String,
    bin: String,
    data: String,
}

/// Recherche les émulateurs sur l'appareil, à chaque appel (aucun cache : un émulateur installé
/// depuis est vu tout de suite). Lecture seule.
#[tauri::command]
async fn emulators() -> Result<Vec<UiEmulator>, String> {
    blocking(|| {
        let cfg = load_config()?;
        let host = ludo_client::detect::Host::current();
        let mut out = Vec::new();
        for id in cfg.fiche_ids() {
            let Ok(fiche) = cfg.fiche(&id) else { continue };
            let default_for = fiche.consoles.iter().filter(|c| cfg.default_emulator(c).as_deref() == Some(id.as_str())).cloned().collect();
            let (found, source, bin, data) = match cfg.locate(&fiche, &host) {
                Ok(l) => {
                    // La section [appareil] de la fiche n'est qu'une supposition : vraie si le programme existe.
                    let present = match &l.origin {
                        config::Origin::Fiche => bin_exists(&l.bin, &host),
                        _ => true,
                    };
                    let source = match l.origin {
                        config::Origin::Config => "Réglage de l'appareil".to_string(),
                        config::Origin::Detected(s) => s,
                        config::Origin::Fiche => "Chemin de la fiche".to_string(),
                    };
                    (present, if present { source } else { String::new() }, l.bin, l.data.display().to_string())
                }
                Err(_) => (false, String::new(), String::new(), String::new()),
            };
            out.push(UiEmulator { name: emulator_label(Some(&id), ""), id, consoles: fiche.consoles.clone(), default_for, found, source, bin, data });
        }
        Ok(out)
    })
    .await
}

/// Le programme d'une commande de lancement existe-t-il ? (`flatpak run <id>` : application installée.)
fn bin_exists(bin: &str, host: &ludo_client::detect::Host) -> bool {
    let mut words = bin.split_whitespace();
    match (words.next(), words.next(), words.next()) {
        (Some("flatpak"), Some("run"), Some(id)) => {
            host.home.join(".local/share/flatpak/app").join(id).is_dir() || host.system_flatpak.join("app").join(id).is_dir()
        }
        (Some(first), _, _) => config::expand_in(&host.home, first).is_file(),
        _ => false,
    }
}

/// Le journal est relu puis réécrit d'un bloc à chaque opération : une seule opération à la fois
/// dans l'interface, pour ne pas perdre l'écriture d'une synchronisation concurrente.
static JOURNAL: Mutex<()> = Mutex::new(());

fn load_config() -> Result<Config, String> {
    let path = Config::default_path();
    Config::load(&path).map_err(|e| format!("{} : {e}", path.display()))
}

/// Exécute un travail bloquant (réseau, disque) hors du fil de l'interface.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

/// Profils du foyer enregistrés sur le serveur.
///
/// Aucun profil sur le serveur : erreur, pour que l'interface garde ses profils de démonstration
/// tant qu'aucun écran de création n'existe (le premier `saveProfile` les enregistre).
#[tauri::command]
async fn profiles() -> Result<Vec<Profile>, String> {
    blocking(|| {
        let cfg = load_config()?;
        let list = Client::new(&cfg.serveur).profiles().map_err(|e| e.to_string())?;
        if list.is_empty() {
            return Err("aucun profil sur le serveur".into());
        }
        Ok(list)
    })
    .await
}

/// Enregistre le profil (image, nom, préférences) ; renvoie la version enregistrée.
#[tauri::command(rename = "saveProfile")]
async fn save_profile(profile: Profile) -> Result<Profile, String> {
    blocking(move || {
        let cfg = load_config()?;
        Client::new(&cfg.serveur).put_profile(&profile).map_err(|e| e.to_string())
    })
    .await
}

/// Une version de sauvegarde telle que l'interface l'affiche (`SaveRevision` de `types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiRevision {
    rev: u64,
    device: String,
    device_kind: &'static str,
    when: String,
    playtime: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    current: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    discarded: bool,
}

const JOURS: [&str; 7] = ["Lun.", "Mar.", "Mer.", "Jeu.", "Ven.", "Sam.", "Dim."];
const MOIS: [&str; 12] = ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];

/// Date lisible : « Aujourd'hui 20:15 », « Hier 21:58 », « Sam. 8 nov. 16:12 » (année ajoutée
/// si elle diffère de celle du jour).
pub fn when_label(t: NaiveDateTime, today: NaiveDate) -> String {
    let hm = format!("{:02}:{:02}", t.hour(), t.minute());
    let d = t.date();
    if d == today {
        format!("Aujourd'hui {hm}")
    } else if today.pred_opt() == Some(d) {
        format!("Hier {hm}")
    } else {
        let jour = JOURS[d.weekday().num_days_from_monday() as usize];
        let mois = MOIS[d.month0() as usize];
        if d.year() == today.year() {
            format!("{jour} {} {mois} {hm}", d.day())
        } else {
            format!("{jour} {} {mois} {} {hm}", d.day(), d.year())
        }
    }
}

/// Temps de jeu lisible : « 12 h 40 ».
pub fn playtime_label(seconds: u64) -> String {
    let minutes = seconds / 60;
    format!("{} h {:02}", minutes / 60, minutes % 60)
}

/// Historique du serveur (plus récent d'abord) vers l'affichage : la tête est la révision la plus
/// récente qui n'a pas été écartée.
pub fn ui_history(revs: &[ludo_proto::SaveRevision], to_local: impl Fn(u64) -> NaiveDateTime, today: NaiveDate) -> Vec<UiRevision> {
    let head = revs.iter().find(|r| !r.discarded).map(|r| r.rev);
    revs.iter()
        .map(|r| UiRevision {
            rev: r.rev,
            device: device_name(&r.device),
            device_kind: device_kind(&r.device),
            when: when_label(to_local(r.created_at), today),
            playtime: playtime_label(r.playtime_s),
            current: Some(r.rev) == head,
            discarded: r.discarded,
        })
        .collect()
}

fn local_time(ts: u64) -> NaiveDateTime {
    let ts = i64::try_from(ts).unwrap_or(i64::MAX);
    match Local.timestamp_opt(ts, 0).single() {
        Some(t) => t.naive_local(),
        None => chrono::DateTime::from_timestamp(ts, 0).unwrap_or_default().naive_utc(),
    }
}

/// Historique des sauvegardes d'un jeu pour un profil.
#[tauri::command]
async fn history(profile: String, game: String) -> Result<Vec<UiRevision>, String> {
    blocking(move || {
        let cfg = load_config()?;
        let revs = Client::new(&cfg.serveur).history(&SaveKey { profile, game }).map_err(|e| e.to_string())?;
        Ok(ui_history(&revs, local_time, Local::now().date_naive()))
    })
    .await
}

/// Verrou « partie en cours » pour une opération de l'interface (restauration, synchronisation).
///
/// L'interface prend le verrou sous son propre identifiant (`<appareil>-ui`) : une partie lancée par
/// `ludo`, même sur cet appareil, apparaît donc comme « ailleurs » et l'opération est refusée, au lieu
/// de déplacer la tête du serveur pendant qu'on joue. Serveur injoignable : pas de verrou, l'opération
/// suit son cours (la synchronisation passe hors ligne d'elle-même). Renvoie le verrou à rendre.
struct UiLease<'a> {
    client: &'a Client,
    key: SaveKey,
    device: String,
}

impl<'a> UiLease<'a> {
    fn take(client: &'a Client, key: &SaveKey, appareil: &str) -> Result<Option<Self>, String> {
        let device = format!("{appareil}-ui");
        match client.acquire_lease(key, &device, false) {
            Ok(LeaseOutcome::HeldElsewhere { lease }) => {
                let holder = lease.device.strip_suffix("-ui").unwrap_or(&lease.device);
                Err(format!("En cours sur {}", device_name(holder)))
            }
            Ok(LeaseOutcome::Granted { .. }) => Ok(Some(Self { client, key: key.clone(), device })),
            Err(_) => Ok(None),
        }
    }
}

impl Drop for UiLease<'_> {
    fn drop(&mut self) {
        let _ = self.client.release_lease(&self.key, &self.device);
    }
}

/// Restaure une version : elle devient la nouvelle tête sur le serveur (compare-and-swap contre
/// la tête actuelle), puis remplace la sauvegarde locale si le jeu est configuré sur cet appareil.
/// Conflit (la tête a bougé entre-temps) : erreur, rien n'est remplacé.
#[tauri::command]
async fn restore(profile: String, game: String, rev: u64) -> Result<(), String> {
    blocking(move || {
        let cfg = load_config()?;
        let client = Client::new(&cfg.serveur);
        // Jeu configuré pour ce profil seulement (même règle que `sync`) ; adaptateur impossible à
        // construire : erreur avant de toucher au serveur.
        // Jeu sans section [jeux] : émulateur par défaut de la console (catalogue lu seulement alors).
        let conf = cfg.game_conf(&game, &profile, None).or_else(|| {
            if cfg.jeux.contains_key(&game) { return None; }
            find_game(&client, &game).ok().and_then(|c| cfg.game_conf(&game, &profile, Some(&c)))
        });
        let conf = conf.as_ref();
        let prepared = match conf {
            Some(g) => Some(config::prepare(&cfg, &g.emulateur, &profile, &g.title_id).map_err(|e| e.to_string())?),
            None => None,
        };
        let _guard = JOURNAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut journal = Journal::open(&config::journal_path()).map_err(|e| e.to_string())?;
        let mut engine = Engine {
            client: &client,
            journal: &mut journal,
            device: cfg.appareil.clone(),
            emulator: conf.map(|g| g.emulateur.clone()).unwrap_or_default(),
            playtime_s: 0,
        };
        let key = SaveKey { profile, game };
        let _lease = UiLease::take(&client, &key, &cfg.appareil)?;
        engine.restore(&key, prepared.as_ref().map(|p| p.layout.as_ref()), rev).map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
}

/// Libellé d'un résultat de synchronisation (mêmes mots que `ludo`).
pub fn sync_label(o: &SyncOutcome) -> &'static str {
    match o {
        SyncOutcome::UpToDate => "À jour",
        SyncOutcome::Downloaded { .. } => "À jour – sauvegarde récupérée",
        SyncOutcome::Uploaded { .. } => "À jour – sauvegarde envoyée",
        SyncOutcome::Offline { pending_upload: true } => "Hors ligne – envoi en attente",
        SyncOutcome::Offline { pending_upload: false } => "Hors ligne",
        SyncOutcome::Conflict { .. } => "Conflit de sauvegarde",
        SyncOutcome::LocalMissing => "Dossier de sauvegarde introuvable",
    }
}

/// Résultat de la synchronisation d'un jeu : (identifiant, résultat ou erreur).
type GameSync = (String, Result<SyncOutcome, String>);

/// Jeux à synchroniser pour ce profil : ceux de `[jeux]`, plus ceux déjà joués sur cet appareil
/// (présents au journal) avec l'émulateur par défaut de leur console. Un jeu jamais joué ici n'est
/// pas touché : sa sauvegarde arrive au premier lancement. Catalogue injoignable : `[jeux]` seuls.
fn games_to_sync(cfg: &Config, client: &Client, journal: &Journal, profile: &str) -> Vec<(String, config::GameConf)> {
    let mut out: Vec<(String, config::GameConf)> = cfg.games_for(profile).map(|(id, g)| (id.clone(), g.clone())).collect();
    let prefix = format!("{profile}/");
    let played: Vec<String> = journal.entries().filter_map(|(k, _)| k.strip_prefix(&prefix).map(str::to_string)).collect();
    if played.iter().all(|g| cfg.jeux.contains_key(g)) {
        return out;
    }
    if let Ok(catalog) = client.games() {
        for c in catalog.iter().filter(|c| played.contains(&c.id) && !cfg.jeux.contains_key(&c.id)) {
            if let Some(g) = cfg.game_conf(&c.id, profile, Some(c)) {
                out.push((c.id.clone(), g));
            }
        }
    }
    out
}

/// Synchronise chaque jeu configuré pour ce profil.
fn sync_profile(cfg: &Config, profile: &str) -> Result<Vec<GameSync>, String> {
    let client = Client::new(&cfg.serveur);
    let _guard = JOURNAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut journal = Journal::open(&config::journal_path()).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (id, g) in &games_to_sync(cfg, &client, &journal, profile) {
        let key = SaveKey { profile: profile.into(), game: id.clone() };
        let result = (|| {
            let prepared = config::prepare(cfg, &g.emulateur, profile, &g.title_id).map_err(|e| e.to_string())?;
            let _lease = UiLease::take(&client, &key, &cfg.appareil)?;
            // Aucune partie jouée ici : le temps de jeu cumulé reste celui du serveur.
            let playtime_s = client.head(&key).ok().flatten().map_or(0, |h| h.playtime_s);
            let mut engine = Engine {
                client: &client,
                journal: &mut journal,
                device: cfg.appareil.clone(),
                emulator: g.emulateur.clone(),
                playtime_s,
            };
            engine.sync(&key, prepared.layout.as_ref()).map_err(|e| e.to_string())
        })();
        out.push((id.clone(), result));
    }
    Ok(out)
}

/// Dernière session jouée (`LastSession` de `types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiLast {
    device: String,
    device_kind: &'static str,
    when: String,
}

/// Jeu tel que l'interface l'affiche (`Game` de `types.ts`, champs en camelCase).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiGame {
    id: String,
    title: String,
    short: String,
    console: String,
    tile: String,
    tile_ink: String,
    art: GameArt,
    /// URL absolue de l'image du jeu (serveur configuré + chemin du catalogue), si le serveur en a une.
    #[serde(skip_serializing_if = "Option::is_none")]
    cover: Option<String>,
    /// URL absolue de l'image de fond grand format, si le serveur en a une.
    #[serde(skip_serializing_if = "Option::is_none")]
    background: Option<String>,
    emulator: String,
    install: &'static str,
    sync: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    playtime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last: Option<UiLast>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<String>,
    /// Échec de la dernière synchronisation manuelle de ce jeu (« En cours sur Steam Deck »…).
    #[serde(skip_serializing_if = "Option::is_none")]
    sync_error: Option<String>,
    /// Mises à jour et DLC du jeu (vue « Contenu additionnel » de la fiche).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    content: Vec<UiContent>,
}

/// Une mise à jour ou un DLC tel que la fiche l'affiche (`GameContent` de `types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiContent {
    id: u64,
    role: &'static str,
    name: String,
    size: String,
    installed: bool,
}

/// Nom affiché d'une mise à jour ou d'un DLC : nom du fichier sans crochets ni parenthèses
/// (identifiants, versions, régions), sans le titre du jeu au début.
pub fn content_name(path: &str, role: FileRole, game_title: &str, index: usize) -> String {
    if role == FileRole::Maj {
        return if index == 0 { "Mise à jour".into() } else { format!("Mise à jour {}", index + 1) };
    }
    let stem = path.rsplit('/').next().unwrap_or(path);
    let stem = stem.rsplit_once('.').map_or(stem, |(a, _)| a);
    let (mut out, mut depth) = (String::new(), 0u32);
    for c in stem.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(if c == '_' { ' ' } else { c }),
            _ => {}
        }
    }
    let name = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let simple = |x: &str| x.to_lowercase().replace(['\'', '’', ':'], "");
    let rest = if simple(&name).starts_with(&simple(game_title)) {
        name.chars().skip(game_title.chars().filter(|c| !"'’:".contains(*c)).count()).collect::<String>()
    } else {
        name.clone()
    };
    let rest = rest.trim_matches(|c: char| "-–. :".contains(c)).to_string();
    if rest.is_empty() { "Contenu téléchargeable".into() } else { rest }
}

/// Mises à jour puis DLC d'un jeu, avec leur présence sur l'appareil.
pub fn build_content(g: &CatalogGame, exists: &dyn Fn(&str) -> bool) -> Vec<UiContent> {
    let mut out = Vec::new();
    for role in [FileRole::Maj, FileRole::Dlc] {
        for (i, f) in g.files.iter().filter(|f| f.role == role).enumerate() {
            out.push(UiContent {
                id: f.id,
                role: if role == FileRole::Maj { "maj" } else { "dlc" },
                name: content_name(&f.path, role, &g.title, i),
                size: size_label(f.size),
                installed: exists(&f.path),
            });
        }
    }
    out
}

/// Taille lisible, virgule décimale et unités françaises : « 6,8 Go », « 65,8 Mo ».
pub fn size_label(bytes: u64) -> String {
    for (unit, name) in [(1_000_000_000u64, "Go"), (1_000_000, "Mo")] {
        if bytes >= unit {
            let tenths = (u128::from(bytes) * 10 + u128::from(unit) / 2) / u128::from(unit);
            return format!("{},{} {name}", tenths / 10, tenths % 10);
        }
    }
    if bytes >= 1_000 {
        format!("{} Ko", (bytes + 500) / 1_000)
    } else {
        format!("{bytes} o")
    }
}

/// Émulateur affiché : celui du jeu dans la configuration de l'appareil, sinon l'usage par console.
pub fn emulator_label(configured: Option<&str>, console: &str) -> String {
    match configured {
        Some(id) => {
            let mut c = id.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        }
        None => match console {
            "switch" => "Ryujinx",
            "switch2" => "Lichii",
            "3ds" => "Azahar",
            "ds" => "melonDS",
            "wiiu" => "Cemu",
            "gc" => "Dolphin",
            _ => "Émulateur",
        }
        .into(),
    }
}

/// Installé = au moins un fichier de base présent dans le dossier des ROM de l'appareil.
pub fn install_state(files: &[GameFile], exists: &dyn Fn(&str) -> bool) -> &'static str {
    if files.iter().any(|f| f.role == FileRole::Base && exists(&f.path)) {
        "installe"
    } else {
        "non_installe"
    }
}

/// État de synchronisation d'après le journal local de l'appareil et la présence d'une sauvegarde
/// sur le serveur : une sauvegarde sur le serveur jamais récupérée ici est « à récupérer » (elle se
/// télécharge d'elle-même au lancement).
pub fn sync_state(entry: Option<&Entry>, on_server: bool) -> &'static str {
    match entry {
        Some(e) if e.conflict => "conflit",
        Some(e) if e.pending_upload => "envoi_en_attente",
        Some(_) => "a_jour",
        None if on_server => "a_recuperer",
        None => "aucune",
    }
}

/// Couleur du texte sur une tuile : sombre sur fond clair, blanc sinon (luminance perçue).
pub fn tile_ink(tile: &str) -> &'static str {
    let c = |i: usize| tile.get(i..i + 2).and_then(|h| u32::from_str_radix(h, 16).ok()).unwrap_or(0);
    let luma = 299 * c(1) + 587 * c(3) + 114 * c(5);
    if luma > 150_000 {
        "#0b1f2a"
    } else {
        "#ffffff"
    }
}

/// Ce que `build_library` lit hors du catalogue : tout est injecté, pour tester sans disque ni réseau.
pub struct LibraryEnv<'a> {
    /// Tête de sauvegarde du profil pour un jeu.
    pub saves: &'a [SaveSummary],
    /// Entrée du journal local pour (profil, jeu).
    pub journal: &'a dyn Fn(&str) -> Option<Entry>,
    /// Émulateur configuré sur cet appareil pour un jeu.
    pub emulator: &'a dyn Fn(&str) -> Option<String>,
    /// Le fichier (chemin relatif à la racine des ROM) existe-t-il sur cet appareil ?
    pub exists: &'a dyn Fn(&str) -> bool,
    pub to_local: &'a dyn Fn(u64) -> NaiveDateTime,
    pub today: NaiveDate,
/// Adresse du serveur (préfixe des chemins d'image du catalogue).
pub server: &'a str,
}

/// URL absolue d'une image : adresse du serveur (sans `/` final) + chemin du catalogue.
pub fn cover_url(server: &str, path: &str) -> String {
    format!("{}/{}", server.trim_end_matches('/'), path.trim_start_matches('/'))
}

/// Catalogue du serveur vers les jeux de l'interface, dans l'ordre du catalogue.
pub fn build_library(games: &[CatalogGame], env: &LibraryEnv) -> Vec<UiGame> {
    games
        .iter()
        .map(|g| {
            let save = env.saves.iter().find(|s| s.game == g.id);
            let total: u64 = g.files.iter().map(|f| f.size).sum();
            UiGame {
                id: g.id.clone(),
                title: g.title.clone(),
                short: g.short.clone(),
                console: g.console.clone(),
                tile: g.art.to.clone(),
                tile_ink: tile_ink(&g.art.to).into(),
                art: g.art.clone(),
                cover: g.cover.as_ref().map(|c| cover_url(env.server, c)),
                background: g.background.as_ref().map(|c| cover_url(env.server, c)),
                emulator: emulator_label((env.emulator)(&g.id).as_deref(), &g.console),
                install: install_state(&g.files, env.exists),
                sync: sync_state((env.journal)(&g.id).as_ref(), save.is_some()),
                sync_error: None,
                content: build_content(g, env.exists),
                playtime: save.map(|s| playtime_label(s.playtime_s)),
                last: save.map(|s| UiLast {
                    device: device_name(&s.device),
                    device_kind: device_kind(&s.device),
                    when: when_label((env.to_local)(s.created_at), env.today),
                }),
                size: (total > 0).then(|| size_label(total)),
            }
        })
        .collect()
}

/// Jeux avec une sauvegarde pour le profil, du plus récent au plus ancien, au plus `limit`.
pub fn select_recent(library: &[UiGame], saves: &[SaveSummary], limit: usize) -> Vec<UiGame> {
    let mut ordered: Vec<&SaveSummary> = saves.iter().collect();
    ordered.sort_by(|a, b| b.created_at.cmp(&a.created_at).then_with(|| a.game.cmp(&b.game)));
    ordered.iter().filter_map(|s| library.iter().find(|g| g.id == s.game)).take(limit).cloned().collect()
}

/// Bibliothèque du profil et têtes de sauvegarde, lues sur le serveur (erreur si injoignable) et
/// sur l'appareil (journal, dossier des ROM).
///
/// Catalogue vide (serveur sans dossier de ROM) : erreur, pour que l'interface garde sa
/// bibliothèque de démonstration plutôt que d'afficher un accueil vide.
fn fetch_library(cfg: &Config, profile: &str) -> Result<(Vec<UiGame>, Vec<SaveSummary>), String> {
    let client = Client::new(&cfg.serveur);
    let games = client.games().map_err(|e| e.to_string())?;
    let saves = client.save_summaries(profile).map_err(|e| e.to_string())?;
    let journal = Journal::open(&config::journal_path()).map_err(|e| e.to_string())?;
    let prefix = format!("{profile}/");
    let entries: BTreeMap<String, Entry> =
        journal.entries().filter_map(|(k, e)| k.strip_prefix(&prefix).map(|g| (g.to_string(), e.clone()))).collect();
    // Collection déjà présente sur l'appareil (dossiers déclarés, dossiers de jeux des émulateurs).
    let local = ludo_client::local::LocalIndex::scan(cfg, &ludo_client::detect::Host::current());
    let library = build_library(
        &games,
        &LibraryEnv {
            saves: &saves,
            journal: &|g| entries.get(g).cloned(),
            emulator: &|g| cfg.game_conf(g, profile, games.iter().find(|c| c.id == g)).map(|c| c.emulateur),
            // Dossier de la Ludothèque (tel quel ou converti par le serveur), sinon collection existante.
            exists: &|p| local.locate(cfg.roms.as_deref(), p).is_some(),
            to_local: &local_time,
            today: Local::now().date_naive(),
            server: &cfg.serveur,
        },
    );
    Ok((library, saves))
}

/// Bibliothèque complète du profil (catalogue du serveur, état local).
#[tauri::command]
async fn library(profile: String) -> Result<Vec<UiGame>, String> {
    blocking(move || Ok(fetch_library(&load_config()?, &profile)?.0)).await
}

/// Jeux récents du profil : ceux qui ont une sauvegarde, du plus récent au plus ancien (5 au plus).
#[tauri::command]
async fn recent(profile: String) -> Result<Vec<UiGame>, String> {
    blocking(move || {
        let (library, saves) = fetch_library(&load_config()?, &profile)?;
        Ok(select_recent(&library, &saves, 5))
    })
    .await
}

/// Temps de réponse du serveur en millisecondes (test de connexion des Réglages).
#[tauri::command]
async fn ping() -> Result<u64, String> {
    blocking(|| {
        let cfg = load_config()?;
        let started = std::time::Instant::now();
        Client::new(&cfg.serveur).health().map_err(|e| e.to_string())?;
        Ok(started.elapsed().as_millis() as u64)
    })
    .await
}

/// Une mise à jour ou un DLC disponible (`UpdateItem` de `types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiUpdate {
    /// « <jeu>|<fichier> » : de quoi installer l'élément (`installContent`).
    id: String,
    group: &'static str,
    title: String,
    detail: String,
    console: String,
    tile: String,
    action: &'static str,
}

/// Mises à jour et DLC du catalogue pas encore installés, pour les jeux installés sur l'appareil.
pub fn build_updates(library: &[UiGame]) -> Vec<UiUpdate> {
    let mut out = Vec::new();
    for g in library.iter().filter(|g| g.install != "non_installe") {
        for c in g.content.iter().filter(|c| !c.installed) {
            let maj = c.role == "maj";
            out.push(UiUpdate {
                id: format!("{}|{}", g.id, c.id),
                group: "jeux",
                title: g.title.clone(),
                detail: if maj { format!("{} · {}", c.name, c.size) } else { format!("DLC disponible : {} · {}", c.name, c.size) },
                console: g.console.clone(),
                tile: format!("linear-gradient(160deg, {}, {})", g.art.from, g.art.to),
                action: if maj { "Mettre à jour" } else { "Installer" },
            });
        }
    }
    out
}

/// Mises à jour disponibles pour cet appareil (catalogue du serveur comparé aux ROM installées).
#[tauri::command]
async fn updates() -> Result<Vec<UiUpdate>, String> {
    blocking(|| {
        let cfg = load_config()?;
        let profile = cfg.profil_defaut.clone().unwrap_or_default();
        Ok(build_updates(&fetch_library(&cfg, &profile)?.0))
    })
    .await
}

/// Synchronisation manuelle de tous les jeux configurés pour ce profil, puis bibliothèque à jour.
///
/// Chaque jeu est synchronisé (verrou de l'interface compris) ; son résultat se lit ensuite dans le
/// journal (conflit, envoi en attente, à jour). Un échec propre à un jeu (adaptateur, dossier de
/// sauvegarde) n'arrête pas les autres : il est écrit dans la sortie d'erreur. Serveur injoignable :
/// erreur, l'interface garde alors ses données de démonstration.
#[tauri::command]
async fn sync(profile: String) -> Result<Vec<UiGame>, String> {
    blocking(move || {
        let cfg = load_config()?;
        let mut failures = std::collections::HashMap::new();
        for (id, result) in sync_profile(&cfg, &profile)? {
            match result {
                Ok(o) => eprintln!("sync {id} : {}", sync_label(&o)),
                Err(e) => {
                    eprintln!("sync {id} : échec : {e}");
                    failures.insert(id, e);
                }
            }
        }
        // Les échecs par jeu remontent à l'interface (« Synchronisation incomplète »).
        let mut games = fetch_library(&cfg, &profile)?.0;
        for g in &mut games {
            g.sync_error = failures.remove(&g.id);
        }
        Ok(games)
    })
    .await
}

/// Événement de progression d'une installation (`ludo://progression`).
pub const EVENT_PROGRESSION: &str = "ludo://progression";

/// Charge de l'événement : `done` et `total` cumulent tous les fichiers de l'opération en cours ;
/// `file` est l'identifiant du fichier en train d'arriver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Progression {
    game: String,
    file: u64,
    done: u64,
    total: u64,
    /// Présent tant que le serveur prépare le fichier (conversion NSZ → NSP) : l'interface affiche
    /// « Préparation sur le serveur… ». Absent = téléchargement normal (compatible avec l'ancienne charge).
    #[serde(skip_serializing_if = "Option::is_none")]
    prepare: Option<Preparation>,
}

/// Avancement de la préparation d'un fichier sur le serveur.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preparation {
    queued: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    percent: Option<u8>,
    elapsed_s: u64,
}

/// Opérations en cours : jeu → drapeau d'annulation. Une seule opération à la fois par jeu.
pub type Occupes = Mutex<BTreeMap<String, Arc<AtomicBool>>>;

static EN_COURS: Occupes = Mutex::new(BTreeMap::new());

/// Réservation d'un jeu ; rendue à la destruction (même en cas d'erreur ou de panique).
#[derive(Debug)]
pub struct JeuOccupe<'a> {
    set: &'a Occupes,
    game: String,
    /// Passe à `true` quand l'interface demande l'annulation (`cancelInstall`).
    pub cancel: Arc<AtomicBool>,
}

impl<'a> JeuOccupe<'a> {
    /// Réserve le jeu, ou refuse si une opération y est déjà en cours.
    pub fn prendre(set: &'a Occupes, game: &str) -> Result<Self, String> {
        let mut g = set.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if g.contains_key(game) {
            return Err("Une opération sur ce jeu est déjà en cours".into());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        g.insert(game.to_string(), cancel.clone());
        Ok(Self { set, game: game.to_string(), cancel })
    }
}

impl Drop for JeuOccupe<'_> {
    fn drop(&mut self) {
        self.set.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&self.game);
    }
}

/// Demande l'annulation de l'opération en cours sur ce jeu ; `false` si aucune.
pub fn annuler(set: &Occupes, game: &str) -> bool {
    match set.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(game) {
        Some(flag) => {
            flag.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

/// Fichiers à installer : sans `only`, le jeu de base puis ses mises à jour (les DLC restent un choix
/// explicite) ; avec `only`, ce seul fichier, qui doit être une mise à jour ou un DLC du jeu.
pub fn install_plan(game: &CatalogGame, only: Option<u64>) -> Result<Vec<GameFile>, String> {
    match only {
        Some(id) => {
            let f = game.files.iter().find(|f| f.id == id).ok_or("Contenu introuvable sur le serveur")?;
            if f.role == FileRole::Base {
                return Err("Ce fichier est le jeu lui-même, pas un contenu additionnel".into());
            }
            Ok(vec![f.clone()])
        }
        None => {
            let plan: Vec<GameFile> = [FileRole::Base, FileRole::Maj]
                .into_iter()
                .flat_map(|role| game.files.iter().filter(move |f| f.role == role).cloned())
                .collect();
            if plan.iter().all(|f| f.role != FileRole::Base) {
                return Err("Aucun fichier à installer pour ce jeu sur le serveur".into());
            }
            Ok(plan)
        }
    }
}

fn roms_dir(cfg: &Config) -> Result<std::path::PathBuf, String> {
    cfg.roms.clone().ok_or_else(|| "Dossier des jeux non configuré sur cet appareil (option roms)".to_string())
}

fn find_game(client: &Client, id: &str) -> Result<CatalogGame, String> {
    client.games().map_err(|e| e.to_string())?.into_iter().find(|g| g.id == id).ok_or_else(|| "Jeu introuvable sur le serveur".to_string())
}

/// Formats que lit l'émulateur configuré pour ce jeu sur cet appareil (fiche) ; vide si le jeu n'est
/// pas configuré ou la fiche illisible : le serveur envoie alors le fichier tel quel.
fn accepted_formats(cfg: &Config, game: &CatalogGame) -> Vec<String> {
    cfg.jeux
        .get(&game.id)
        .map(|g| g.emulateur.clone())
        .or_else(|| cfg.default_emulator(&game.console))
        .and_then(|e| cfg.fiche(&e).ok())
        .map(|f| f.formats)
        .unwrap_or_default()
}

/// Installe les fichiers du plan l'un après l'autre en émettant la progression cumulée.
///
/// Un fichier converti par le serveur (NSZ → NSP) change de taille : le total cumulé remplace la
/// taille du catalogue par la taille réelle dès qu'elle est connue.
fn run_install(app: &tauri::AppHandle, cfg: &Config, game: &CatalogGame, plan: &[GameFile], cancel: Arc<AtomicBool>) -> Result<(), String> {
    let roms = roms_dir(cfg)?;
    let client = Client::new(&cfg.serveur);
    let formats = accepted_formats(cfg, game);
    let total: Cell<u64> = Cell::new(plan.iter().map(|f| f.size).sum());
    let before = Cell::new(0u64);
    let file_id = Cell::new(plan.first().map_or(0, |f| f.id));
    let emit = |done: u64, prepare: Option<Preparation>| {
        let _ = app.emit(EVENT_PROGRESSION, Progression { game: game.id.clone(), file: file_id.get(), done: before.get() + done, total: total.get(), prepare });
    };
    let installer = Installer::new(&client, &roms).with_cancel(cancel).with_formats(formats).with_prepare(|p| {
        emit(0, Some(Preparation { queued: p.state == "queued", percent: p.percent, elapsed_s: p.elapsed_s }));
    });
    for f in plan {
        file_id.set(f.id);
        // Taille réellement attendue de ce fichier (celle du catalogue tant qu'on ne sait pas mieux).
        let known = Cell::new(f.size);
        let mut report = |done: u64, file_total: u64| {
            if file_total != known.get() {
                total.set(total.get() - known.get() + file_total);
                known.set(file_total);
            }
            emit(done, None);
        };
        installer.install(&game.id, f, &mut report).map_err(|e| e.to_string())?;
        before.set(before.get() + known.get());
    }
    emit(total.get() - before.get(), None);
    Ok(())
}

/// Installe un jeu : fichiers de base et mises à jour (les DLC se choisissent un par un).
#[tauri::command(rename = "installGame")]
async fn install_game(app: tauri::AppHandle, game: String) -> Result<(), String> {
    blocking(move || {
        let busy = JeuOccupe::prendre(&EN_COURS, &game)?;
        let cfg = load_config()?;
        let g = find_game(&Client::new(&cfg.serveur), &game)?;
        run_install(&app, &cfg, &g, &install_plan(&g, None)?, busy.cancel.clone())
    })
    .await
}

/// Installe une mise à jour ou un DLC (identifiant de fichier du catalogue).
#[tauri::command(rename = "installContent")]
async fn install_content(app: tauri::AppHandle, game: String, content: u64) -> Result<(), String> {
    blocking(move || {
        let busy = JeuOccupe::prendre(&EN_COURS, &game)?;
        let cfg = load_config()?;
        let g = find_game(&Client::new(&cfg.serveur), &game)?;
        run_install(&app, &cfg, &g, &install_plan(&g, Some(content))?, busy.cancel.clone())
    })
    .await
}

/// Annule l'installation en cours d'un jeu (sans effet s'il n'y en a pas). Le téléchargement d'un
/// fichier garde son `.part` pour reprendre ; l'extraction d'un dossier est supprimée.
#[tauri::command(rename = "cancelInstall")]
fn cancel_install(game: String) {
    annuler(&EN_COURS, &game);
}

/// Désinstalle un jeu : supprime de l'appareil ses fichiers de ROM (base, mises à jour, DLC).
/// Les sauvegardes ne sont jamais touchées.
#[tauri::command(rename = "uninstallGame")]
async fn uninstall_game(game: String) -> Result<(), String> {
    blocking(move || {
        let _busy = JeuOccupe::prendre(&EN_COURS, &game)?;
        let cfg = load_config()?;
        let roms = roms_dir(&cfg)?;
        let g = find_game(&Client::new(&cfg.serveur), &game)?;
        // Jeu de la collection existante (hors dossier de la Ludothèque) : jamais supprimé d'ici.
        let in_roms = g.files.iter().any(|f| roms.join(&f.path).exists() || install::converted_rel(&f.path).is_some_and(|c| roms.join(c).exists()));
        if !in_roms {
            let local = ludo_client::local::LocalIndex::scan(&cfg, &ludo_client::detect::Host::current());
            if let Some(p) = g.files.iter().find_map(|f| local.find(&g.console, &f.path)) {
                return Err(format!("Jeu de la collection de l'appareil ({}) : à retirer depuis ce dossier, la Ludothèque n'y supprime rien", p.parent().unwrap_or(p).display()));
            }
        }
        install::uninstall(&roms, &g.files).map(|_| ()).map_err(|e| e.to_string())
    })
    .await
}

/// Événement des étapes d'un lancement (`ludo://passage`).
pub const EVENT_PASSAGE: &str = "ludo://passage";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PassageEvent {
    game: String,
    step: &'static str,
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

/// Relaie les étapes du lancement à l'interface et installe le jeu s'il manque.
struct PassageObserver {
    app: tauri::AppHandle,
    cfg: Config,
    game: String,
    cancel: Arc<AtomicBool>,
}

impl Observer for PassageObserver {
    fn step(&mut self, step: Step, state: StepState, detail: Option<String>) {
        let _ = self.app.emit(EVENT_PASSAGE, PassageEvent { game: self.game.clone(), step: step.id(), state: state.id(), detail });
    }

    fn install(&mut self, game: &CatalogGame) -> Result<(), String> {
        run_install(&self.app, &self.cfg, game, &install_plan(game, None)?, self.cancel.clone())
    }
}

/// Un côté du conflit tel que l'écran « Conflit de sauvegarde » l'affiche (`ConflictSide` de `types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiSide {
    device: String,
    device_kind: &'static str,
    when: String,
    playtime: String,
    delta: String,
    /// Seulement si mesurable (temps de jeu supérieur des deux côtés) : jamais ici, le temps de
    /// jeu d'une copie locale non synchronisée n'est connu de personne.
    #[serde(skip_serializing_if = "Option::is_none")]
    ahead: Option<bool>,
}

/// Les deux versions en conflit : la copie de cet appareil et la tête du serveur.
///
/// Le temps de jeu local n'est pas mesurable (il n'est enregistré qu'à l'envoi) : la carte locale
/// affiche celui de la dernière version commune, et seul le serveur indique ce qu'il a gagné depuis.
pub fn ui_conflict(info: &ConflictInfo, local_device: &str, to_local: impl Fn(u64) -> NaiveDateTime, today: NaiveDate) -> (UiSide, UiSide) {
    let base_playtime = info.base.as_ref().map(|b| b.playtime_s);
    let local = UiSide {
        device: device_name(local_device),
        device_kind: device_kind(local_device),
        when: "Sur cet appareil".into(),
        playtime: base_playtime.map_or_else(|| "–".into(), playtime_label),
        delta: "Modifiée depuis la dernière synchronisation".into(),
        ahead: None,
    };
    let s = &info.server;
    let server = UiSide {
        device: device_name(&s.device),
        device_kind: device_kind(&s.device),
        when: when_label(to_local(s.created_at), today),
        playtime: playtime_label(s.playtime_s),
        delta: match base_playtime {
            Some(b) => format!("+{} depuis la base", playtime_label(s.playtime_s.saturating_sub(b))),
            None => "Sans version commune avec cet appareil".into(),
        },
        ahead: None,
    };
    (local, server)
}

/// Résultat de `launchGame` (`LaunchResult` de `api.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "result", rename_all = "lowercase")]
pub enum UiLaunch {
    /// Partie jouée ; `detail` : « Hors ligne : envoi en attente »…
    Termine {
        playtime: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        /// L'émulateur s'est arrêté avec une erreur (la sauvegarde est quand même synchronisée).
        #[serde(rename = "emulatorError", skip_serializing_if = "std::ops::Not::not")]
        emulator_error: bool,
    },
    /// Rien n'est lancé tant que le joueur n'a pas tranché (`played` : le conflit vient de l'envoi
    /// après la partie, le jeu ne doit pas être relancé).
    Conflit { local: UiSide, server: UiSide, played: bool, rev: u64 },
}

pub fn launch_error(e: LaunchError) -> String {
    match e {
        LaunchError::HeldElsewhere(device) => format!("En cours sur {}", device_name(launch::holder_device(&device))),
        LaunchError::Failed(msg) | LaunchError::SendFailed { message: msg, .. } => msg,
    }
}

/// Envois de sauvegarde en échec après une partie jouée : jeu → (temps de jeu, émulateur sans erreur).
/// « Réessayer l'envoi » renvoie la sauvegarde sans relancer le jeu.
static ENVOIS: Mutex<BTreeMap<String, (u64, bool)>> = Mutex::new(BTreeMap::new());

fn ui_conflit(info: &ConflictInfo, cfg: &Config) -> UiLaunch {
    let (local, server) = ui_conflict(info, &cfg.appareil, local_time, Local::now().date_naive());
    UiLaunch::Conflit { local, server, played: info.played, rev: info.server.rev }
}

/// Lance un jeu pour de vrai (comme `ludo run`) : installation si besoin, sauvegarde, émulateur,
/// envoi à la fermeture. Les étapes partent sur `ludo://passage`. Conflit : rien n'est lancé.
#[tauri::command(rename = "launchGame")]
async fn launch_game(app: tauri::AppHandle, profile: String, game: String) -> Result<UiLaunch, String> {
    blocking(move || {
        let busy = JeuOccupe::prendre(&EN_COURS, &game)?;
        let cfg = load_config()?;
        let key = SaveKey { profile, game: game.clone() };
        let mut obs = PassageObserver { app, cfg: cfg.clone(), game, cancel: busy.cancel.clone() };
        let journal_path = config::journal_path();
        let env = launch::Env { cfg: &cfg, journal_path: &journal_path, journal_lock: &JOURNAL };
        let outcome = match launch::launch(&env, &key, &mut obs) {
            Ok(o) => o,
            Err(LaunchError::SendFailed { message, playtime_s, exit_ok }) => {
                ENVOIS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(obs.game.clone(), (playtime_s, exit_ok));
                return Err(message);
            }
            Err(e) => return Err(launch_error(e)),
        };
        ENVOIS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&obs.game);
        match outcome {
            LaunchOutcome::Played { after, playtime_s, exit_ok, .. } => Ok(termine(&after, playtime_s, exit_ok)),
            LaunchOutcome::Conflict(info) => Ok(ui_conflit(&info, &cfg)),
        }
    })
    .await
}

fn termine(after: &SyncOutcome, playtime_s: u64, exit_ok: bool) -> UiLaunch {
    UiLaunch::Termine {
        playtime: playtime_label(playtime_s),
        detail: match after {
            SyncOutcome::Offline { pending_upload: true } => Some("Hors ligne : envoi en attente".into()),
            SyncOutcome::LocalMissing => Some("Dossier de sauvegarde introuvable".into()),
            _ => None,
        },
        emulator_error: !exit_ok,
    }
}

/// Renvoie la sauvegarde d'une partie déjà jouée dont l'envoi a échoué, sans relancer le jeu.
#[tauri::command(rename = "retrySend")]
async fn retry_send(profile: String, game: String) -> Result<UiLaunch, String> {
    blocking(move || {
        let _busy = JeuOccupe::prendre(&EN_COURS, &game)?;
        let (playtime_s, exit_ok) = ENVOIS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&game).copied().ok_or("Aucun envoi en attente pour ce jeu")?;
        let cfg = load_config()?;
        let journal_path = config::journal_path();
        let env = launch::Env { cfg: &cfg, journal_path: &journal_path, journal_lock: &JOURNAL };
        match launch::send(&env, &SaveKey { profile, game: game.clone() }, playtime_s).map_err(launch_error)? {
            Settled::Done(SyncOutcome::Conflict { .. }) => Err("Conflit de sauvegarde".into()),
            Settled::Done(after) => {
                ENVOIS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&game);
                Ok(termine(&after, playtime_s, exit_ok))
            }
            Settled::Conflict(info) => {
                ENVOIS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&game);
                Ok(ui_conflit(&info, &cfg))
            }
        }
    })
    .await
}

/// Résultat de `resolveConflict` : choix appliqué, ou conflit à retrancher (la sauvegarde du serveur
/// a changé depuis l'affichage).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "result", rename_all = "lowercase")]
#[allow(clippy::large_enum_variant)] // valeur éphémère, renvoyée une fois à l'interface
pub enum UiResolve {
    Fait,
    Conflit { local: UiSide, server: UiSide, played: bool, rev: u64 },
}

/// Applique le choix du joueur (« local » ou « serveur ») sur la version du serveur affichée
/// (`rev`) ; l'autre version reste dans l'historique du serveur (écartée). Si le serveur a bougé
/// entre-temps : rien n'est appliqué, le conflit à jour est renvoyé.
#[tauri::command(rename = "resolveConflict")]
async fn resolve_conflict(profile: String, game: String, choice: String, rev: u64) -> Result<UiResolve, String> {
    blocking(move || {
        let resolution = match choice.as_str() {
            "local" => Resolution::KeepLocal,
            "serveur" => Resolution::KeepServer,
            _ => return Err("Choix inconnu".to_string()),
        };
        let _busy = JeuOccupe::prendre(&EN_COURS, &game)?;
        let cfg = load_config()?;
        let journal_path = config::journal_path();
        let env = launch::Env { cfg: &cfg, journal_path: &journal_path, journal_lock: &JOURNAL };
        match launch::resolve(&env, &SaveKey { profile, game }, resolution, rev).map_err(launch_error)? {
            Settled::Conflict(info) => match ui_conflit(&info, &cfg) {
                UiLaunch::Conflit { local, server, played, rev } => Ok(UiResolve::Conflit { local, server, played, rev }),
                UiLaunch::Termine { .. } => unreachable!("ui_conflit renvoie toujours un conflit"),
            },
            Settled::Done(SyncOutcome::Offline { .. }) => Err("Serveur injoignable : le choix n'a pas été appliqué".into()),
            Settled::Done(SyncOutcome::LocalMissing) => Err("Dossier de sauvegarde introuvable : rien n'a été modifié".into()),
            Settled::Done(_) => Ok(UiResolve::Fait),
        }
    })
    .await
}

/// Événement envoyé à l'interface quand la fermeture de la fenêtre est refusée (partie en cours).
pub const EVENT_FERMETURE: &str = "ludo://fermeture-refusee";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    rumble::init();
    tauri::Builder::default()
        // Partie en cours : la fenêtre ne se ferme pas (l'émulateur serait orphelin et la
        // sauvegarde jamais envoyée) ; l'interface affiche « Partie en cours : fermer le jeu d'abord ».
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if launch::game_running() {
                    api.prevent_close();
                    let _ = window.app_handle().emit(EVENT_FERMETURE, ());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![device, ping, updates, rumble::rumble, profiles, save_profile, history, restore, sync, library, recent, install_game, install_content, uninstall_game, cancel_install, launch_game, retry_send, resolve_conflict, emulators])
        .build(tauri::generate_context!())
        .expect("échec du démarrage de l'interface")
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested { api, code: None, .. } if launch::game_running() => {
                api.prevent_exit();
                let _ = app.emit(EVENT_FERMETURE, ());
            }
            // Arrêt voulu malgré la partie : l'émulateur est terminé proprement, la sauvegarde reste
            // locale et sera synchronisée au prochain lancement.
            tauri::RunEvent::Exit => launch::terminate_running(),
            _ => {}
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflit_pour_l_interface() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        let server = ludo_proto::SaveRevision { playtime_s: 3 * 3600 + 23 * 60, ..rev(4, "steam-deck", false) };
        let base = ludo_proto::SaveRevision { playtime_s: 3 * 3600, ..rev(3, "pc", false) };
        let info = ConflictInfo { server: server.clone(), base: Some(base), played: false };
        let (local, srv) = ui_conflict(&info, "pc", |_| at(2026, 11, 15, 14, 2), today);
        assert_eq!((local.device.as_str(), local.playtime.as_str(), local.ahead), ("PC", "3 h 00", None));
        assert_eq!(
            serde_json::to_value(&srv).unwrap(),
            serde_json::json!({ "device": "Steam Deck", "deviceKind": "deck", "when": "Aujourd'hui 14:02", "playtime": "3 h 23", "delta": "+0 h 23 depuis la base" })
        );
        // Sans version commune : pas de delta inventé.
        let (local, srv) = ui_conflict(&ConflictInfo { server, base: None, played: true }, "pc", |_| at(2026, 11, 15, 14, 2), today);
        assert_eq!(local.playtime, "–");
        assert_eq!(srv.delta, "Sans version commune avec cet appareil");
    }

    #[test]
    fn erreurs_de_lancement_en_francais() {
        assert_eq!(launch_error(LaunchError::HeldElsewhere("steam-deck".into())), "En cours sur Steam Deck");
        assert_eq!(launch_error(LaunchError::HeldElsewhere("pc-ui".into())), "En cours sur PC");
        assert_eq!(launch_error(LaunchError::HeldElsewhere("steam-deck-app".into())), "En cours sur Steam Deck");
        assert_eq!(launch_error(LaunchError::Failed("Jeu introuvable".into())), "Jeu introuvable");
    }

    #[test]
    fn resultat_du_lancement_serialise() {
        let r = UiLaunch::Termine { playtime: "1 h 02".into(), detail: None, emulator_error: false };
        assert_eq!(serde_json::to_value(&r).unwrap(), serde_json::json!({ "result": "termine", "playtime": "1 h 02" }));
        let r = termine(&SyncOutcome::UpToDate, 3720, false);
        assert_eq!(serde_json::to_value(&r).unwrap(), serde_json::json!({ "result": "termine", "playtime": "1 h 02", "emulatorError": true }));
    }

    #[test]
    fn appareils_reconnus() {
        assert_eq!((device_kind("steam-deck"), device_name("steam-deck").as_str()), ("deck", "Steam Deck"));
        assert_eq!(device_kind("thor"), "thor");
        assert_eq!(device_kind("pc"), "pc");
    }

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap()
    }

    #[test]
    fn dates_lisibles_en_francais() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        assert_eq!(when_label(at(2026, 11, 15, 20, 15), today), "Aujourd'hui 20:15");
        assert_eq!(when_label(at(2026, 11, 14, 21, 58), today), "Hier 21:58");
        assert_eq!(when_label(at(2026, 11, 7, 16, 12), today), "Sam. 7 nov. 16:12");
        assert_eq!(when_label(at(2026, 11, 12, 9, 5), today), "Jeu. 12 nov. 09:05");
        assert_eq!(when_label(at(2026, 2, 2, 7, 0), today), "Lun. 2 févr. 07:00");
        assert_eq!(when_label(at(2025, 12, 31, 23, 59), today), "Mer. 31 déc. 2025 23:59");
        // Changement d'année : « Hier » reste juste le 1er janvier.
        let jan1 = NaiveDate::from_ymd_opt(2027, 1, 1).unwrap();
        assert_eq!(when_label(at(2026, 12, 31, 22, 0), jan1), "Hier 22:00");
    }

    #[test]
    fn temps_de_jeu_lisible() {
        assert_eq!(playtime_label(0), "0 h 00");
        assert_eq!(playtime_label(45_600), "12 h 40");
        assert_eq!(playtime_label(21_720 + 59), "6 h 02");
    }

    fn rev(rev: u64, device: &str, discarded: bool) -> ludo_proto::SaveRevision {
        ludo_proto::SaveRevision {
            rev,
            parent: rev.saturating_sub(1),
            hash: "h".into(),
            size: 1,
            device: device.into(),
            emulator: "ryujinx".into(),
            playtime_s: rev * 3600,
            created_at: rev,
            discarded,
        }
    }

    #[test]
    fn historique_pour_l_interface() {
        // Plus récent d'abord, comme le serveur : la 4 est écartée, la tête est la 3.
        let revs = [rev(4, "pc", true), rev(3, "steam-deck", false), rev(2, "thor", false)];
        let today = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        let to_local = |ts: u64| at(2026, 11, 15, 10, ts as u32);
        let ui = ui_history(&revs, to_local, today);
        assert_eq!(
            serde_json::to_value(&ui).unwrap(),
            serde_json::json!([
                { "rev": 4, "device": "PC", "deviceKind": "pc", "when": "Aujourd'hui 10:04", "playtime": "4 h 00", "discarded": true },
                { "rev": 3, "device": "Steam Deck", "deviceKind": "deck", "when": "Aujourd'hui 10:03", "playtime": "3 h 00", "current": true },
                { "rev": 2, "device": "Ayn Thor", "deviceKind": "thor", "when": "Aujourd'hui 10:02", "playtime": "2 h 00" }
            ])
        );
        assert!(ui_history(&[], to_local, today).is_empty());
    }

    fn file(id: u64, role: FileRole, path: &str, size: u64) -> GameFile {
        GameFile { id, role, path: path.into(), format: "nsp".into(), size, mtime: 0 }
    }

    fn catalog_game(id: &str, console: &str, files: Vec<GameFile>) -> CatalogGame {
        CatalogGame {
            id: id.into(),
            console: console.into(),
            title: "Luigi's Mansion 3".into(),
            short: "Luigi's\nMansion 3".into(),
            title_id: None,
            art: GameArt { motif: "manoir".into(), from: "#22e08f".into(), to: "#0a5a78".into(), ink: None },
            cover: None,
            background: None,
            catalogued: true,
            files,
        }
    }

    #[test]
    fn url_absolue_de_l_image() {
        assert_eq!(cover_url("https://ludo.test/", "/api/v1/games/x/cover?v=1"), "https://ludo.test/api/v1/games/x/cover?v=1");
        let mut g = catalog_game("switch-lm3", "switch", vec![]);
        g.cover = Some("/api/v1/games/switch-lm3/cover?v=a-b".into());
        let entries = BTreeMap::new();
        let lib = build_library(&[g, catalog_game("ds-x", "ds", vec![])], &env(&[], &entries, &[]));
        assert_eq!(lib[0].cover.as_deref(), Some("https://ludo.test/api/v1/games/switch-lm3/cover?v=a-b"));
        assert_eq!(lib[1].cover, None);
    }

    fn save(game: &str, device: &str, playtime_s: u64, created_at: u64) -> SaveSummary {
        SaveSummary { game: game.into(), rev: 1, playtime_s, device: device.into(), emulator: "ryujinx".into(), created_at }
    }

    #[test]
    fn tailles_lisibles() {
        assert_eq!(size_label(6_800_000_000), "6,8 Go");
        assert_eq!(size_label(65_800_000), "65,8 Mo");
        assert_eq!(size_label(13_249_000_000), "13,2 Go");
        assert_eq!(size_label(12_300), "12 Ko");
        assert_eq!(size_label(999), "999 o");
    }

    #[test]
    fn etats_d_installation_et_de_synchronisation() {
        let files = [file(1, FileRole::Maj, "switch/MAJ-DLC/m.nsp", 1), file(2, FileRole::Base, "switch/a.nsp", 1)];
        // Seule la mise à jour est là : pas installé. Le fichier de base est là : installé.
        assert_eq!(install_state(&files, &|p| p.contains("MAJ-DLC")), "non_installe");
        assert_eq!(install_state(&files, &|p| p == "switch/a.nsp"), "installe");
        assert_eq!(install_state(&[], &|_| true), "non_installe");

        let entry = |conflict, pending_upload| Entry { base_rev: 1, base_hash: None, pending_upload, conflict };
        assert_eq!(sync_state(Some(&entry(true, true)), true), "conflit");
        assert_eq!(sync_state(Some(&entry(false, true)), true), "envoi_en_attente");
        assert_eq!(sync_state(Some(&entry(false, false)), true), "a_jour");
        assert_eq!(sync_state(None, true), "a_recuperer");
        assert_eq!(sync_state(None, false), "aucune");
        assert_eq!(content_name("switch/MAJ-DLC/Luigis Mansion 3 [0100DCA0064A6800][v1].nsp", FileRole::Maj, "Luigi's Mansion 3", 0), "Mise à jour");
        assert_eq!(content_name("switch/MAJ-DLC/Luigis Mansion 3 - Pack multijoueur 1 [0100DCA0064A7001][v2].nsp", FileRole::Dlc, "Luigi's Mansion 3", 0), "Pack multijoueur 1");
        assert_eq!(content_name("switch/MAJ-DLC/Luigis Mansion 3 [0100DCA0064A7002].nsp", FileRole::Dlc, "Luigi's Mansion 3", 1), "Contenu téléchargeable");
    }

    #[test]
    fn emulateurs_par_defaut_et_configures() {
        assert_eq!(emulator_label(Some("ryujinx"), "switch"), "Ryujinx");
        assert_eq!(emulator_label(Some("eden"), "switch"), "Eden");
        assert_eq!(emulator_label(None, "3ds"), "Azahar");
        assert_eq!(emulator_label(None, "wiiu"), "Cemu");
    }

    #[test]
    fn encre_des_tuiles() {
        assert_eq!(tile_ink("#0a5a78"), "#ffffff");
        assert_eq!(tile_ink("#9ae6ff"), "#0b1f2a");
    }

    fn env<'a>(saves: &'a [SaveSummary], entries: &'a BTreeMap<String, Entry>, local: &'a [&'a str]) -> LibraryEnv<'a> {
        LibraryEnv {
            saves,
            journal: Box::leak(Box::new(move |g: &str| entries.get(g).cloned())),
            emulator: Box::leak(Box::new(|g: &str| (g == "switch-lm3").then(|| "eden".to_string()))),
            exists: Box::leak(Box::new(move |p: &str| local.contains(&p))),
            to_local: Box::leak(Box::new(|ts: u64| at(2026, 11, 15, 20, 15) - chrono::Duration::days(ts as i64))),
            today: NaiveDate::from_ymd_opt(2026, 11, 15).unwrap(),
            server: "https://ludo.test/",
        }
    }

    #[test]
    fn mises_a_jour_des_jeux_installes_seulement() {
        let games = [
            catalog_game("switch-lm3", "switch", vec![file(1, FileRole::Base, "switch/lm3.nsp", 10), file(2, FileRole::Maj, "switch/MAJ-DLC/lm3 [v1].nsp", 5), file(3, FileRole::Dlc, "switch/MAJ-DLC/lm3 Pack.nsp", 5)]),
            catalog_game("switch-mk8", "switch", vec![file(4, FileRole::Base, "switch/mk8.nsp", 10), file(5, FileRole::Maj, "switch/MAJ-DLC/mk8 [v1].nsp", 5)]),
        ];
        let entries = BTreeMap::new();
        // LM3 installé avec sa mise à jour, MK8 pas installé : seul le DLC de LM3 est proposé.
        let lib = build_library(&games, &env(&[], &entries, &["switch/lm3.nsp", "switch/MAJ-DLC/lm3 [v1].nsp"]));
        let u = build_updates(&lib);
        assert_eq!(u.len(), 1);
        assert_eq!((u[0].id.as_str(), u[0].action), ("switch-lm3|3", "Installer"));
        assert!(u[0].tile.starts_with("linear-gradient("));
    }

    #[test]
    fn jeu_de_l_interface_a_la_forme_attendue() {
        let games = [catalog_game(
            "switch-lm3",
            "switch",
            vec![file(1, FileRole::Base, "switch/lm3.nsp", 6_000_000_000), file(2, FileRole::Dlc, "switch/MAJ-DLC/d.nsp", 800_000_000)],
        )];
        let saves = [save("switch-lm3", "steam-deck", 45_600, 0)];
        let entries = BTreeMap::from([("switch-lm3".to_string(), Entry { base_rev: 1, base_hash: None, pending_upload: true, conflict: false })]);
        let lib = build_library(&games, &env(&saves, &entries, &["switch/lm3.nsp"]));
        assert_eq!(
            serde_json::to_value(&lib).unwrap(),
            serde_json::json!([{
                "id": "switch-lm3", "title": "Luigi's Mansion 3", "short": "Luigi's\nMansion 3", "console": "switch",
                "tile": "#0a5a78", "tileInk": "#ffffff",
                "art": { "motif": "manoir", "from": "#22e08f", "to": "#0a5a78" },
                "content": [{ "id": 2, "role": "dlc", "name": "d", "size": "800,0 Mo", "installed": false }],
                "emulator": "Eden", "install": "installe", "sync": "envoi_en_attente",
                "playtime": "12 h 40",
                "last": { "device": "Steam Deck", "deviceKind": "deck", "when": "Aujourd'hui 20:15" },
                "size": "6,8 Go"
            }])
        );
    }

    #[test]
    fn jeu_sans_sauvegarde_ni_fichier_local() {
        let games = [catalog_game("ds-x", "ds", vec![file(1, FileRole::Base, "ds/x.nds", 65_800_000)])];
        let lib = build_library(&games, &env(&[], &BTreeMap::new(), &[]));
        let v = serde_json::to_value(&lib[0]).unwrap();
        assert_eq!((v["install"].as_str(), v["sync"].as_str(), v["emulator"].as_str()), (Some("non_installe"), Some("aucune"), Some("melonDS")));
        assert!(v.get("playtime").is_none() && v.get("last").is_none());
        assert_eq!(v["size"], "65,8 Mo");
    }

    #[test]
    fn jeux_recents() {
        let games: Vec<_> = (1..=7).map(|i| catalog_game(&format!("g{i}"), "switch", vec![])).collect();
        // g1..g7 ont une sauvegarde (g1 la plus ancienne) ; « inconnu » n'est pas au catalogue ; g8 sans sauvegarde.
        let mut saves: Vec<_> = (1..=7).map(|i| save(&format!("g{i}"), "pc", 60, 100 + i)).collect();
        saves.push(save("inconnu", "pc", 60, 999));
        let entries = BTreeMap::new();
        let lib = build_library(&games, &env(&saves, &entries, &[]));
        let ids: Vec<_> = select_recent(&lib, &saves, 5).into_iter().map(|g| g.id).collect();
        assert_eq!(ids, ["g7", "g6", "g5", "g4", "g3"]);
        assert!(select_recent(&lib, &[], 5).is_empty());
    }

    fn plan_game() -> CatalogGame {
        catalog_game(
            "switch-lm3",
            "switch",
            vec![
                file(5, FileRole::Dlc, "switch/MAJ-DLC/d.nsp", 3),
                file(2, FileRole::Maj, "switch/MAJ-DLC/m.nsp", 2),
                file(1, FileRole::Base, "switch/lm3.nsp", 10),
            ],
        )
    }

    #[test]
    fn plan_d_installation() {
        let g = plan_game();
        // Jeu complet : base puis mises à jour, sans DLC.
        let ids: Vec<_> = install_plan(&g, None).unwrap().iter().map(|f| f.id).collect();
        assert_eq!(ids, [1, 2]);
        // Un contenu précis : DLC ou mise à jour seulement.
        assert_eq!(install_plan(&g, Some(5)).unwrap()[0].id, 5);
        assert!(install_plan(&g, Some(1)).unwrap_err().contains("le jeu lui-même"));
        assert!(install_plan(&g, Some(99)).unwrap_err().contains("introuvable"));
        // Sans fichier de base sur le serveur : rien à installer.
        let sans_base = catalog_game("x", "switch", vec![file(2, FileRole::Maj, "switch/MAJ-DLC/m.nsp", 2)]);
        assert!(install_plan(&sans_base, None).is_err());
    }

    #[test]
    fn une_seule_operation_par_jeu() {
        let set: Occupes = Mutex::new(BTreeMap::new());
        let a = JeuOccupe::prendre(&set, "switch-lm3").unwrap();
        assert!(JeuOccupe::prendre(&set, "switch-lm3").unwrap_err().contains("déjà en cours"));
        // Un autre jeu n'est pas bloqué.
        let _b = JeuOccupe::prendre(&set, "ds-x").unwrap();
        // Annulation : lève le drapeau du jeu concerné seulement.
        assert!(annuler(&set, "switch-lm3"));
        assert!(a.cancel.load(Ordering::SeqCst) && !_b.cancel.load(Ordering::SeqCst));
        assert!(!annuler(&set, "inconnu"));
        drop(a);
        assert!(JeuOccupe::prendre(&set, "switch-lm3").is_ok());
    }

    #[test]
    fn evenement_de_progression() {
        let p = Progression { game: "g".into(), file: 3, done: 10, total: 40, prepare: None };
        // Sans préparation, la charge reste celle d'avant (compatibilité de l'interface).
        assert_eq!(serde_json::to_value(&p).unwrap(), serde_json::json!({ "game": "g", "file": 3, "done": 10, "total": 40 }));
        let p = Progression { prepare: Some(Preparation { queued: false, percent: Some(42), elapsed_s: 7 }), ..p };
        assert_eq!(
            serde_json::to_value(&p).unwrap(),
            serde_json::json!({ "game": "g", "file": 3, "done": 10, "total": 40, "prepare": { "queued": false, "percent": 42, "elapsedS": 7 } })
        );
        assert_eq!(EVENT_PROGRESSION, "ludo://progression");
    }

    #[test]
    fn formats_de_l_emulateur_du_jeu() {
        let cfg: Config = serde_json::from_value(serde_json::json!({
            "serveur": "x", "appareil": "pc",
            "jeux": { "switch-mk8": { "emulateur": "eden", "title_id": "0100152000022000" } }
        }))
        .unwrap();
        assert_eq!(accepted_formats(&cfg, &catalog_game("switch-mk8", "switch", vec![])), ["nsp", "xci"]);
        // Sans section [jeux] : émulateur par défaut de la console (Ryujinx pour la Switch).
        assert_eq!(accepted_formats(&cfg, &catalog_game("switch-autre", "switch", vec![])), ["nsp", "xci"]);
        assert!(accepted_formats(&cfg, &catalog_game("3ds-x", "3ds", vec![])).is_empty());
    }

    #[test]
    fn dossier_des_roms_obligatoire() {
        let cfg: Config = serde_json::from_value(serde_json::json!({ "serveur": "x", "appareil": "pc" })).unwrap();
        assert!(roms_dir(&cfg).unwrap_err().contains("roms"));
        let cfg: Config = serde_json::from_value(serde_json::json!({ "serveur": "x", "appareil": "pc", "roms": "/r" })).unwrap();
        assert_eq!(roms_dir(&cfg).unwrap(), std::path::PathBuf::from("/r"));
    }
}
