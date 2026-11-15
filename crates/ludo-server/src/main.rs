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
    if let Some(roms) = std::env::var_os("LUDO_ROMS") {
        store = store.with_roms(roms.into());
        // Un catalogue illisible ne doit pas empêcher les sauvegardes : on garde l'ancien catalogue.
        match store.rescan() {
            Ok(r) => tracing::info!(jeux = r.games, fichiers = r.files, "catalogue balayé"),
            Err(e) => tracing::warn!(erreur = %e, "balayage du catalogue échoué"),
        }
    }
    let store = Arc::new(store);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(%addr, %data, "ludo-server démarré");
    axum::serve(listener, ludo_server::router(store))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
