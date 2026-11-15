//! Types partagés entre le serveur `ludo-server` et les clients (`ludo`, `ludod`, app Android).
//!
//! Le protocole suit le modèle Steam Cloud : une sauvegarde par (profil, jeu), sous forme neutre,
//! versionnée par un numéro de révision côté serveur. Un envoi précise la révision dont il descend
//! (`base_rev`) ; le serveur ne l'accepte que si sa tête n'a pas bougé (compare-and-swap).
//! L'heure des appareils n'intervient jamais dans la logique : elle ne sert qu'à l'affichage.

use serde::{Deserialize, Serialize};

/// Version de l'API, préfixe des routes (`/api/v1/...`).
pub const API_VERSION: &str = "v1";

/// Identifiant d'un profil (« guilhem », « claire »).
pub type ProfileId = String;
/// Identifiant d'un jeu dans le catalogue (ex. `switch-0100152000022000`).
pub type GameId = String;
/// Identifiant d'un appareil (« steam-deck », « pc », « thor »).
pub type DeviceId = String;
/// Numéro de révision d'une sauvegarde. 0 = aucune sauvegarde sur le serveur.
pub type Revision = u64;
/// Empreinte SHA-256 (hexadécimal) de l'archive neutre d'une sauvegarde.
pub type ContentHash = String;

/// Clé d'une sauvegarde : un profil et un jeu.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SaveKey {
    pub profile: ProfileId,
    pub game: GameId,
}

/// Description d'une révision de sauvegarde stockée sur le serveur.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveRevision {
    pub rev: Revision,
    /// Révision dont celle-ci descend (0 pour la première).
    pub parent: Revision,
    pub hash: ContentHash,
    pub size: u64,
    pub device: DeviceId,
    /// Émulateur qui a produit la sauvegarde (affichage et diagnostic seulement).
    pub emulator: String,
    /// Temps de jeu cumulé déclaré par le client, en secondes.
    pub playtime_s: u64,
    /// Horodatage serveur (secondes Unix), pour l'affichage uniquement.
    pub created_at: u64,
    /// Version écartée lors d'un conflit : gardée dans l'historique, jamais tête.
    #[serde(default)]
    pub discarded: bool,
}

/// Réponse à un envoi de sauvegarde.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum PushOutcome {
    /// La sauvegarde devient la nouvelle tête.
    Accepted { revision: SaveRevision },
    /// Le contenu est identique à la tête : rien n'a été créé.
    Unchanged { revision: SaveRevision },
    /// La tête a bougé depuis `base_rev` : conflit à trancher par le joueur.
    Conflict { head: SaveRevision },
}

/// Paramètres d'un envoi (query string ; le corps est l'archive neutre).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushParams {
    pub base_rev: Revision,
    pub device: DeviceId,
    pub emulator: String,
    pub playtime_s: u64,
    /// Archiver une version écartée lors d'un conflit (« Utiliser l'autre sauvegarde ») :
    /// elle entre dans l'historique sans devenir la tête, quelle que soit `base_rev`.
    #[serde(default)]
    pub discarded: bool,
}

/// Verrou souple « partie en cours » sur (profil, jeu).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    pub key: SaveKey,
    pub device: DeviceId,
    /// Expiration (secondes Unix, horloge du serveur).
    pub expires_at: u64,
}

/// Demande de prise ou de renouvellement de verrou.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseRequest {
    pub device: DeviceId,
    /// Forcer la prise même si un autre appareil le tient (« Jouer ici »).
    #[serde(default)]
    pub force: bool,
}

/// Réponse à une demande de verrou.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum LeaseOutcome {
    Granted { lease: Lease },
    /// Un autre appareil joue déjà avec ce profil à ce jeu.
    HeldElsewhere { lease: Lease },
}

/// Durée de vie d'un verrou sans battement, en secondes (conception 4.4).
pub const LEASE_TTL_S: u64 = 120;

/// Personnages illustrés autorisés pour l'image d'un profil.
pub const AVATAR_KINDS: &[&str] = &["renard", "champi", "chat", "etoile", "planete", "poisson"];

/// Longueur maximale du nom d'un profil, en caractères (après suppression des espaces autour).
pub const PROFILE_NAME_MAX: usize = 16;

/// Préférences d'un profil : elles suivent la personne sur tous ses appareils.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfilePrefs {
    pub sons: bool,
    pub vibrations: bool,
}

impl Default for ProfilePrefs {
    fn default() -> Self {
        Self { sons: true, vibrations: true }
    }
}

/// Profil d'une personne du foyer, tel que l'interface le manipule (champs en camelCase).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: ProfileId,
    pub name: String,
    /// Personnage illustré (une valeur de [`AVATAR_KINDS`]) ; absent = initiale sur l'accent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_kind: Option<String>,
    pub accent: String,
    pub accent_soft: String,
    pub accent_ink: String,
    #[serde(default)]
    pub prefs: ProfilePrefs,
}
