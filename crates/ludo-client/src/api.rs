//! Client HTTP synchrone du serveur de la ludothèque.

use ludo_proto::{LeaseOutcome, LeaseRequest, Profile, PushOutcome, PushParams, SaveKey, SaveRevision};
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
        Self { agent, base: base.trim_end_matches('/').to_string() }
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
}
