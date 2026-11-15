//! Coque Tauri de l'interface Ludothèque : expose au front les commandes du client `ludo`.
//!
//! Commandes branchées : `device`, `rumble` (vibrations natives), `profiles`, `saveProfile`,
//! `history`, `restore`, `sync`. Les autres (`recent`, `library`, `updates`, `prepare`) arriveront
//! avec le catalogue du serveur (lot P1c) ; d'ici là, le front utilise sa source de démonstration
//! pour chaque commande absente ou en échec.

pub mod rumble;

use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike};
use ludo_client::api::Client;
use ludo_client::config::{self, Config};
use ludo_client::engine::{Engine, SyncOutcome};
use ludo_client::journal::Journal;
use ludo_proto::{Profile, SaveKey};
use serde::Serialize;
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

/// Restaure une version : elle devient la nouvelle tête sur le serveur (compare-and-swap contre
/// la tête actuelle), puis remplace la sauvegarde locale si le jeu est configuré sur cet appareil.
/// Conflit (la tête a bougé entre-temps) : erreur, rien n'est remplacé.
#[tauri::command]
async fn restore(profile: String, game: String, rev: u64) -> Result<(), String> {
    blocking(move || {
        let cfg = load_config()?;
        let client = Client::new(&cfg.serveur);
        // Jeu configuré mais adaptateur impossible à construire : erreur avant de toucher au serveur.
        let prepared = match cfg.jeux.get(&game) {
            Some(g) => Some(config::prepare(&cfg, &g.emulateur, &profile, &g.title_id).map_err(|e| e.to_string())?),
            None => None,
        };
        let _guard = JOURNAL.lock().map_err(|e| e.to_string())?;
        let mut journal = Journal::open(&config::journal_path()).map_err(|e| e.to_string())?;
        let mut engine = Engine {
            client: &client,
            journal: &mut journal,
            device: cfg.appareil.clone(),
            emulator: cfg.jeux.get(&game).map(|g| g.emulateur.clone()).unwrap_or_default(),
            playtime_s: 0,
        };
        let key = SaveKey { profile, game };
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
    let _guard = JOURNAL.lock().map_err(|e| e.to_string())?;
    let mut journal = Journal::open(&config::journal_path()).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (id, g) in cfg.games_for(profile) {
        let key = SaveKey { profile: profile.into(), game: id.clone() };
        let result = (|| {
            let prepared = config::prepare(cfg, &g.emulateur, profile, &g.title_id).map_err(|e| e.to_string())?;
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

/// Synchronisation manuelle de tous les jeux configurés pour ce profil.
///
/// L'interface attend la bibliothèque à jour (`Game[]`), qui n'existe pas encore (catalogue du
/// lot P1c) : après avoir réellement synchronisé, la commande renvoie toujours une erreur qui
/// résume chaque jeu, et l'interface garde sa bibliothèque de démonstration. Une liste vide
/// viderait la bibliothèque de l'accueil (`all = lib` dans `Accueil.svelte`).
#[tauri::command]
async fn sync(profile: String) -> Result<Vec<serde_json::Value>, String> {
    blocking(move || {
        let cfg = load_config()?;
        let results = sync_profile(&cfg, &profile)?;
        let resume: Vec<String> = results
            .iter()
            .map(|(id, r)| match r {
                Ok(o) => format!("{id} : {}", sync_label(o)),
                Err(e) => format!("{id} : {e}"),
            })
            .collect();
        Err(format!("catalogue des jeux absent (lot P1c) ; synchronisation faite : [{}]", resume.join(" ; ")))
    })
    .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    rumble::init();
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![device, rumble::rumble, profiles, save_profile, history, restore, sync])
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
}
