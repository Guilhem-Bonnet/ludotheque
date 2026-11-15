//! Client HTTP synchrone du serveur de la ludothèque.

use ludo_proto::{CatalogGame, LeaseOutcome, LeaseRequest, Profile, PushOutcome, PushParams, RescanReport, SaveKey, SaveRevision, SaveSummary};
use std::time::Duration;
use ureq::Agent;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// Serveur injoignable (réseau, délai) : la ludothèque passe en « Hors ligne ».
    #[error("serveur injoignable : {0}")]
    Offline(String),
    #[error("réponse inattendue du serveur ({status}) : {body}")]
    Http { status: u16, body: String },
    #[error("réponse illisible : {0}")]
    Decode(String),
}

impl From<ureq::Error> for ApiError {
    fn from(e: ureq::Error) -> Self {
        ApiError::Offline(e.to_string())
    }
}

/// Taille maximale d'une sauvegarde téléchargée.
const MAX_DOWNLOAD: u64 = 1024 * 1024 * 1024;

pub struct Client {
    agent: Agent,
    /// Agent des téléchargements de jeux : pas de délai global (une ROM pèse jusqu'à 15 Go) ; le
    /// délai d'inactivité sur le corps est géré par l'installateur (`install::Installer`).
    download_agent: Agent,
    base: String,
}

impl Client {
    /// `base` : par exemple `https://ludotheque.srvdreamer.fr` ou `http://127.0.0.1:8080`.
    pub fn new(base: &str) -> Self {
        let agent: Agent = Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(3)))
            .timeout_global(Some(Duration::from_secs(600)))
            .build()
            .into();
        let download_agent: Agent = Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(3)))
            .timeout_recv_response(Some(Duration::from_secs(60)))
            .timeout_global(None)
            .build()
            .into();
        Self { agent, download_agent, base: base.trim_end_matches('/').to_string() }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1/{path}", self.base)
    }

    fn fail(status: u16, resp: &mut ureq::http::Response<ureq::Body>) -> ApiError {
        let body = resp.body_mut().read_to_string().unwrap_or_default();
        ApiError::Http { status, body }
    }

    pub fn health(&self) -> Result<(), ApiError> {
        let mut r = self.agent.get(self.url("health")).call()?;
        match r.status().as_u16() {
            200 => Ok(()),
            s => Err(Self::fail(s, &mut r)),
        }
    }

    pub fn head(&self, key: &SaveKey) -> Result<Option<SaveRevision>, ApiError> {
        let mut r = self.agent.get(self.url(&format!("saves/{}/{}", key.profile, key.game))).call()?;
        match r.status().as_u16() {
            200 => r.body_mut().read_json().map(Some).map_err(|e| ApiError::Decode(e.to_string())),
            404 => Ok(None),
            s => Err(Self::fail(s, &mut r)),
        }
    }

    pub fn history(&self, key: &SaveKey) -> Result<Vec<SaveRevision>, ApiError> {
        let mut r = self.agent.get(self.url(&format!("saves/{}/{}/history", key.profile, key.game))).call()?;
        match r.status().as_u16() {
            200 => r.body_mut().read_json().map_err(|e| ApiError::Decode(e.to_string())),
            s => Err(Self::fail(s, &mut r)),
        }
    }

    pub fn blob(&self, key: &SaveKey, rev: u64) -> Result<Vec<u8>, ApiError> {
        let mut r = self.agent.get(self.url(&format!("saves/{}/{}/{rev}", key.profile, key.game))).call()?;
        match r.status().as_u16() {
            200 => r
                .body_mut()
                .with_config()
                .limit(MAX_DOWNLOAD)
                .read_to_vec()
                .map_err(|e| ApiError::Offline(e.to_string())),
            s => Err(Self::fail(s, &mut r)),
        }
    }

    pub fn push(&self, key: &SaveKey, p: &PushParams, archive: &[u8]) -> Result<PushOutcome, ApiError> {
        let mut r = self
            .agent
            .put(self.url(&format!("saves/{}/{}", key.profile, key.game)))
            .query("base_rev", p.base_rev.to_string())
            .query("device", &p.device)
            .query("emulator", &p.emulator)
            .query("playtime_s", p.playtime_s.to_string())
            .query("discarded", p.discarded.to_string())
            .send(archive)?;
        match r.status().as_u16() {
            200 | 201 | 409 => r.body_mut().read_json().map_err(|e| ApiError::Decode(e.to_string())),
            s => Err(Self::fail(s, &mut r)),
        }
    }

    pub fn acquire_lease(&self, key: &SaveKey, device: &str, force: bool) -> Result<LeaseOutcome, ApiError> {
        let req = LeaseRequest { device: device.into(), force };
        let mut r = self
            .agent
            .post(self.url(&format!("leases/{}/{}", key.profile, key.game)))
            .send_json(&req)?;
        match r.status().as_u16() {
            200 | 423 => r.body_mut().read_json().map_err(|e| ApiError::Decode(e.to_string())),
            s => Err(Self::fail(s, &mut r)),
        }
    }

    pub fn release_lease(&self, key: &SaveKey, device: &str) -> Result<(), ApiError> {
        let mut r = self
            .agent
            .delete(self.url(&format!("leases/{}/{}", key.profile, key.game)))
            .query("device", device)
            .call()?;
        match r.status().as_u16() {
            204 | 200 => Ok(()),
            s => Err(Self::fail(s, &mut r)),
        }
    }

    /// Profils du foyer, dans l'ordre de leur création.
    pub fn profiles(&self) -> Result<Vec<Profile>, ApiError> {
        let mut r = self.agent.get(self.url("profiles")).call()?;
        match r.status().as_u16() {
            200 => r.body_mut().read_json().map_err(|e| ApiError::Decode(e.to_string())),
            s => Err(Self::fail(s, &mut r)),
        }
    }

    /// Crée ou met à jour un profil ; renvoie le profil tel que le serveur l'a enregistré
    /// (nom sans espaces autour). Un profil invalide est refusé (`Http { status: 400, .. }`).
    pub fn put_profile(&self, profile: &Profile) -> Result<Profile, ApiError> {
        let mut r = self.agent.put(self.url(&format!("profiles/{}", profile.id))).send_json(profile)?;
        match r.status().as_u16() {
            200 => r.body_mut().read_json().map_err(|e| ApiError::Decode(e.to_string())),
            s => Err(Self::fail(s, &mut r)),
        }
    }

    /// Catalogue des jeux du serveur (titres, jaquettes, fichiers).
    pub fn games(&self) -> Result<Vec<CatalogGame>, ApiError> {
        let mut r = self.agent.get(self.url("games")).call()?;
        match r.status().as_u16() {
            200 => r.body_mut().read_json().map_err(|e| ApiError::Decode(e.to_string())),
            s => Err(Self::fail(s, &mut r)),
        }
    }

    /// Têtes de sauvegarde d'un profil (une par jeu), en une seule requête.
    pub fn save_summaries(&self, profile: &str) -> Result<Vec<SaveSummary>, ApiError> {
        let mut r = self.agent.get(self.url(&format!("saves/{profile}"))).call()?;
        match r.status().as_u16() {
            200 => r.body_mut().read_json().map_err(|e| ApiError::Decode(e.to_string())),
            s => Err(Self::fail(s, &mut r)),
        }
    }

    /// Demande au serveur de balayer à nouveau son dossier de ROM.
    pub fn rescan(&self) -> Result<RescanReport, ApiError> {
        let mut r = self.agent.post(self.url("catalog/rescan")).send_empty()?;
        match r.status().as_u16() {
            200 => r.body_mut().read_json().map_err(|e| ApiError::Decode(e.to_string())),
            s => Err(Self::fail(s, &mut r)),
        }
    }

    /// Ouvre le téléchargement d'un fichier du catalogue, à partir de l'octet `from` (en-tête
    /// `Range`). Statuts rendus tels quels : 200 (fichier entier, ou tar d'un dossier), 206 (reprise)
    /// et 416 (reprise impossible) ; tout autre statut est une erreur.
    pub fn download(&self, game: &str, file_id: u64, from: u64, if_range: Option<&str>) -> Result<ureq::http::Response<ureq::Body>, ApiError> {
        self.download_as(game, file_id, from, if_range, &[])
    }

    /// Comme [`Client::download`], en indiquant les formats que l'émulateur sait lire (`?formats=`) :
    /// le serveur convertit alors un NSZ / XCZ en NSP / XCI si besoin. Un 202 (conversion en cours,
    /// corps = [`ludo_proto::ConvertProgress`], `Retry-After` en secondes) est rendu tel quel.
    pub fn download_as(&self, game: &str, file_id: u64, from: u64, if_range: Option<&str>, formats: &[String]) -> Result<ureq::http::Response<ureq::Body>, ApiError> {
        let mut url = self.url(&format!("games/{game}/files/{file_id}"));
        if !formats.is_empty() {
            url.push_str(&format!("?formats={}", formats.join(",")));
        }
        let mut req = self.download_agent.get(url);
        if from > 0 {
            req = req.header("Range", format!("bytes={from}-"));
            // Validateur de la reprise : si le fichier a changé, le serveur renvoie tout (200).
            if let Some(v) = if_range {
                req = req.header("If-Range", v);
            }
        }
        let mut r = req.call()?;
        match r.status().as_u16() {
            200 | 202 | 206 | 416 => Ok(r),
            s => Err(Self::fail(s, &mut r)),
        }
    }
}
