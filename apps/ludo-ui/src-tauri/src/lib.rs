//! Coque Tauri de l'interface Ludothèque : expose au front les commandes du client `ludo`.
//!
//! Commandes branchées : `device`. Les autres (`profiles`, `recent`, `library`, `history`,
//! `updates`, `prepare`) arriveront avec le catalogue du serveur ; d'ici là, le front utilise sa
//! source de démonstration pour chaque commande absente.

use ludo_client::api::Client;
use ludo_client::config::Config;
use serde::Serialize;

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![device])
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
}
