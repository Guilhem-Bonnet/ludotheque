//! Point d'entrée du serveur. Variables : `LUDO_DATA` (dossier de données, défaut `./data`),
//! `LUDO_ADDR` (défaut `0.0.0.0:8080`).

use ludo_server::store::{Store, SystemClock};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_target(false).init();
    let data = std::env::var("LUDO_DATA").unwrap_or_else(|_| "./data".into());
    let addr = std::env::var("LUDO_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let store = Arc::new(Store::open(std::path::Path::new(&data), Box::new(SystemClock))?);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(%addr, %data, "ludo-server démarré");
    axum::serve(listener, ludo_server::router(store))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
