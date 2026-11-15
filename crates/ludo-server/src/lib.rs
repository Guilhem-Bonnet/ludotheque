//! Serveur de la ludothèque : API HTTP des sauvegardes et des verrous de partie.
//!
//! Routes (`/api/v1`) :
//! - `GET    /health`
//! - `GET    /saves/{profile}/{game}`                 tête (404 si aucune sauvegarde)
//! - `GET    /saves/{profile}/{game}/history`         révisions, plus récente d'abord
//! - `GET    /saves/{profile}/{game}/{rev}`           contenu (archive neutre)
//! - `PUT    /saves/{profile}/{game}?base_rev=&device=&emulator=&playtime_s=`   envoi (CAS)
//! - `POST   /leases/{profile}/{game}`                prise / renouvellement du verrou
//! - `DELETE /leases/{profile}/{game}?device=`        libération
//! - `GET    /profiles`                               profils du foyer, ordre de création
//! - `PUT    /profiles/{id}`                          création ou mise à jour d'un profil (JSON)
//!
//! Pas d'authentification : usage LAN seulement (conception 4.9).

pub mod store;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use ludo_proto::{LeaseOutcome, LeaseRequest, Profile, PushOutcome, PushParams, SaveKey};
use serde::Deserialize;
use std::sync::Arc;
use store::{Store, StoreError};

/// Taille maximale d'une archive de sauvegarde (certaines sauvegardes Switch pèsent des centaines de Mo).
pub const MAX_SAVE_BYTES: usize = 1024 * 1024 * 1024;

impl IntoResponse for StoreError {
    fn into_response(self) -> Response {
        let status = match self {
            StoreError::InvalidId(_)
            | StoreError::InvalidArchive(_)
            | StoreError::EmptyArchive
            | StoreError::InvalidProfile(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status.is_server_error() {
            tracing::error!(erreur = %self, "erreur serveur");
        }
        (status, self.to_string()).into_response()
    }
}

type AppState = Arc<Store>;

/// Le stockage fait des entrées-sorties bloquantes (disque, SQLite) : hors du runtime async.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, StoreError> + Send + 'static,
) -> Result<T, StoreError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| StoreError::Io(std::io::Error::other(e.to_string())))?
}

pub fn router(store: Arc<Store>) -> Router {
    Router::new()
        .route("/api/v1/health", get(|| async { "ok" }))
        .route("/api/v1/saves/{profile}/{game}", get(get_head).put(put_save))
        .route("/api/v1/saves/{profile}/{game}/history", get(get_history))
        .route("/api/v1/saves/{profile}/{game}/{rev}", get(get_blob))
        .route("/api/v1/leases/{profile}/{game}", post(post_lease).delete(delete_lease))
        .route("/api/v1/profiles", get(get_profiles))
        // Un profil tient en quelques centaines d'octets : limite propre à cette route (16 Kio).
        .route("/api/v1/profiles/{id}", put(put_profile).layer(DefaultBodyLimit::max(16 * 1024)))
        .layer(DefaultBodyLimit::max(MAX_SAVE_BYTES))
        .with_state(store)
}

fn key(profile: String, game: String) -> SaveKey {
    SaveKey { profile, game }
}

async fn get_head(State(s): State<AppState>, Path((p, g)): Path<(String, String)>) -> Result<Response, StoreError> {
    Ok(match blocking(move || s.head(&key(p, g))).await? {
        Some(h) => Json(h).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    })
}

async fn get_history(State(s): State<AppState>, Path((p, g)): Path<(String, String)>) -> Result<Response, StoreError> {
    Ok(Json(blocking(move || s.history(&key(p, g))).await?).into_response())
}

async fn get_blob(
    State(s): State<AppState>,
    Path((p, g, rev)): Path<(String, String, u64)>,
) -> Result<Response, StoreError> {
    Ok(match blocking(move || s.blob(&key(p, g), rev)).await? {
        Some(data) => ([("content-type", "application/x-tar")], data).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    })
}

async fn put_save(
    State(s): State<AppState>,
    Path((p, g)): Path<(String, String)>,
    Query(params): Query<PushParams>,
    body: Bytes,
) -> Result<Response, StoreError> {
    // Une archive vide ou malsaine est refusée par le stockage (400).
    let outcome = blocking(move || s.push(&key(p, g), &params, &body)).await?;
    let status = match outcome {
        PushOutcome::Accepted { .. } => StatusCode::CREATED,
        PushOutcome::Unchanged { .. } => StatusCode::OK,
        PushOutcome::Conflict { .. } => StatusCode::CONFLICT,
    };
    Ok((status, Json(outcome)).into_response())
}

async fn post_lease(
    State(s): State<AppState>,
    Path((p, g)): Path<(String, String)>,
    Json(req): Json<LeaseRequest>,
) -> Result<Response, StoreError> {
    let outcome = blocking(move || s.acquire_lease(&key(p, g), &req.device, req.force)).await?;
    let status = match outcome {
        LeaseOutcome::Granted { .. } => StatusCode::OK,
        LeaseOutcome::HeldElsewhere { .. } => StatusCode::LOCKED,
    };
    Ok((status, Json(outcome)).into_response())
}

#[derive(Deserialize)]
struct DeviceQuery {
    device: String,
}

async fn delete_lease(
    State(s): State<AppState>,
    Path((p, g)): Path<(String, String)>,
    Query(q): Query<DeviceQuery>,
) -> Result<Response, StoreError> {
    blocking(move || s.release_lease(&key(p, g), &q.device)).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn get_profiles(State(s): State<AppState>) -> Result<Response, StoreError> {
    Ok(Json(blocking(move || s.profiles()).await?).into_response())
}

async fn put_profile(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(profile): Json<Profile>,
) -> Result<Response, StoreError> {
    Ok(Json(blocking(move || s.put_profile(&id, &profile)).await?).into_response())
}
