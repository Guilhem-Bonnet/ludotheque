//! Point d'entrée du serveur. Variables : `LUDO_DATA` (dossier de données, défaut `./data`),
//! `LUDO_ADDR` (défaut `0.0.0.0:8080`),
//! `LUDO_ROMS` (dossier des ROM : un sous-dossier par console + `catalogue.toml` ; absent = catalogue vide).
//! Conversion NSZ → NSP / XCZ → XCI : `LUDO_SWITCH_KEYS` (fichier prod.keys ; absent = conversion
//! indisponible, 503), `LUDO_CACHE` (cache des conversions, volume non sauvegardé ; défaut
//! `<LUDO_DATA>/cache`), `LUDO_CACHE_MAX_GB` (défaut 80), `LUDO_CONVERT_CMD` (défaut `nsz`),
//! `LUDO_CONVERT_TIMEOUT_S` (défaut 10800).

use ludo_server::convert::{ConvertConfig, Converter};
use ludo_server::store::{Store, SystemClock};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_target(false).init();
    let data = std::env::var("LUDO_DATA").unwrap_or_else(|_| "./data".into());
    let addr = std::env::var("LUDO_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let mut store = Store::open(std::path::Path::new(&data), Box::new(SystemClock))?;
    let roms = std::env::var_os("LUDO_ROMS");
    if let Some(r) = &roms {
        store = store.with_roms(r.into());
    }
    let store = Arc::new(store);
    let cache = std::env::var_os("LUDO_CACHE").map_or_else(|| std::path::Path::new(&data).join("cache"), Into::into);
    let mut cc = ConvertConfig::new(cache);
    cc.keys = std::env::var_os("LUDO_SWITCH_KEYS").map(Into::into).filter(|k: &std::path::PathBuf| !k.as_os_str().is_empty());
    if let Some(gb) = std::env::var("LUDO_CACHE_MAX_GB").ok().and_then(|v| v.parse::<u64>().ok()) {
        cc.max_bytes = gb.saturating_mul(1_000_000_000);
    }
    if let Ok(c) = std::env::var("LUDO_CONVERT_CMD") {
        cc.command = c;
    }
    if let Some(t) = std::env::var("LUDO_CONVERT_TIMEOUT_S").ok().and_then(|v| v.parse::<u64>().ok()) {
        cc.timeout = std::time::Duration::from_secs(t);
    }
    let converter = match Converter::new(cc.clone()) {
        Ok(c) => Some(c),
        Err(e) => {
            // Un cache inutilisable ne doit pas empêcher les sauvegardes : conversion indisponible.
            tracing::warn!(erreur = %e, cache = %cc.cache.display(), "cache de conversion inutilisable");
            None
        }
    };
    tracing::info!(cles = cc.keys.as_ref().is_some_and(|k| k.is_file()), cache = %cc.cache.display(), max_gb = cc.max_bytes / 1_000_000_000, "conversion NSZ/XCZ");
    // Le serveur écoute tout de suite : les sauvegardes ne dépendent pas du catalogue, et un premier
    // balayage de centaines de Go sur NFS ne doit pas faire croire au cluster que le pod est en panne.
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(%addr, %data, "ludo-server démarré");
    if roms.is_some() {
        let s = store.clone();
        tokio::task::spawn_blocking(move || match s.rescan() {
            Ok(r) => tracing::info!(jeux = r.games, fichiers = r.files, ecartes = r.ignored.len(), "catalogue balayé"),
            // Un catalogue illisible n'empêche pas les sauvegardes : on garde l'ancien catalogue.
            Err(e) => tracing::warn!(erreur = %e, "balayage du catalogue échoué"),
        });
    }
    axum::serve(listener, ludo_server::router_with(store, converter)).with_graceful_shutdown(shutdown()).await?;
    Ok(())
}

/// Arrêt propre sur Ctrl+C ou SIGTERM (envoyé par Kubernetes avant de tuer le pod) : les requêtes
/// en cours, dont l'écriture d'une sauvegarde, se terminent avant l'arrêt.
async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = term => {},
    }
    tracing::info!("arrêt demandé, fin des requêtes en cours");
}
