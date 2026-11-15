//! Coque Tauri de l'interface Ludothèque : expose au front les commandes du client `ludo`.
//!
//! Commandes branchées : `device`, `rumble` (vibrations natives), `profiles`, `saveProfile`,
//! `history`, `restore`, `sync`, `library`, `recent` (catalogue du serveur). Les autres (`updates`,
//! `prepare`) arriveront avec la suite du lot P1c ; d'ici là, le front utilise sa source de
//! démonstration pour chaque commande absente ou en échec.

pub mod rumble;

use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike};
use ludo_client::api::Client;
use ludo_client::config::{self, Config};
use ludo_client::engine::{Engine, SyncOutcome};
use ludo_client::journal::{Entry, Journal};
use ludo_proto::{CatalogGame, FileRole, GameArt, GameFile, LeaseOutcome, Profile, SaveKey, SaveSummary};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    name: String,
    kind: &'static str,
    default_profile: Option<String>,
    server: &'static str,
    free_space: String,
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
            free_space: String::new(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
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
        let conf = cfg.jeux.get(&game).filter(|g| g.concerns(&profile));
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

/// Synchronise chaque jeu configuré pour ce profil.
fn sync_profile(cfg: &Config, profile: &str) -> Result<Vec<GameSync>, String> {
    let client = Client::new(&cfg.serveur);
    let _guard = JOURNAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut journal = Journal::open(&config::journal_path()).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (id, g) in cfg.games_for(profile) {
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
    emulator: String,
    install: &'static str,
    sync: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    playtime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last: Option<UiLast>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<String>,
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

/// État de synchronisation d'après le journal local de l'appareil.
pub fn sync_state(entry: Option<&Entry>) -> &'static str {
    match entry {
        Some(e) if e.conflict => "conflit",
        Some(e) if e.pending_upload => "envoi_en_attente",
        Some(_) => "a_jour",
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
                emulator: emulator_label((env.emulator)(&g.id).as_deref(), &g.console),
                install: install_state(&g.files, env.exists),
                sync: sync_state((env.journal)(&g.id).as_ref()),
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
    if games.is_empty() {
        return Err("aucun jeu dans le catalogue du serveur".into());
    }
    let saves = client.save_summaries(profile).map_err(|e| e.to_string())?;
    let journal = Journal::open(&config::journal_path()).map_err(|e| e.to_string())?;
    let prefix = format!("{profile}/");
    let entries: BTreeMap<String, Entry> =
        journal.entries().filter_map(|(k, e)| k.strip_prefix(&prefix).map(|g| (g.to_string(), e.clone()))).collect();
    let library = build_library(
        &games,
        &LibraryEnv {
            saves: &saves,
            journal: &|g| entries.get(g).cloned(),
            emulator: &|g| cfg.jeux.get(g).map(|c| c.emulateur.clone()),
            exists: &|p| cfg.roms.as_ref().is_some_and(|r| r.join(p).exists()),
            to_local: &local_time,
            today: Local::now().date_naive(),
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
        for (id, result) in sync_profile(&cfg, &profile)? {
            match result {
                Ok(o) => eprintln!("sync {id} : {}", sync_label(&o)),
                Err(e) => eprintln!("sync {id} : échec : {e}"),
            }
        }
        Ok(fetch_library(&cfg, &profile)?.0)
    })
    .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    rumble::init();
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![device, rumble::rumble, profiles, save_profile, history, restore, sync, library, recent])
        .run(tauri::generate_context!())
        .expect("échec du démarrage de l'interface");
}

#[cfg(test)]
mod tests {
    use super::*;

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
            catalogued: true,
            files,
        }
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
        assert_eq!(sync_state(Some(&entry(true, true))), "conflit");
        assert_eq!(sync_state(Some(&entry(false, true))), "envoi_en_attente");
        assert_eq!(sync_state(Some(&entry(false, false))), "a_jour");
        assert_eq!(sync_state(None), "aucune");
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
        }
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
}
