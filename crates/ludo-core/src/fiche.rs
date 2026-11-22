//! Fiches d'émulateur (conception 4.6) : un fichier TOML par émulateur, commun à tous les
//! appareils, qui dit comment le lancer, où sont ses sauvegardes et comment il isole les profils.

use serde::Deserialize;
use std::collections::BTreeMap;

/// Façon dont l'émulateur sépare les profils.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileMode {
    /// Profils natifs de l'émulateur (Ryujinx, Eden, Lichii).
    Natifs,
    /// `ludo` active le dossier de sauvegardes du profil avant le lancement (Azahar).
    Dossier,
    /// `ludo` réécrit le chemin des sauvegardes dans la config (melonDS, RetroArch).
    FichierConfig,
    /// Pas de séparation possible : un seul profil autorisé.
    Aucun,
}

/// Réglages propres à un appareil.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DeviceSettings {
    pub bin: String,
    pub data: String,
}

/// Une façon de trouver l'émulateur installé sur un appareil (essayées dans l'ordre de la fiche).
/// Exactement un des quatre moyens est renseigné ; `data` liste les dossiers de données possibles
/// pour cette installation (le premier qui existe est retenu, sinon le premier de la liste).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Candidate {
    /// Application Flatpak (`flatpak run <id>`), installée pour l'utilisateur ou le système.
    pub flatpak: Option<String>,
    /// Motif de nom d'AppImage (`Eden*.AppImage`) cherché dans les dossiers d'applications usuels.
    pub appimage: Option<String>,
    /// Programme cherché dans le `PATH`.
    pub commande: Option<String>,
    /// Exécutable à un emplacement fixe (`~/Applications/publish/Ryujinx`).
    pub fichier: Option<String>,
    #[serde(default)]
    pub data: Vec<String>,
}

/// Une fiche d'émulateur.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Fiche {
    pub id: String,
    pub consoles: Vec<String>,
    pub formats: Vec<String>,
    /// Gabarit de lancement : `{bin}`, `{rom}`, `{profil_index}`, `{profil_uuid}`, `{data}`.
    pub lancement: String,
    pub profils: ProfileMode,
    /// Gabarit du dossier de sauvegarde d'un jeu : `{data}`, `{profil_uuid}`, `{title_id}`,
    /// `{title_id_maj}`, `{title_id_haut}` / `{title_id_bas}` (8 chiffres hexadécimaux de chaque
    /// moitié, 3DS et Wii U), `{code_jeu}` / `{code_jeu_court}` / `{region_gc}` (identifiant ASCII
    /// GameCube ou DS, voir `rom`), `{rom_nom}` (nom du fichier de la ROM sans extension).
    pub sauvegardes: String,
    #[serde(default)]
    pub exclusions: Vec<String>,
    /// Fichiers du jeu quand le dossier de sauvegarde est partagé par plusieurs jeux (gabarits,
    /// jokers `*` et `?`) : seuls ces fichiers sont synchronisés, les autres restent en place.
    #[serde(default)]
    pub fichiers: Vec<String>,
    #[serde(default)]
    pub systeme: Vec<String>,
    /// Réglages relevés à la main par appareil : dernier recours si la détection ne trouve rien
    /// (appareils où elle ne s'applique pas, comme Android).
    #[serde(default)]
    pub appareil: BTreeMap<String, DeviceSettings>,
    /// Moyens de détection, du préféré au moins préféré.
    #[serde(default)]
    pub detection: Vec<Candidate>,
    /// Portage natif d'UN jeu (recompilation statique) : identifiant de titre du seul jeu qu'il
    /// fait tourner. Ses sauvegardes ne sont pas rangées par titre.
    #[serde(default)]
    pub titre: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum FicheError {
    #[error("fiche illisible : {0}")]
    Parse(#[from] toml::de::Error),
    #[error("variable inconnue dans le gabarit : {{{0}}}")]
    UnknownVar(String),
    #[error("gabarit mal formé : {0}")]
    Malformed(String),
    #[error("appareil absent de la fiche : {0}")]
    UnknownDevice(String),
}

impl Fiche {
    pub fn parse(text: &str) -> Result<Self, FicheError> {
        Ok(toml::from_str(text)?)
    }

    /// Réglages de l'appareil, ou erreur s'il n'est pas décrit.
    pub fn device(&self, device: &str) -> Result<&DeviceSettings, FicheError> {
        self.appareil.get(device).ok_or_else(|| FicheError::UnknownDevice(device.into()))
    }
}

/// Remplace les `{variables}` d'un gabarit. Une variable inconnue est une erreur (pas de chemin
/// à moitié rempli qui écrirait au mauvais endroit).
pub fn render(template: &str, vars: &BTreeMap<&str, String>) -> Result<String, FicheError> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('}').ok_or_else(|| FicheError::Malformed(template.into()))?;
        let name = &after[..end];
        let value = vars.get(name).ok_or_else(|| FicheError::UnknownVar(name.into()))?;
        out.push_str(value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RYUJINX: &str = include_str!("../../../fiches/ryujinx.toml");

    #[test]
    fn fiche_ryujinx_valide() {
        let f = Fiche::parse(RYUJINX).unwrap();
        assert_eq!(f.id, "ryujinx");
        assert_eq!(f.profils, ProfileMode::Natifs);
        assert!(f.device("deck").is_ok());
        assert!(f.device("thor").is_err());
    }

    #[test]
    fn rendu_du_gabarit() {
        let vars = BTreeMap::from([("data", "/d".to_string()), ("title_id", "0100".to_string())]);
        assert_eq!(render("{data}/save/{title_id}/", &vars).unwrap(), "/d/save/0100/");
        assert!(matches!(render("{data}/{inconnu}", &vars), Err(FicheError::UnknownVar(v)) if v == "inconnu"));
        assert!(matches!(render("{data", &vars), Err(FicheError::Malformed(_))));
    }

    #[test]
    fn toutes_les_fiches_du_depot_sont_valides() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fiches");
        let mut n = 0;
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "toml") {
                Fiche::parse(&std::fs::read_to_string(&p).unwrap())
                    .unwrap_or_else(|err| panic!("{} : {err}", p.display()));
                n += 1;
            }
        }
        assert!(n >= 1);
    }
}
