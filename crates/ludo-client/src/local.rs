//! Jeux déjà présents sur l'appareil HORS du dossier de la Ludothèque : la collection existante
//! (dossiers déclarés `[dossiers]`, dossiers de jeux réglés dans Ryujinx et Eden, dossiers EmuDeck).
//!
//! Un fichier du catalogue est reconnu par son NOM (sans le dossier), en tenant pour équivalents
//! un format compressé et sa forme convertie (`.nsz` ≈ `.nsp`, `.xcz` ≈ `.xci`) : c'est ainsi que
//! la collection du serveur a été constituée à partir de celle du PC. Lecture seule : la Ludothèque
//! lance ces fichiers mais ne les déplace ni ne les supprime jamais.

use crate::config::{expand_in, Config};
use crate::detect::Host;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Profondeur de recherche sous chaque dossier (`cartoucheSwitch/MAJ-DLC/…` = 2).
const MAX_DEPTH: usize = 4;

/// Formats équivalents : compressé → forme lue par les émulateurs.
const EQUIVALENT: &[(&str, &str)] = &[("nsz", "nsp"), ("xcz", "xci")];

/// Dossiers EmuDeck par console (Steam Deck), sous `~/Emulation/roms/`.
const EMUDECK: &[(&str, &str)] = &[("switch", "switch"), ("3ds", "n3ds"), ("ds", "nds"), ("wiiu", "wiiu"), ("gc", "gc")];

/// Clé de rapprochement d'un nom de fichier : (nom sans extension, extension normalisée), en minuscules.
pub fn match_key(name: &str) -> (String, String) {
    let name = name.trim_end_matches('/');
    let base = name.rsplit('/').next().unwrap_or(name);
    let (stem, ext) = match base.rsplit_once('.') {
        // Un nom de dossier peut contenir des points sans avoir d'extension (« Super Mario 3D Land (Rev 1.2) »).
        Some((s, e)) if !e.is_empty() && e.len() <= 4 && e.chars().all(|c| c.is_ascii_alphanumeric()) => (s, e.to_lowercase()),
        _ => (base, String::new()),
    };
    let ext = EQUIVALENT.iter().find(|(from, _)| *from == ext).map_or(ext.clone(), |(_, to)| to.to_string());
    (stem.to_lowercase(), ext)
}

/// Clé de correspondance souple entre deux copies d'une même ROM nommées différemment : préfixe de
/// numérotation « 3DS1626 - » retiré, ponctuation et espaces ramenés à un seul espace, casse ignorée,
/// extension retirée. Relevé le 21/11 : le serveur range « 3DS1626 - Animal Crossing - New Leaf -
/// Welcome Amiibo (Europe)(En,Fr,De,Es,It).cxi », la Thor « Animal Crossing - New Leaf - Welcome
/// Amiibo (Europe) (En,Fr,De,Es,It).cxi ».
pub fn loose_key(name: &str) -> String {
    let (stem, _) = match_key(name);
    let words: Vec<&str> = stem.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let skip = usize::from(words.first().is_some_and(|w| w.len() > 3 && w.starts_with("3ds") && w[3..].chars().all(|c| c.is_ascii_digit())));
    words[skip..].join(" ")
}

/// Identité Switch d'un nom de fichier : `[<titre 16 hex>]` et `[v<version>]` (absente = v0).
/// Deux copies du même jeu portent souvent des noms différents (« (nsw2u.xyz) », « [v0] » omis) ;
/// l'identifiant de titre et la version, eux, ne changent pas. Une mise à jour d'une autre
/// version n'est donc jamais confondue.
pub fn title_key(name: &str) -> Option<(String, String)> {
    let base = name.trim_end_matches('/').rsplit('/').next().unwrap_or(name);
    let mut tid = None;
    let mut version = None;
    for part in base.split('[').skip(1) {
        let inner = part.split(']').next().unwrap_or("");
        if tid.is_none() && inner.len() == 16 && inner.chars().all(|c| c.is_ascii_hexdigit()) {
            tid = Some(inner.to_ascii_uppercase());
        } else if version.is_none() && inner.len() > 1 && inner.starts_with(['v', 'V']) && inner[1..].chars().all(|c| c.is_ascii_digit()) {
            version = Some(inner[1..].trim_start_matches('0').to_string());
        }
    }
    Some((tid?, version.filter(|v| !v.is_empty()).unwrap_or_else(|| "0".into())))
}

/// Index des fichiers de jeu présents sur l'appareil, par console.
#[derive(Debug, Default, Clone)]
pub struct LocalIndex {
    by_console: BTreeMap<String, BTreeMap<(String, String), PathBuf>>,
    /// Même index par identité Switch (titre + version), pour les noms de fichier différents.
    by_title: BTreeMap<String, BTreeMap<(String, String), PathBuf>>,
    /// Dossiers parcourus (affichés dans les Réglages, pour comprendre ce qui est vu).
    pub roots: Vec<(String, PathBuf)>,
}

impl LocalIndex {
    /// Parcourt les dossiers connus pour cet appareil.
    pub fn scan(cfg: &Config, host: &Host) -> Self {
        let mut idx = LocalIndex::default();
        if !cfg.collection {
            return idx;
        }
        for (console, dir) in roots(cfg, host) {
            idx.add_root(&console, &dir);
        }
        idx
    }

    /// Ajoute un dossier (ignoré s'il n'existe pas ou s'il est déjà parcouru pour cette console).
    pub fn add_root(&mut self, console: &str, dir: &Path) {
        if !dir.is_dir() || self.roots.iter().any(|(c, d)| c == console && d == dir) {
            return;
        }
        self.roots.push((console.to_string(), dir.to_path_buf()));
        let files = self.by_console.entry(console.to_string()).or_default();
        walk(dir, 0, files);
        let titles = self.by_title.entry(console.to_string()).or_default();
        for (key, path) in files.iter() {
            if let Some(t) = title_key(&format!("{}.{}", key.0, key.1)) {
                titles.entry(t).or_insert_with(|| path.clone());
            }
        }
    }

    /// Fichier local correspondant à un chemin du catalogue (`switch/MAJ-DLC/x [id][v1].nsz`).
    pub fn find(&self, console: &str, catalog_rel: &str) -> Option<&PathBuf> {
        self.by_console
            .get(console)?
            .get(&match_key(catalog_rel))
            .or_else(|| self.by_title.get(console)?.get(&title_key(catalog_rel)?))
    }

    /// Fichier du catalogue présent sur l'appareil : dans le dossier de la Ludothèque (`roms`), tel
    /// quel ou converti par le serveur, sinon dans la collection existante. La console est le premier
    /// dossier du chemin du catalogue (`switch/…`, `3ds/…`).
    pub fn locate(&self, roms: Option<&Path>, catalog_rel: &str) -> Option<PathBuf> {
        if let Some(r) = roms {
            let direct = r.join(catalog_rel);
            if direct.exists() {
                return Some(direct);
            }
            if let Some(c) = crate::install::converted_rel(catalog_rel).map(|c| r.join(c)).filter(|p| p.exists()) {
                return Some(c);
            }
        }
        let console = catalog_rel.split('/').next()?;
        self.find(console, catalog_rel).cloned()
    }
}

fn walk(dir: &Path, depth: usize, out: &mut BTreeMap<(String, String), PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.ends_with(".part") {
            continue;
        }
        let path = e.path();
        // Le premier trouvé l'emporte (ordre des dossiers : déclarés, puis émulateurs, puis EmuDeck).
        out.entry(match_key(&name)).or_insert_with(|| path.clone());
        if depth + 1 < MAX_DEPTH && path.is_dir() {
            walk(&path, depth + 1, out);
        }
    }
}

/// Dossiers à parcourir : `[dossiers]` de la configuration, dossiers de jeux des émulateurs Switch
/// (réglés dans Ryujinx et Eden), dossiers EmuDeck existants.
pub fn roots(cfg: &Config, host: &Host) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for (console, dirs) in &cfg.dossiers {
        out.extend(dirs.iter().map(|d| (console.clone(), expand_in(&host.home, d))));
    }
    for id in ["ryujinx", "eden"] {
        let Ok(fiche) = cfg.fiche(id) else { continue };
        let Ok(loc) = cfg.locate(&fiche, host) else { continue };
        let dirs = match id {
            "ryujinx" => ryujinx_game_dirs(&loc.data),
            _ => eden_game_dirs(&loc.data),
        };
        for c in &fiche.consoles {
            out.extend(dirs.iter().map(|d| (c.clone(), d.clone())));
        }
    }
    for (console, sub) in EMUDECK {
        out.push((console.to_string(), host.home.join("Emulation/roms").join(sub)));
    }
    out
}

/// `game_dirs` de `Config.json` de Ryujinx (dossier de données).
pub fn ryujinx_game_dirs(data: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(data.join("Config.json")) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return Vec::new() };
    v.get("game_dirs").and_then(|d| d.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).map(PathBuf::from).collect()).unwrap_or_default()
}

/// `Paths\gamedirs\N\path` de `qt-config.ini` d'Eden. Le fichier est dans le dossier de configuration,
/// voisin du dossier de données (`…/data/eden` → `…/config/eden`, `~/.local/share/eden` → `~/.config/eden`).
pub fn eden_game_dirs(data: &Path) -> Vec<PathBuf> {
    let s = data.to_string_lossy();
    let candidates = [s.replace("/data/eden", "/config/eden"), s.replace("/.local/share/eden", "/.config/eden"), s.to_string()];
    let Some(text) = candidates.iter().find_map(|c| std::fs::read_to_string(Path::new(c).join("qt-config.ini")).ok()) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            let k = k.trim();
            (k.starts_with("Paths\\gamedirs\\") && k.ends_with("\\path") && !v.trim().is_empty()).then(|| PathBuf::from(v.trim()))
        })
        // Entrées spéciales d'Eden (« SDMC », « UserNAND », « SysNAND ») : pas des dossiers.
        .filter(|p| p.is_absolute())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cle_de_rapprochement() {
        assert_eq!(match_key("switch/MAJ-DLC/Jeu [0100F2C0115B6800][v131072] (MAJ 1.1.0).nsz"), ("jeu [0100f2c0115b6800][v131072] (maj 1.1.0)".into(), "nsp".into()));
        assert_eq!(match_key("Jeu [0100F2C0115B6800][v131072] (MAJ 1.1.0).nsp"), match_key("x/Jeu [0100F2C0115B6800][v131072] (MAJ 1.1.0).nsz"));
        assert_eq!(match_key("switch/Zelda [0100F2C0115B6000][v0].xcz").1, "xci");
        // Dossier (jeu Switch 2 extrait) : pas d'extension, même avec des points dans le nom.
        assert_eq!(match_key("switch2/Pokemon Pokopia [0400A3A01EE4C000]"), ("pokemon pokopia [0400a3a01ee4c000]".into(), String::new()));
        assert_eq!(match_key("3ds/Super Mario 3D Land (Rev 1.2)").1, "");
        assert_eq!(match_key("3ds/Jeu (Europe).cxi").1, "cxi");
    }

    #[test]
    fn index_retrouve_par_nom_et_format_equivalent() {
        let t = tempfile::tempdir().unwrap();
        let sw = t.path().join("cartoucheSwitch");
        std::fs::create_dir_all(sw.join("MAJ-DLC")).unwrap();
        std::fs::write(sw.join("Zelda [0100F2C0115B6000][v0].xci"), b"x").unwrap();
        std::fs::write(sw.join("MAJ-DLC/Zelda [0100F2C0115B6800][v131072].nsp"), b"x").unwrap();
        std::fs::write(sw.join(".cache.nsp"), b"x").unwrap();
        let mut idx = LocalIndex::default();
        idx.add_root("switch", &sw);
        idx.add_root("switch", &t.path().join("absent"));
        assert_eq!(idx.find("switch", "switch/Zelda [0100F2C0115B6000][v0].xcz"), Some(&sw.join("Zelda [0100F2C0115B6000][v0].xci")));
        assert!(idx.find("switch", "switch/MAJ-DLC/Zelda [0100F2C0115B6800][v131072].nsz").is_some());
        // Autre console, autre nom : rien.
        assert!(idx.find("3ds", "3ds/Zelda [0100F2C0115B6000][v0].xcz").is_none());
        assert!(idx.find("switch", "switch/Autre [0100000000000000][v0].nsz").is_none());
        assert_eq!(idx.roots.len(), 1, "un dossier absent n'est pas compté");
    }

    #[test]
    fn dossier_ludotheque_puis_collection() {
        let t = tempfile::tempdir().unwrap();
        let roms = t.path().join("roms");
        std::fs::create_dir_all(roms.join("switch")).unwrap();
        std::fs::write(roms.join("switch/A [0100000000000001][v0].nsp"), b"x").unwrap();
        let coll = t.path().join("coll");
        std::fs::create_dir_all(&coll).unwrap();
        std::fs::write(coll.join("B [0100000000000002][v0].xci"), b"x").unwrap();
        let mut idx = LocalIndex::default();
        idx.add_root("switch", &coll);
        // Converti par le serveur dans le dossier de la Ludothèque.
        assert_eq!(idx.locate(Some(&roms), "switch/A [0100000000000001][v0].nsz"), Some(roms.join("switch/A [0100000000000001][v0].nsp")));
        // Présent seulement dans la collection.
        assert_eq!(idx.locate(Some(&roms), "switch/B [0100000000000002][v0].xcz"), Some(coll.join("B [0100000000000002][v0].xci")));
        assert_eq!(idx.locate(None, "switch/B [0100000000000002][v0].xcz"), Some(coll.join("B [0100000000000002][v0].xci")));
        assert_eq!(idx.locate(Some(&roms), "switch/C [0100000000000003][v0].nsz"), None);
    }

    #[test]
    fn identite_switch_titre_et_version() {
        assert_eq!(title_key("switch/Pokemon Brilliant Diamond [0100000011D90000][v0].nsz"), Some(("0100000011D90000".into(), "0".into())));
        assert_eq!(title_key("Pokemon Brilliant Diamond [0100000011D90000][v0](nsw2u.xyz).xci"), title_key("x/Pokemon BD [0100000011d90000].nsz"));
        assert_eq!(title_key("MAJ [0100F2C0115B6800][v131072] (MAJ 1.1.0).nsz").unwrap().1, "131072");
        assert_ne!(title_key("MAJ [0100F2C0115B6800][v131072].nsp"), title_key("MAJ [0100F2C0115B6800][v65536].nsp"), "autre version de mise à jour");
        assert_eq!(title_key("3ds/Jeu (Europe).cxi"), None);
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("Pokemon Lets Go Pikachu [010003F003A34000].xci"), b"x").unwrap();
        let mut idx = LocalIndex::default();
        idx.add_root("switch", t.path());
        assert!(idx.find("switch", "switch/Pokemon Let's Go Pikachu [010003F003A34000][v0].nsz").is_some());
        assert!(idx.find("switch", "switch/MAJ-DLC/Pokemon Let's Go Pikachu [010003F003A34800][v65536].nsz").is_none());
    }

    #[test]
    fn dossiers_des_emulateurs() {
        let t = tempfile::tempdir().unwrap();
        let ryu = t.path().join("ryu");
        std::fs::create_dir_all(&ryu).unwrap();
        std::fs::write(ryu.join("Config.json"), r#"{"game_dirs": ["/mnt/games/Rom/cartoucheSwitch"], "autre": 1}"#).unwrap();
        assert_eq!(ryujinx_game_dirs(&ryu), [PathBuf::from("/mnt/games/Rom/cartoucheSwitch")]);
        assert!(ryujinx_game_dirs(&t.path().join("vide")).is_empty());

        let data = t.path().join(".var/app/dev.eden_emu.eden/data/eden");
        let conf = t.path().join(".var/app/dev.eden_emu.eden/config/eden");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::write(conf.join("qt-config.ini"), "[UI]\nPaths\\gamedirs\\1\\path=/mnt/games/Rom/cartoucheSwitch\nPaths\\gamedirs\\2\\path=SDMC\nPaths\\gamedirs\\size=2\n").unwrap();
        assert_eq!(eden_game_dirs(&data), [PathBuf::from("/mnt/games/Rom/cartoucheSwitch")]);
    }

    #[test]
    fn cle_souple_entre_serveur_et_thor() {
        let serveur = "3ds/3DS1626 - Animal Crossing - New Leaf - Welcome Amiibo (Europe)(En,Fr,De,Es,It).cxi";
        let thor = "Animal Crossing - New Leaf - Welcome Amiibo (Europe) (En,Fr,De,Es,It).cxi";
        assert_eq!(loose_key(serveur), loose_key(thor));
        assert_eq!(loose_key("3ds/3DS1720- Ever Oasis (Europe)(M6).cxi"), loose_key("Ever Oasis (Europe) (M6).cxi"));
        assert_ne!(loose_key("Pokemon X (Europe).cxi"), loose_key("Pokemon Y (Europe).cxi"));
        assert_eq!(loose_key("3DS"), "3ds", "un titre réduit au mot 3DS n'est pas un préfixe");
    }
}
