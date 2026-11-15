//! Configuration d'un appareil (`~/.config/ludo/config.toml`) et construction des adaptateurs
//! à partir des fiches d'émulateur.

use crate::layout::{DirLayout, RyujinxLayout, SaveLayout};
use ludo_core::fiche::{render, Fiche, FicheError, ProfileMode};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Identité d'un profil dans chaque émulateur de cet appareil.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProfileIds {
    /// `user_id` du profil dans `Profiles.json` de Ryujinx.
    pub ryujinx_user_id: Option<String>,
    /// Nom du profil Ryujinx (option `--profile`).
    pub ryujinx_nom: Option<String>,
    /// Dossier de profil Eden/Lichii (`nand/user/save/0000000000000000/<UUID>`).
    pub eden_uuid: Option<String>,
    /// Index du profil Eden/Lichii (option `-u`).
    pub eden_index: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub serveur: String,
    pub appareil: String,
    /// Profil utilisé sans demander (appareil personnel). Absent = appareil partagé.
    pub profil_defaut: Option<String>,
    /// Dossier de fiches qui remplace les fiches intégrées.
    pub fiches: Option<PathBuf>,
    #[serde(default)]
    pub profils: BTreeMap<String, ProfileIds>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("configuration illisible : {0}")]
    Parse(#[from] toml::de::Error),
    #[error("entrée-sortie : {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Fiche(#[from] FicheError),
    #[error("fiche inconnue : {0}")]
    UnknownFiche(String),
    #[error("profil inconnu sur cet appareil : {0}")]
    UnknownProfile(String),
    #[error("identifiant manquant pour le profil {profile} : {field}")]
    MissingId { profile: String, field: &'static str },
    #[error("identifiant de jeu invalide (16 caractères hexadécimaux attendus) : {0}")]
    BadTitleId(String),
}

/// Fiches livrées avec `ludo`.
const BUILTIN: &[(&str, &str)] = &[
    ("ryujinx", include_str!("../../../fiches/ryujinx.toml")),
    ("eden", include_str!("../../../fiches/eden.toml")),
];

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

/// Remplace un `~` initial par le dossier personnel.
pub fn expand(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None if p == "~" => home(),
        None => PathBuf::from(p),
    }
}

impl Config {
    pub fn default_path() -> PathBuf {
        home().join(".config/ludo/config.toml")
    }

    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        Ok(toml::from_str(&std::fs::read_to_string(path)?)?)
    }

    pub fn fiche(&self, id: &str) -> Result<Fiche, ConfigError> {
        if let Some(dir) = &self.fiches {
            let p = dir.join(format!("{id}.toml"));
            if p.exists() {
                return Ok(Fiche::parse(&std::fs::read_to_string(p)?)?);
            }
        }
        let text = BUILTIN.iter().find(|(n, _)| *n == id).map(|(_, t)| *t).ok_or_else(|| ConfigError::UnknownFiche(id.into()))?;
        Ok(Fiche::parse(text)?)
    }

    pub fn profile(&self, name: &str) -> Result<&ProfileIds, ConfigError> {
        self.profils.get(name).ok_or_else(|| ConfigError::UnknownProfile(name.into()))
    }
}

/// Tout ce qu'il faut pour sauvegarder et lancer un jeu avec un émulateur sur cet appareil.
pub struct Prepared {
    pub layout: Box<dyn SaveLayout>,
    pub vars: BTreeMap<&'static str, String>,
    pub fiche: Fiche,
}

fn need<T: Clone>(v: &Option<T>, profile: &str, field: &'static str) -> Result<T, ConfigError> {
    v.clone().ok_or(ConfigError::MissingId { profile: profile.into(), field })
}

/// Construit l'adaptateur et les variables de gabarit pour (émulateur, profil, jeu).
pub fn prepare(cfg: &Config, emulator: &str, profile: &str, title_id: &str) -> Result<Prepared, ConfigError> {
    let fiche = cfg.fiche(emulator)?;
    let dev = fiche.device(&cfg.appareil)?;
    let ids = cfg.profile(profile)?;
    let tid = u64::from_str_radix(title_id, 16).map_err(|_| ConfigError::BadTitleId(title_id.into()))?;
    if title_id.len() != 16 {
        return Err(ConfigError::BadTitleId(title_id.into()));
    }
    let data = expand(&dev.data);

    let mut vars: BTreeMap<&'static str, String> = BTreeMap::new();
    vars.insert("bin", dev.bin.clone());
    vars.insert("data", data.display().to_string());
    vars.insert("profil", profile.into());
    vars.insert("title_id", format!("{tid:016x}"));
    vars.insert("title_id_maj", format!("{tid:016X}"));

    let layout: Box<dyn SaveLayout> = if fiche.id == "ryujinx" {
        let uid = need(&ids.ryujinx_user_id, profile, "ryujinx_user_id")?;
        vars.insert("profil_nom", need(&ids.ryujinx_nom, profile, "ryujinx_nom")?);
        Box::new(RyujinxLayout { save_root: data.join("bis/user/save"), title_id: tid, user_id: uid, exclude: fiche.exclusions.clone() })
    } else {
        if fiche.profils == ProfileMode::Natifs {
            vars.insert("profil_uuid", need(&ids.eden_uuid, profile, "eden_uuid")?);
            vars.insert("profil_index", need(&ids.eden_index, profile, "eden_index")?.to_string());
        }
        let account = expand(&render(&fiche.sauvegardes, &vars)?);
        // Sauvegarde « appareil » des émulateurs à profils natifs : UUID nul.
        let device = if fiche.profils == ProfileMode::Natifs {
            let mut dv = vars.clone();
            dv.insert("profil_uuid", "0".repeat(32));
            Some(expand(&render(&fiche.sauvegardes, &dv)?))
        } else {
            None
        };
        Box::new(DirLayout { account, device, exclude: fiche.exclusions.clone() })
    };
    Ok(Prepared { layout, vars, fiche })
}

/// Ligne de commande de lancement, découpée en arguments.
pub fn launch_command(p: &Prepared, rom: &Path) -> Result<Vec<String>, ConfigError> {
    let mut out = Vec::new();
    for part in p.fiche.lancement.split_whitespace() {
        let mut vars = p.vars.clone();
        vars.insert("rom", rom.display().to_string());
        let rendered = render(part, &vars)?;
        // `{bin}` peut contenir plusieurs mots (« flatpak run … »).
        if part == "{bin}" {
            out.extend(rendered.split_whitespace().map(|s| expand(s).display().to_string()));
        } else {
            out.push(rendered);
        }
    }
    Ok(out)
}
