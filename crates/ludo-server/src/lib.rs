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
//! - `GET    /saves/{profile}`                        têtes de sauvegarde du profil, une par jeu
//! - `GET    /games`                                  catalogue des jeux et de leurs fichiers
//! - `GET    /games/{id}/files/{file_id}`              téléchargement (HTTP Range pour reprendre)
//! - `POST   /catalog/rescan`                         nouveau balayage du dossier des ROM
//!
//! Pas d'authentification : usage LAN seulement (conception 4.9).

pub mod catalog;
pub mod store;

use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use ludo_proto::{LeaseOutcome, LeaseRequest, Profile, PushOutcome, PushParams, SaveKey};
use serde::Deserialize;
use std::sync::Arc;
use store::{Store, StoreError};

/// Taille maximale d'une archive de sauvegarde (certaines sauvegardes Switch pèsent des centaines de Mo).
pub const MAX_SAVE_BYTES: usize = 1024 * 1024 * 1024;

/// Taille des blocs lus pour envoyer une ROM (les ROM pèsent jusqu'à 15 Go ; 4 Kio par défaut limiterait le débit).
const STREAM_CHUNK: usize = 256 * 1024;

impl IntoResponse for StoreError {
    fn into_response(self) -> Response {
        let status = match self {
            StoreError::InvalidId(_)
            | StoreError::InvalidArchive(_)
            | StoreError::EmptyArchive
            | StoreError::InvalidProfile(_) => StatusCode::BAD_REQUEST,
            StoreError::NotFound => StatusCode::NOT_FOUND,
            StoreError::Forbidden(_) => StatusCode::FORBIDDEN,
            StoreError::IsDirectory => StatusCode::UNPROCESSABLE_ENTITY,
            StoreError::NoRoms | StoreError::RomsUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
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
        .route("/api/v1/saves/{profile}", get(get_summaries))
        .route("/api/v1/games", get(get_games))
        .route("/api/v1/games/{id}/files/{file_id}", get(get_game_file))
        .route("/api/v1/catalog/rescan", post(post_rescan))
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

async fn get_summaries(State(s): State<AppState>, Path(profile): Path<String>) -> Result<Response, StoreError> {
    Ok(Json(blocking(move || s.save_summaries(&profile)).await?).into_response())
}

async fn get_games(State(s): State<AppState>) -> Result<Response, StoreError> {
    Ok(Json(blocking(move || s.games()).await?).into_response())
}

async fn post_rescan(State(s): State<AppState>) -> Result<Response, StoreError> {
    Ok(Json(blocking(move || s.rescan()).await?).into_response())
}

/// Intervalle d'octets demandé par un en-tête `Range`.
#[derive(Debug, PartialEq, Eq)]
pub enum RangeSpec {
    /// Pas d'en-tête exploitable : le fichier entier.
    Full,
    /// Octets `start..=end` (déjà bornés à la taille).
    Part(u64, u64),
    /// Intervalle hors du fichier : 416.
    Unsatisfiable,
}

/// Lit un en-tête `Range: bytes=a-b | a- | -n` pour un fichier de `len` octets. Un intervalle
/// multiple ou d'une autre unité est ignoré (réponse complète, ce que HTTP autorise).
pub fn parse_range(header: Option<&str>, len: u64) -> RangeSpec {
    let Some(h) = header.map(str::trim) else { return RangeSpec::Full };
    let Some(spec) = h.get(..6).filter(|p| p.eq_ignore_ascii_case("bytes=")).map(|_| &h[6..]) else {
        return RangeSpec::Full;
    };
    // Nombres : chiffres seulement (pas de « +5 ») ; un nombre qui déborde rend la plage insatisfiable.
    let num = |x: &str| -> Option<Result<u64, ()>> {
        if x.is_empty() {
            None
        } else if x.bytes().all(|c| c.is_ascii_digit()) {
            Some(x.parse::<u64>().map_err(|_| ()))
        } else {
            Some(Err(()))
        }
    };
    if spec.contains(',') {
        return RangeSpec::Full;
    }
    let Some((a, b)) = spec.split_once('-') else { return RangeSpec::Full };
    let (a, b) = (a.trim(), b.trim());
    match (num(a), num(b)) {
        // Débordement ou suffixe nul (« bytes=-0 ») : insatisfiable (RFC 9110, 14.1.2).
        (Some(Ok(_)), Some(Err(()))) | (None, Some(Err(()))) | (Some(Err(())), _) => {
            if a.bytes().chain(b.bytes()).all(|c| c.is_ascii_digit()) { RangeSpec::Unsatisfiable } else { RangeSpec::Full }
        }
        (None, Some(Ok(0))) => RangeSpec::Unsatisfiable,
        (Some(Ok(start)), Some(Ok(end))) if start <= end => {
            if start >= len {
                RangeSpec::Unsatisfiable
            } else {
                RangeSpec::Part(start, end.min(len - 1))
            }
        }
        (Some(Ok(start)), None) => {
            if start >= len {
                RangeSpec::Unsatisfiable
            } else {
                RangeSpec::Part(start, len - 1)
            }
        }
        (None, Some(Ok(n))) => {
            if len == 0 {
                RangeSpec::Unsatisfiable
            } else {
                RangeSpec::Part(len.saturating_sub(n), len - 1)
            }
        }
        _ => RangeSpec::Full,
    }
}

/// Téléchargement d'un fichier de jeu, en flux (jamais chargé en mémoire), avec reprise (Range).
async fn get_game_file(
    State(s): State<AppState>,
    Path((game, file_id)): Path<(String, u64)>,
    headers: HeaderMap,
) -> Result<Response, StoreError> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let (path, _) = blocking(move || s.file_location(&game, file_id)).await?;
    let mut file = tokio::fs::File::open(&path).await.map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => StoreError::NotFound,
        _ => StoreError::Io(e),
    })?;
    let meta = file.metadata().await?;
    if !meta.is_file() {
        return Err(StoreError::Forbidden("pas un fichier".into()));
    }
    let len = meta.len();
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    // Validateur faible : taille et date. Un `If-Range` différent annule la reprise (fichier remplacé).
    let etag = format!("\"{len:x}-{mtime:x}\"");
    let header = |name| headers.get(name).and_then(|v| v.to_str().ok());
    let wanted = match header(header::IF_RANGE) {
        Some(v) if v.trim() != etag => None,
        _ => header(header::RANGE),
    };
    let base = Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::ETAG, &etag);
    let respond = |b: axum::http::response::Builder, body: Body| b.body(body).map_err(|e| StoreError::Io(std::io::Error::other(e.to_string())));
    match parse_range(wanted, len) {
        RangeSpec::Full => respond(base.status(StatusCode::OK).header(header::CONTENT_LENGTH, len), Body::from_stream(tokio_util::io::ReaderStream::with_capacity(file, STREAM_CHUNK))),
        RangeSpec::Part(start, end) => {
            file.seek(std::io::SeekFrom::Start(start)).await?;
            let n = end - start + 1;
            respond(
                base.status(StatusCode::PARTIAL_CONTENT)
                    .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
                    .header(header::CONTENT_LENGTH, n),
                Body::from_stream(tokio_util::io::ReaderStream::with_capacity(file.take(n), STREAM_CHUNK)),
            )
        }
        RangeSpec::Unsatisfiable => respond(
            base.status(StatusCode::RANGE_NOT_SATISFIABLE).header(header::CONTENT_RANGE, format!("bytes */{len}")),
            Body::empty(),
        ),
    }
}
