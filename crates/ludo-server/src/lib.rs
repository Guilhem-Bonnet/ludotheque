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
//! - `GET    /games/{id}/files/{file_id}`              téléchargement (HTTP Range pour reprendre) ;
//!   un dossier (Wii U, Switch 2 extrait) est servi en tar non compressé généré à la volée
//!   (`Content-Type: application/x-tar`, `X-Ludo-Archive: tar`, `X-Ludo-Total-Bytes` = somme des
//!   fichiers) : pas de `Range` (réponse 200 complète, l'en-tête est ignoré), pas de `Content-Length`
//!   `?formats=nsp,xci` : formats lus par l'émulateur ; un NSZ / XCZ non accepté est converti par le
//!   serveur (cache `LUDO_CACHE`) : `202` + `Retry-After` + JSON tant que ce n'est pas prêt, `503` sans clés
//! - `PUT    /games/{id}/cover`                      image du jeu (png, jpeg ou webp, 5 Mo au plus, octets magiques vérifiés)
//! - `PUT    /games/{id}/background`                 image de fond, grand format (mêmes règles)
//! - `GET    /games/{id}/background`                 image de fond (ETag, `Cache-Control`)
//! - `GET    /games/{id}/cover`                      image du jeu (ETag, `Cache-Control` ; 304 sur `If-None-Match`)
//! - `POST   /catalog/rescan`                         nouveau balayage du dossier des ROM
//!
//! Pas d'authentification : usage LAN seulement (conception 4.9).

pub mod catalog;
pub mod convert;
pub mod store;

use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Extension, Json, Router};
use convert::{Converter, Outcome};
use ludo_proto::{LeaseOutcome, LeaseRequest, Profile, PushOutcome, PushParams, SaveKey};
use serde::Deserialize;
use std::sync::Arc;
use store::{ImageKind, Store, StoreError};

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
            | StoreError::InvalidProfile(_)
            | StoreError::InvalidCover(_) => StatusCode::BAD_REQUEST,
            StoreError::NotFound => StatusCode::NOT_FOUND,
            StoreError::Forbidden(_) => StatusCode::FORBIDDEN,
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
    router_with(store, None)
}

/// Routeur avec conversion NSZ → NSP / XCZ → XCI (`None` : conversion indisponible, 503).
pub fn router_with(store: Arc<Store>, converter: Option<Arc<Converter>>) -> Router {
    Router::new()
        .route("/api/v1/health", get(|| async { "ok" }))
        .route("/api/v1/saves/{profile}/{game}", get(get_head).put(put_save))
        .route("/api/v1/saves/{profile}/{game}/history", get(get_history))
        .route("/api/v1/saves/{profile}/{game}/{rev}", get(get_blob))
        .route("/api/v1/leases/{profile}/{game}", post(post_lease).delete(delete_lease))
        .route("/api/v1/saves/{profile}", get(get_summaries))
        .route("/api/v1/games", get(get_games))
        .route("/api/v1/games/{id}/files/{file_id}", get(get_game_file))
        .route("/api/v1/games/{id}/cover", get(get_cover).put(put_cover).layer(DefaultBodyLimit::max(store::MAX_COVER_BYTES)))
        .route("/api/v1/games/{id}/background", get(get_background).put(put_background).layer(DefaultBodyLimit::max(store::MAX_COVER_BYTES)))
        .route("/api/v1/catalog/rescan", post(post_rescan))
        .route("/api/v1/profiles", get(get_profiles))
        // Un profil tient en quelques centaines d'octets : limite propre à cette route (16 Kio).
        .route("/api/v1/profiles/{id}", put(put_profile).layer(DefaultBodyLimit::max(16 * 1024)))
        .layer(DefaultBodyLimit::max(MAX_SAVE_BYTES))
        .layer(Extension(converter))
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

async fn put_cover(State(s): State<AppState>, Path(id): Path<String>, body: Bytes) -> Result<Response, StoreError> {
    put_image(s, ImageKind::Cover, id, body).await
}

async fn get_cover(State(s): State<AppState>, Path(id): Path<String>, headers: HeaderMap) -> Result<Response, StoreError> {
    get_image(s, ImageKind::Cover, id, headers).await
}

async fn put_background(State(s): State<AppState>, Path(id): Path<String>, body: Bytes) -> Result<Response, StoreError> {
    put_image(s, ImageKind::Background, id, body).await
}

async fn get_background(State(s): State<AppState>, Path(id): Path<String>, headers: HeaderMap) -> Result<Response, StoreError> {
    get_image(s, ImageKind::Background, id, headers).await
}

async fn put_image(s: AppState, kind: ImageKind, id: String, body: Bytes) -> Result<Response, StoreError> {
    blocking(move || s.put_image(kind, &id, &body)).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn get_image(s: AppState, kind: ImageKind, id: String, headers: HeaderMap) -> Result<Response, StoreError> {
    let (path, mime, v) = blocking(move || s.image_info(kind, &id).ok_or(StoreError::NotFound)).await?;
    let etag = format!("\"{v}\"");
    // L'URL porte déjà `?v=` : l'image ne change pas sous une même URL, le cache peut la garder longtemps.
    let cache = "public, max-age=86400";
    if headers.get(header::IF_NONE_MATCH).and_then(|h| h.to_str().ok()).is_some_and(|h| h.split(',').any(|t| t.trim() == etag)) {
        return Ok((StatusCode::NOT_MODIFIED, [(header::ETAG, etag), (header::CACHE_CONTROL, cache.into())]).into_response());
    }
    let data = tokio::fs::read(&path).await.map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { StoreError::NotFound } else { StoreError::Io(e) })?;
    Ok(([(header::CONTENT_TYPE, mime.to_string()), (header::ETAG, etag), (header::CACHE_CONTROL, cache.into()), (header::X_CONTENT_TYPE_OPTIONS, "nosniff".into())], data).into_response())
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
///
/// `?formats=nsp,xci` : formats que l'émulateur du client sait lire. Un NSZ / XCZ stocké que le
/// client ne lit pas, alors qu'il lit le NSP / XCI équivalent, est converti (cache du serveur) :
/// `202 Accepted` + `Retry-After` + [`ludo_proto::ConvertProgress`] tant que ce n'est pas prêt, puis
/// le fichier converti comme n'importe quel fichier (`X-Ludo-Format` = format servi). Sans le
/// paramètre, ou si le format stocké est accepté, le fichier est servi tel quel.
async fn get_game_file(
    State(s): State<AppState>,
    Extension(conv): Extension<Option<Arc<Converter>>>,
    Path((game, file_id)): Path<(String, u64)>,
    Query(q): Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Response, StoreError> {
    let (path, gfile) = blocking(move || s.file_location(&game, file_id)).await?;
    let accepted: Vec<String> = q.get("formats").map(|v| v.split(',').map(|f| f.trim().to_ascii_lowercase()).filter(|f| !f.is_empty()).collect()).unwrap_or_default();
    let Some(target) = ludo_proto::conversion_target(&gfile.format, &accepted) else {
        return serve_path(path, &headers, None, &[]).await;
    };
    let unavailable = || (StatusCode::SERVICE_UNAVAILABLE, "Conversion indisponible : clés manquantes").into_response();
    let Some(conv) = conv else { return Ok(unavailable()) };
    let (len, mtime) = {
        let m = tokio::fs::metadata(&path).await.map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { StoreError::NotFound } else { StoreError::Io(e) })?;
        (m.len(), m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs()))
    };
    match conv.request(&path, len, mtime, target).await {
        Outcome::Ready { file, len, key, format, guard } => {
            // Validateur stable : identifiant de conversion et taille (la date du cache change à chaque accès).
            let etag = format!("\"c{}-{len:x}\"", &key[..16]);
            serve_open(tokio::fs::File::from_std(file), &headers, Some(etag), &[("x-ludo-format", format.to_string()), ("x-ludo-total-bytes", len.to_string())], Some(guard)).await
        }
        Outcome::Pending(p) => Ok(Response::builder()
            .status(StatusCode::ACCEPTED)
            .header(header::RETRY_AFTER, "5")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&p).unwrap_or_default()))
            .map_err(|e| StoreError::Io(std::io::Error::other(e.to_string())))?),
        Outcome::Failed(msg) => Ok((StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()),
        Outcome::Unavailable => Ok(unavailable()),
    }
}

/// Sert un fichier ou un dossier (tar) du disque ; `etag` remplace le validateur taille + date,
/// `extra` ajoute des en-têtes à la réponse.
async fn serve_path(path: std::path::PathBuf, headers: &HeaderMap, etag: Option<String>, extra: &[(&'static str, String)]) -> Result<Response, StoreError> {
    // Un dossier est servi en tar à la volée (le `Range` est ignoré : réponse 200 complète).
    if tokio::fs::symlink_metadata(&path).await.is_ok_and(|m| m.is_dir()) {
        return tar_response(path).await;
    }
    let file = tokio::fs::File::open(&path).await.map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => StoreError::NotFound,
        _ => StoreError::Io(e),
    })?;
    serve_open(file, headers, etag, extra, None).await
}

/// Flux qui garde `_guard` (utilisation d'un fichier du cache) jusqu'à la fin de l'envoi.
struct Guarded<S>(S, #[allow(dead_code)] Option<convert::UseGuard>);

impl<S: futures_core::Stream + Unpin> futures_core::Stream for Guarded<S> {
    type Item = S::Item;
    fn poll_next(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.0).poll_next(cx)
    }
}

/// Sert un fichier déjà ouvert (donc à l'abri d'une éviction du cache entre-temps).
async fn serve_open(mut file: tokio::fs::File, headers: &HeaderMap, etag: Option<String>, extra: &[(&'static str, String)], guard: Option<convert::UseGuard>) -> Result<Response, StoreError> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let meta = file.metadata().await?;
    if !meta.is_file() {
        return Err(StoreError::Forbidden("pas un fichier".into()));
    }
    let len = meta.len();
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    // Validateur faible : taille et date. Un `If-Range` différent annule la reprise (fichier remplacé).
    let etag = etag.unwrap_or_else(|| format!("\"{len:x}-{mtime:x}\""));
    let header = |name| headers.get(name).and_then(|v| v.to_str().ok());
    let wanted = match header(header::IF_RANGE) {
        Some(v) if v.trim() != etag => None,
        _ => header(header::RANGE),
    };
    let base = Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::ETAG, &etag);
    let base = extra.iter().fold(base, |b, (k, v)| b.header(*k, v));
    let respond = |b: axum::http::response::Builder, body: Body| b.body(body).map_err(|e| StoreError::Io(std::io::Error::other(e.to_string())));
    match parse_range(wanted, len) {
        RangeSpec::Full => respond(base.status(StatusCode::OK).header(header::CONTENT_LENGTH, len), Body::from_stream(Guarded(tokio_util::io::ReaderStream::with_capacity(file, STREAM_CHUNK), guard))),
        RangeSpec::Part(start, end) => {
            file.seek(std::io::SeekFrom::Start(start)).await?;
            let n = end - start + 1;
            respond(
                base.status(StatusCode::PARTIAL_CONTENT)
                    .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
                    .header(header::CONTENT_LENGTH, n),
                Body::from_stream(Guarded(tokio_util::io::ReaderStream::with_capacity(file.take(n), STREAM_CHUNK), guard)),
            )
        }
        RangeSpec::Unsatisfiable => respond(
            base.status(StatusCode::RANGE_NOT_SATISFIABLE).header(header::CONTENT_RANGE, format!("bytes */{len}")),
            Body::empty(),
        ),
    }
}

/// Écrivain bloquant qui découpe le tar en blocs envoyés au flux de la réponse. Si le client se
/// déconnecte (récepteur fermé), l'écriture échoue et la tâche de génération s'arrête.
struct ChannelWriter {
    tx: tokio::sync::mpsc::Sender<std::io::Result<Bytes>>,
    buf: Vec<u8>,
}

impl ChannelWriter {
    fn send_buf(&mut self) -> std::io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let chunk = Bytes::from(std::mem::replace(&mut self.buf, Vec::with_capacity(STREAM_CHUNK)));
        self.tx.blocking_send(Ok(chunk)).map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))
    }
}

impl std::io::Write for ChannelWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(data);
        if self.buf.len() >= STREAM_CHUNK {
            self.send_buf()?;
        }
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.send_buf()
    }
}

/// Flux d'octets alimenté par un canal (la génération du tar tourne sur un fil bloquant).
struct ChannelStream(tokio::sync::mpsc::Receiver<std::io::Result<Bytes>>);

impl futures_core::Stream for ChannelStream {
    type Item = std::io::Result<Bytes>;
    fn poll_next(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::Item>> {
        self.0.poll_recv(cx)
    }
}

/// Entrée d'un dossier à archiver : chemin relatif (séparateur `/`) et nature.
#[derive(Debug, PartialEq, Eq)]
pub struct TarEntry {
    /// Chemin relatif au dossier du jeu (peut ne pas être de l'UTF-8 : le scanner compte ces entrées aussi).
    pub name: std::path::PathBuf,
    pub is_dir: bool,
}

/// Liste, dans un ordre stable, les dossiers et fichiers réguliers sous `root` (chemins relatifs à
/// `root`). Les liens symboliques et tout ce qui n'est ni fichier ni dossier sont ignorés : le
/// tar ne peut ni sortir de la racine ni suivre un lien. Renvoie aussi la somme des tailles.
pub fn list_tar_entries(root: &std::path::Path) -> std::io::Result<(Vec<TarEntry>, u64)> {
    fn walk(dir: &std::path::Path, prefix: &std::path::Path, out: &mut Vec<TarEntry>, total: &mut u64) -> std::io::Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let ft = e.file_type()?;
            let rel = prefix.join(e.file_name());
            if ft.is_dir() {
                out.push(TarEntry { name: rel.clone(), is_dir: true });
                walk(&e.path(), &rel, out, total)?;
            } else if ft.is_file() {
                *total += e.metadata()?.len();
                out.push(TarEntry { name: rel, is_dir: false });
            }
        }
        Ok(())
    }
    let (mut out, mut total) = (Vec::new(), 0);
    walk(root, std::path::Path::new(""), &mut out, &mut total)?;
    Ok((out, total))
}

fn write_tar(root: &std::path::Path, entries: &[TarEntry], w: ChannelWriter) -> std::io::Result<()> {
    let mut b = tar::Builder::new(w);
    b.mode(tar::HeaderMode::Deterministic);
    for e in entries {
        let full = root.join(&e.name);
        if e.is_dir {
            b.append_dir(&e.name, &full)?;
        } else {
            // Fichier remplacé par autre chose depuis le listage : on s'arrête (tar incohérent).
            let mut f = std::fs::File::open(&full)?;
            if !f.metadata()?.is_file() {
                return Err(std::io::Error::other("entrée modifiée pendant l'archivage"));
            }
            b.append_file(&e.name, &mut f)?;
        }
    }
    let mut w = b.into_inner()?;
    std::io::Write::flush(&mut w)
}

/// Réponse tar d'un dossier de jeu : générée à la volée, sans copie temporaire.
async fn tar_response(root: std::path::PathBuf) -> Result<Response, StoreError> {
    let dir = root.clone();
    let (entries, total) = blocking(move || list_tar_entries(&dir).map_err(StoreError::Io)).await?;
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    let err_tx = tx.clone();
    tokio::task::spawn_blocking(move || {
        let w = ChannelWriter { tx, buf: Vec::with_capacity(STREAM_CHUNK) };
        if let Err(e) = write_tar(&root, &entries, w) {
            tracing::warn!(erreur = %e, "archivage interrompu");
            // Erreur transmise au flux : la réponse se termine en échec (le client voit un tar tronqué).
            let _ = err_tx.blocking_send(Err(e));
        }
    });
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-tar")
        .header("x-ludo-archive", "tar")
        .header("x-ludo-total-bytes", total)
        .body(Body::from_stream(ChannelStream(rx)))
        .map_err(|e| StoreError::Io(std::io::Error::other(e.to_string())))
}
