//! Configuration d'un appareil (`~/.config/ludo/config.toml`) et construction des adaptateurs
//! à partir des fiches d'émulateur.

use crate::layout::{DirLayout, RyujinxLayout, SaveLayout};
use crate::detect::{self, Host};
use ludo_core::fiche::{render, DeviceSettings, Fiche, FicheError, ProfileMode};
use ludo_proto::CatalogGame;
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

/// Un jeu synchronisé par cet appareil (section `[jeux.<identifiant>]`), en attendant le
/// catalogue du serveur.
#[derive(Debug, Clone, Deserialize)]
pub struct GameConf {
    /// Fiche d'émulateur : ryujinx, eden…
    pub emulateur: String,
    /// Identifiant du titre (16 caractères hexadécimaux).
    pub title_id: String,
    /// Profils concernés ; absent = tous les profils de l'appareil.
    #[serde(default)]
    pub profils: Option<Vec<String>>,
}

impl GameConf {
    /// Ce jeu se synchronise-t-il pour ce profil ?
    pub fn concerns(&self, profile: &str) -> bool {
        self.profils.as_ref().is_none_or(|ps| ps.iter().any(|p| p == profile))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub serveur: String,
    pub appareil: String,
    /// Profil utilisé sans demander (appareil personnel). Absent = appareil partagé.
    pub profil_defaut: Option<String>,
    /// Dossier de fiches qui remplace les fiches intégrées.
    pub fiches: Option<PathBuf>,
    /// Dossier des ROM de cet appareil, même organisation que sur le serveur (`<roms>/switch/…`).
    /// Absent = aucun jeu considéré comme installé.
    pub roms: Option<PathBuf>,
    #[serde(default)]
    pub profils: BTreeMap<String, ProfileIds>,
    /// Jeux synchronisés par cet appareil (identifiant ludothèque → émulateur et titre). Facultatif :
    /// un jeu absent prend l'émulateur par défaut de sa console et le title_id du catalogue.
    #[serde(default)]
    pub jeux: BTreeMap<String, GameConf>,
    /// Émulateur par défaut par console (`switch = "eden"`), en plus de `DEFAULT_EMULATORS`.
    #[serde(default)]
    pub consoles: BTreeMap<String, String>,
    /// Emplacement imposé d'un émulateur sur cet appareil (`[emulateurs.ryujinx] bin = … data = …`) :
    /// prime sur la détection.
    #[serde(default)]
    pub emulateurs: BTreeMap<String, DeviceSettings>,
}

/// Émulateur par défaut de chaque console (décision du 14/11 : Ryujinx pour la Switch).
pub const DEFAULT_EMULATORS: &[(&str, &str)] = &[("switch", "ryujinx")];

/// D'où vient l'emplacement retenu pour un émulateur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// `[emulateurs.<id>]` de la configuration.
    Config,
    /// Trouvé sur l'appareil (le texte dit comment).
    Detected(String),
    /// Section `[appareil.<nom>]` de la fiche (dernier recours).
    Fiche,
}

/// Emplacement retenu pour un émulateur sur cet appareil.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    pub bin: String,
    pub data: PathBuf,
    pub origin: Origin,
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

/// Journal local de synchronisation (partagé par `ludo` et l'interface).
pub fn journal_path() -> PathBuf {
    home().join(".local/share/ludo/journal.json")
}

/// Remplace un `~` initial par le dossier personnel.
pub fn expand(p: &str) -> PathBuf {
    expand_in(&home(), p)
}

/// Remplace un `~` initial par `home`.
pub fn expand_in(home: &Path, p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if p == "~" => home.to_path_buf(),
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

    /// Identifiants de toutes les fiches connues : intégrées, puis celles du dossier `fiches`
    /// (un émulateur nouveau se déclare en y déposant sa fiche, sans reconstruire `ludo`).
    pub fn fiche_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = BUILTIN.iter().map(|(n, _)| n.to_string()).collect();
        if let Some(dir) = &self.fiches {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.extension().is_some_and(|x| x == "toml") {
                        if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                            ids.push(stem.to_string());
                        }
                    }
                }
            }
        }
        ids.sort();
        ids.dedup();
        ids
    }

    /// Émulateur par défaut d'une console : configuration de l'appareil, sinon `DEFAULT_EMULATORS`.
    pub fn default_emulator(&self, console: &str) -> Option<String> {
        self.consoles
            .get(console)
            .cloned()
            .or_else(|| DEFAULT_EMULATORS.iter().find(|(c, _)| *c == console).map(|(_, e)| e.to_string()))
    }

    /// Émulateur et title_id d'un jeu pour ce profil : la section `[jeux.<id>]` si elle existe (et
    /// concerne le profil), sinon l'émulateur par défaut de la console avec le title_id du catalogue.
    /// `None` : jeu non jouable ici (console sans émulateur, title_id inconnu, autre profil).
    pub fn game_conf(&self, game: &str, profile: &str, catalog: Option<&CatalogGame>) -> Option<GameConf> {
        if let Some(g) = self.jeux.get(game) {
            return g.concerns(profile).then(|| g.clone());
        }
        let c = catalog.filter(|c| c.id == game)?;
        let emulateur = self.default_emulator(&c.console)?;
        let title_id = c.title_id.clone().filter(|t| t.len() == 16 && u64::from_str_radix(t, 16).is_ok())?;
        Some(GameConf { emulateur, title_id: title_id.to_lowercase(), profils: None })
    }

    /// Emplacement d'un émulateur sur cet appareil : `[emulateurs.<id>]`, sinon détection, sinon la
    /// section `[appareil.<nom>]` de la fiche.
    pub fn locate(&self, fiche: &Fiche, host: &Host) -> Result<Located, ConfigError> {
        if let Some(s) = self.emulateurs.get(&fiche.id) {
            return Ok(Located { bin: s.bin.clone(), data: expand_in(&host.home, &s.data), origin: Origin::Config });
        }
        if let Some(f) = detect::detect(fiche, host) {
            return Ok(Located { bin: f.bin, data: f.data, origin: Origin::Detected(f.source) });
        }
        let dev = fiche.device(&self.appareil)?;
        Ok(Located { bin: dev.bin.clone(), data: expand_in(&host.home, &dev.data), origin: Origin::Fiche })
    }

    pub fn profile(&self, name: &str) -> Result<&ProfileIds, ConfigError> {
        self.profils.get(name).ok_or_else(|| ConfigError::UnknownProfile(name.into()))
    }

    /// Jeux configurés sur cet appareil pour ce profil (ordre des identifiants).
    pub fn games_for<'a>(&'a self, profile: &'a str) -> impl Iterator<Item = (&'a String, &'a GameConf)> + 'a {
        self.jeux.iter().filter(move |(_, g)| g.concerns(profile))
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
    let loc = cfg.locate(&fiche, &Host::current())?;
    let ids = cfg.profile(profile)?;
    let tid = u64::from_str_radix(title_id, 16).map_err(|_| ConfigError::BadTitleId(title_id.into()))?;
    if title_id.len() != 16 {
        return Err(ConfigError::BadTitleId(title_id.into()));
    }
    let data = loc.data;

    let mut vars: BTreeMap<&'static str, String> = BTreeMap::new();
    vars.insert("bin", loc.bin);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jeux_configures_par_profil() {
        let cfg: Config = toml::from_str(
            r#"
            serveur = "http://127.0.0.1:8080"
            appareil = "steam-deck"

            [jeux.switch-mk8]
            emulateur = "ryujinx"
            title_id = "0100152000022000"

            [jeux.switch-lm3]
            emulateur = "eden"
            title_id = "0100dca0064a6000"
            profils = ["claire"]
            "#,
        )
        .unwrap();
        let ids = |p| cfg.games_for(p).map(|(id, _)| id.as_str()).collect::<Vec<_>>();
        assert_eq!(ids("claire"), vec!["switch-lm3", "switch-mk8"]);
        assert_eq!(ids("guilhem"), vec!["switch-mk8"]);
        // Configuration existante sans section `jeux` : toujours lisible.
        let old: Config = toml::from_str("serveur = \"x\"\nappareil = \"pc\"").unwrap();
        assert!(old.jeux.is_empty() && old.roms.is_none());
        let with: Config = toml::from_str("serveur = \"x\"\nappareil = \"pc\"\nroms = \"/jeux/roms\"").unwrap();
        assert_eq!(with.roms, Some(PathBuf::from("/jeux/roms")));
    }

    fn catalogue(id: &str, console: &str, title_id: Option<&str>) -> CatalogGame {
        serde_json::from_value(serde_json::json!({
            "id": id, "console": console, "title": id, "short": id, "title_id": title_id,
            "art": { "motif": "x", "from": "#000", "to": "#000", "ink": "#fff" },
            "catalogued": true, "files": []
        }))
        .unwrap()
    }

    #[test]
    fn emulateur_par_defaut_et_section_jeux() {
        let mut cfg: Config = toml::from_str(
            "serveur = \"x\"\nappareil = \"pc\"\n[jeux.switch-a]\nemulateur = \"eden\"\ntitle_id = \"0100152000022000\"\nprofils = [\"claire\"]\n",
        )
        .unwrap();
        let tloz = catalogue("switch-b", "switch", Some("0100F2C0115B6000"));
        // Défaut intégré : Ryujinx pour la Switch, title_id du catalogue (minuscules).
        let g = cfg.game_conf("switch-b", "guilhem", Some(&tloz)).unwrap();
        assert_eq!((g.emulateur.as_str(), g.title_id.as_str()), ("ryujinx", "0100f2c0115b6000"));
        // La section [jeux] l'emporte, et garde sa restriction de profil.
        assert_eq!(cfg.game_conf("switch-a", "claire", None).unwrap().emulateur, "eden");
        assert!(cfg.game_conf("switch-a", "guilhem", Some(&catalogue("switch-a", "switch", Some("0100152000022000")))).is_none());
        // Console sans émulateur, title_id absent ou invalide, catalogue d'un autre jeu : non jouable.
        assert!(cfg.game_conf("3ds-x", "guilhem", Some(&catalogue("3ds-x", "3ds", None))).is_none());
        assert!(cfg.game_conf("switch-c", "guilhem", Some(&catalogue("switch-c", "switch", None))).is_none());
        assert!(cfg.game_conf("switch-d", "guilhem", Some(&catalogue("switch-d", "switch", Some("pas-hexa-du-tout")))).is_none());
        assert!(cfg.game_conf("switch-b", "guilhem", Some(&catalogue("autre", "switch", Some("0100F2C0115B6000")))).is_none());
        // Défaut de l'appareil : remplace le défaut intégré.
        cfg.consoles.insert("switch".into(), "eden".into());
        assert_eq!(cfg.game_conf("switch-b", "guilhem", Some(&tloz)).unwrap().emulateur, "eden");
    }

    #[test]
    fn emplacement_config_puis_detection_puis_fiche() {
        let t = tempfile::tempdir().unwrap();
        let host = Host { home: t.path().into(), path: None, system_flatpak: t.path().join("sys") };
        let mut cfg: Config = toml::from_str("serveur = \"x\"\nappareil = \"pc\"").unwrap();
        let fiche = cfg.fiche("ryujinx").unwrap();
        // Rien d'installé : section [appareil.pc] de la fiche.
        let l = cfg.locate(&fiche, &host).unwrap();
        assert_eq!((l.origin, l.bin.as_str()), (Origin::Fiche, "flatpak run io.github.ryubing.Ryujinx"));
        // Flatpak installé : détecté, mêmes chemins de données que la fiche (sauvegardes au même endroit).
        std::fs::create_dir_all(t.path().join(".local/share/flatpak/app/io.github.ryubing.Ryujinx")).unwrap();
        let l = cfg.locate(&fiche, &host).unwrap();
        assert!(matches!(l.origin, Origin::Detected(_)));
        assert_eq!(l.data, t.path().join(".var/app/io.github.ryubing.Ryujinx/config/Ryujinx"));
        // Emplacement imposé : prime sur tout.
        cfg.emulateurs.insert("ryujinx".into(), DeviceSettings { bin: "/opt/ryu".into(), data: "~/ryu".into() });
        let l = cfg.locate(&fiche, &host).unwrap();
        assert_eq!((l.origin, l.bin.as_str(), l.data), (Origin::Config, "/opt/ryu", t.path().join("ryu")));
        // Appareil inconnu de la fiche et rien détecté : erreur.
        let thor: Config = toml::from_str("serveur = \"x\"\nappareil = \"thor\"").unwrap();
        let empty = Host { home: t.path().join("vide"), path: None, system_flatpak: t.path().join("sys") };
        assert!(thor.locate(&fiche, &empty).is_err());
    }

    #[test]
    fn fiches_du_dossier_ajoutees_aux_integrees() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("azahar.toml"), "").unwrap();
        std::fs::write(t.path().join("notes.txt"), "").unwrap();
        let mut cfg: Config = toml::from_str("serveur = \"x\"\nappareil = \"pc\"").unwrap();
        assert_eq!(cfg.fiche_ids(), ["eden", "ryujinx"]);
        cfg.fiches = Some(t.path().into());
        assert_eq!(cfg.fiche_ids(), ["azahar", "eden", "ryujinx"]);
    }
}
