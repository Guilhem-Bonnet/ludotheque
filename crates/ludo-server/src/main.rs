//! Point d'entrée du serveur. Variables : `LUDO_DATA` (dossier de données, défaut `./data`),
//! `LUDO_ADDR` (défaut `0.0.0.0:8080`),
//! `LUDO_ROMS` (dossier des ROM : un sous-dossier par console + `catalogue.toml` ; absent = catalogue vide).

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
    axum::serve(listener, ludo_server::router(store)).with_graceful_shutdown(shutdown()).await?;
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
