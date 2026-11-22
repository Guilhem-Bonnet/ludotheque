//! Énergie : état de la batterie (indicateur d'angle) et repos de l'interface.
//!
//! Tout arrive par événement, jamais par scrutation :
//! - **Android** (Ayn Thor) : la page lit la batterie elle-même (`navigator.getBattery()` de la WebView,
//!   événements `levelchange` / `chargingchange`) ; le plugin Kotlin `EnergiePlugin` met les minuteries
//!   JavaScript en pause quand l'app n'est plus visible et baisse la luminosité de l'écran du haut au
//!   repos (commande `energie_repos`).
//! - **Linux** (Steam Deck, portable) : batterie lue dans `/sys/class/power_supply` au démarrage, puis
//!   relue à chaque changement signalé par UPower (`upower --monitor`, flux poussé par D-Bus) ; sans
//!   UPower, l'interface relit seulement au retour du focus (événement de fenêtre).
//! - **PC fixe** (aucune batterie système) : `batterie_etat` rend `None`, rien n'est affiché et aucun
//!   repos n'est appliqué. Les réglages d'énergie du système ne sont jamais touchés.

use serde::Serialize;

/// Événement Tauri émis à chaque changement de batterie (bureau).
pub const EVENT_BATTERIE: &str = "ludo://batterie";

/// Niveau de batterie et branchement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Batterie {
    /// 0 à 100.
    pub niveau: u8,
    /// Secteur branché (en charge ou pleine sur secteur).
    pub en_charge: bool,
}

/// État de la batterie du système, `None` sans batterie (PC fixe) ou hors Linux.
#[tauri::command]
pub fn batterie_etat() -> Option<Batterie> {
    #[cfg(target_os = "linux")]
    {
        lire(std::path::Path::new("/sys/class/power_supply"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Repos de l'interface (inactivité) : sur Android, assombrit l'écran du haut (luminosité de la
/// fenêtre de l'activité) et prévient la page du haut ; ailleurs, rien de plus que le voile CSS.
// Asynchrone : `run_mobile_plugin` attend la réponse du fil d'interface Android, jamais sur la boucle principale.
#[tauri::command]
pub async fn energie_repos(app: tauri::AppHandle, actif: bool) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        android::repos(&app, actif)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, actif);
        Ok(())
    }
}

/// Batterie système dans un dossier de type `/sys/class/power_supply` : première alimentation
/// `type = Battery` qui n'est pas celle d'un périphérique (`scope = Device` : manette, souris).
/// Branchement : `status` Charging/Full, ou une alimentation secteur `online = 1`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn lire(dir: &std::path::Path) -> Option<Batterie> {
    let read = |p: std::path::PathBuf| std::fs::read_to_string(p).ok().map(|s| s.trim().to_string());
    let mut entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).collect();
    entries.sort();
    let mut batterie: Option<(u8, String)> = None;
    let mut secteur = false;
    for p in &entries {
        let kind = read(p.join("type")).unwrap_or_default();
        if kind == "Mains" || kind == "USB" {
            secteur |= read(p.join("online")).as_deref() == Some("1");
        }
        if kind != "Battery" || batterie.is_some() || read(p.join("scope")).as_deref() == Some("Device") {
            continue;
        }
        let Some(niveau) = read(p.join("capacity")).and_then(|c| c.parse::<u8>().ok()) else { continue };
        batterie = Some((niveau.min(100), read(p.join("status")).unwrap_or_default()));
    }
    let (niveau, status) = batterie?;
    Some(Batterie { niveau, en_charge: secteur || status == "Charging" || status == "Full" })
}

/// `upower --monitor` en cours (bureau), arrêté à la fermeture de l'app par [`arreter`].
#[cfg(target_os = "linux")]
static UPOWER: std::sync::Mutex<Option<std::process::Child>> = std::sync::Mutex::new(None);

/// Bureau : suit UPower et émet [`EVENT_BATTERIE`] quand l'état lu change. Rien sans batterie.
/// `upower --monitor` n'écrit qu'à un changement (signal D-Bus de UPower) : la boucle de lecture
/// attend ces lignes, elle ne relit rien d'elle-même. Fin du flux (UPower absent ou arrêté) = fin du
/// suivi, sans relance : l'interface relit alors au retour du focus.
#[cfg(target_os = "linux")]
pub fn suivre(app: tauri::AppHandle) {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    use tauri::Emitter;
    let Some(mut dernier) = batterie_etat() else { return };
    let Ok(mut child) = Command::new("upower").arg("--monitor").stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else { return };
    let Some(out) = child.stdout.take() else { return };
    if let Ok(mut g) = UPOWER.lock() {
        *g = Some(child);
    }
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines() {
            if line.is_err() {
                break;
            }
            if let Some(etat) = batterie_etat() {
                if etat != dernier {
                    dernier = etat.clone();
                    let _ = app.emit(EVENT_BATTERIE, etat);
                }
            }
        }
        // Flux fermé : processus terminé (ou tué par `arreter`), récupéré ici.
        if let Some(mut c) = UPOWER.lock().ok().and_then(|mut g| g.take()) {
            let _ = c.wait();
        }
    });
}

/// Fermeture de l'app : `upower --monitor` ne doit pas lui survivre.
pub fn arreter() {
    #[cfg(target_os = "linux")]
    if let Some(mut c) = UPOWER.lock().ok().and_then(|mut g| g.take()) {
        let _ = c.kill();
        let _ = c.wait();
    }
}

#[cfg(target_os = "android")]
pub use android::plugin;

#[cfg(target_os = "android")]
mod android {
    use serde::Serialize;
    use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
    use tauri::{Manager, Wry};

    /// Plugin Kotlin `EnergiePlugin` (gen/android/app/src/main/java/fr/srvdreamer/ludotheque).
    pub struct Energie(PluginHandle<Wry>);

    pub fn plugin() -> TauriPlugin<Wry> {
        Builder::new("ludo-energie")
            .setup(|app, api| {
                let handle = api.register_android_plugin("fr.srvdreamer.ludotheque", "EnergiePlugin")?;
                app.manage(Energie(handle));
                Ok(())
            })
            .build()
    }

    #[derive(Serialize)]
    struct ReposArgs {
        actif: bool,
    }

    pub fn repos(app: &tauri::AppHandle, actif: bool) -> Result<(), String> {
        let e = app.try_state::<Energie>().ok_or_else(|| "Pont Android indisponible".to_string())?;
        e.0.run_mobile_plugin::<serde_json::Value>("repos", ReposArgs { actif }).map(|_| ()).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Dossier temporaire propre au test (pas de dépendance `tempfile` dans cette crate).
    fn dossier(nom: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ludo-energie-{}-{nom}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn alim(root: &std::path::Path, nom: &str, fichiers: &[(&str, &str)]) {
        let d = root.join(nom);
        fs::create_dir_all(&d).unwrap();
        for (f, v) in fichiers {
            fs::write(d.join(f), format!("{v}\n")).unwrap();
        }
    }

    #[test]
    fn pc_fixe_sans_batterie() {
        let t = dossier("pc");
        alim(t.as_path(), "AC", &[("type", "Mains"), ("online", "1")]);
        // Manette sans fil : batterie de périphérique, jamais prise pour celle du système.
        alim(t.as_path(), "hid-manette-battery", &[("type", "Battery"), ("scope", "Device"), ("capacity", "40"), ("status", "Discharging")]);
        assert_eq!(lire(t.as_path()), None);
        assert_eq!(lire(&t.as_path().join("absent")), None);
    }

    #[test]
    fn deck_sur_batterie_puis_branche() {
        let t = dossier("deck");
        alim(t.as_path(), "ACAD", &[("type", "Mains"), ("online", "0")]);
        alim(t.as_path(), "BAT1", &[("type", "Battery"), ("capacity", "57"), ("status", "Discharging")]);
        assert_eq!(lire(t.as_path()), Some(Batterie { niveau: 57, en_charge: false }));
        alim(t.as_path(), "ACAD", &[("type", "Mains"), ("online", "1")]);
        alim(t.as_path(), "BAT1", &[("type", "Battery"), ("capacity", "100"), ("status", "Not charging")]);
        assert_eq!(lire(t.as_path()), Some(Batterie { niveau: 100, en_charge: true }));
        alim(t.as_path(), "ACAD", &[("type", "Mains"), ("online", "0")]);
        alim(t.as_path(), "BAT1", &[("type", "Battery"), ("capacity", "120"), ("status", "Charging")]);
        assert_eq!(lire(t.as_path()), Some(Batterie { niveau: 100, en_charge: true }));
    }

    #[test]
    fn capacite_illisible_ignoree() {
        let t = dossier("illisible");
        alim(t.as_path(), "BAT0", &[("type", "Battery"), ("capacity", "?"), ("status", "Unknown")]);
        assert_eq!(lire(t.as_path()), None);
    }
}
