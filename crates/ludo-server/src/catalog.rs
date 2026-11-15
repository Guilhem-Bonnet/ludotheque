//! Catalogue des jeux : balayage du dossier des ROM du serveur et lecture de `catalogue.toml`.
//!
//! Organisation attendue sous la racine des ROM : un sous-dossier par console (`switch`, `switch2`,
//! `3ds`, `ds`, `wiiu`, `gc`). Les noms et icônes internes aux ROM (NACP/SMDH) exigent les clés des
//! consoles : hors périmètre. Le fichier `catalogue.toml` à la racine décrit les jeux à la main ;
//! un fichier trouvé mais absent du catalogue apparaît quand même, avec un titre tiré de son nom et
//! une jaquette vive choisie de façon déterministe.
//!
//! ```toml
//! [[game]]
//! id       = "switch-lm3"
//! console  = "switch"
//! title    = "Luigi's Mansion 3"
//! short    = "Luigi's\nMansion 3"        # optionnel (défaut : le titre)
//! title_id = "0100dca0064a6000"          # optionnel (sinon lu dans le nom du fichier, entre crochets)
//! entry    = "switch/Luigi's Mansion 3.nsp"   # relatif à la racine des ROM (ou au dossier de la console)
//! art      = { motif = "manoir", from = "#22e08f", to = "#0a5a78", ink = "#0a3a50" }   # optionnel
//! ```
//!
//! Formats reconnus : Switch `.nsp .xci .nsz .xcz` (mises à jour et DLC dans `MAJ-DLC/`), 3DS
//! `.cxi .3ds .zcci .zcxi`, DS `.nds`, GameCube `.rvz`, Wii U un dossier, Switch 2 `.nup` ou un
//! dossier extrait. Les dossiers `Zip*` sont ignorés. Pas de hachage : taille et date suffisent.

use crate::store::StoreError;
use ludo_proto::{FileRole, GameArt, CONSOLES, MOTIFS};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::UNIX_EPOCH;

/// Fichier trouvé sur le disque.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedFile {
    pub path: String,
    pub role: FileRole,
    pub format: String,
    pub size: u64,
    pub mtime: u64,
}

/// Jeu reconstitué par le balayage (avant écriture en base).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedGame {
    pub id: String,
    pub console: String,
    pub title: String,
    pub short: String,
    pub title_id: Option<String>,
    pub art: GameArt,
    pub catalogued: bool,
    pub files: Vec<ScannedFile>,
}

#[derive(Debug, Deserialize)]
struct CatalogFile {
    #[serde(default)]
    game: Vec<CatalogEntry>,
}

#[derive(Debug, Deserialize)]
struct CatalogEntry {
    id: String,
    console: String,
    title: String,
    short: Option<String>,
    title_id: Option<String>,
    entry: String,
    art: Option<GameArt>,
}

/// Jaquettes vives de repli (dégradés saturés, jamais ternes) ; l'identifiant du jeu en choisit une.
const PRESETS: &[(&str, &str, &str, Option<&str>)] = &[
    ("manoir", "#22e08f", "#0a5a78", None),
    ("course", "#ff9f1c", "#ff2e63", None),
    ("lune", "#ff5fa8", "#5b2bff", Some("#34138f")),
    ("prairie", "#45cf78", "#2fb8ff", None),
    ("cristal", "#5ee0ff", "#3d4dff", None),
    ("vagues", "#3fd0ff", "#1257d6", None),
    ("montagnes", "#7fe3c4", "#1a8f8a", Some("#0b4a44")),
    ("eclair", "#ffd23f", "#ff5a3c", None),
    ("cristal", "#a98bff", "#3a2bd6", None),
    ("eclair", "#ffe14d", "#ff9f1c", None),
    ("lune", "#ff9a3c", "#3b2a8a", Some("#1e1450")),
    ("montagnes", "#5ad1ff", "#3b3fd6", Some("#1b1f6e")),
    ("course", "#ffe14d", "#ff2e63", None),
    ("vagues", "#34e0a1", "#0a8fd6", None),
    ("prairie", "#b6f24a", "#12b886", None),
];

fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
}

/// Jaquette de repli, déterministe pour un identifiant donné.
pub fn preset_art(id: &str) -> GameArt {
    let (motif, from, to, ink) = PRESETS[(fnv1a(id) % PRESETS.len() as u64) as usize];
    GameArt { motif: motif.into(), from: from.into(), to: to.into(), ink: ink.map(String::from) }
}

fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

fn valid_art(a: &GameArt) -> bool {
    MOTIFS.contains(&a.motif.as_str()) && is_hex_color(&a.from) && is_hex_color(&a.to) && a.ink.as_deref().is_none_or(is_hex_color)
}

fn formats(console: &str) -> &'static [&'static str] {
    match console {
        "switch" => &["nsp", "xci", "nsz", "xcz"],
        "switch2" => &["nup"],
        "3ds" => &["cxi", "3ds", "zcci", "zcxi"],
        "ds" => &["nds"],
        "gc" => &["rvz"],
        _ => &[],
    }
}

/// Un dossier de premier niveau est-il un jeu à part entière (Wii U, Switch 2 extrait) ?
fn dir_is_game(console: &str) -> bool {
    matches!(console, "wiiu" | "switch2")
}

fn has_addons(console: &str) -> bool {
    matches!(console, "switch" | "switch2")
}

/// Titre lisible tiré d'un nom de fichier : groupes `[...]` et `(...)` retirés, `_` en espace.
pub fn clean_name(stem: &str) -> String {
    let mut out = String::new();
    let mut depth = 0u32;
    for c in stem.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(if c == '_' { ' ' } else { c }),
            _ => {}
        }
    }
    let joined = out.split_whitespace().collect::<Vec<_>>().join(" ");
    joined.trim_matches(|c: char| "-. ".contains(c)).to_string()
}

/// Identifiant de titre Switch (16 chiffres hexadécimaux commençant par `01`) lu dans un nom.
pub fn title_id_in(name: &str) -> Option<u64> {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .find(|run| run.len() == 16 && run.starts_with("01") && run.chars().all(|c| c.is_ascii_hexdigit()))
        .and_then(|run| u64::from_str_radix(run, 16).ok())
}

fn parse_title_id(s: &str) -> Option<u64> {
    (s.len() == 16 && s.chars().all(|c| c.is_ascii_hexdigit())).then(|| u64::from_str_radix(s, 16).ok()).flatten()
}

/// Version ASCII d'un nom pour un identifiant : minuscules, accents retirés, tirets.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().flat_map(char::to_lowercase) {
        let c = match c {
            'à' | 'á' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' => 'i',
            'ô' | 'ö' | 'ó' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// Nom comparable : minuscules alphanumériques seulement.
fn norm(s: &str) -> String {
    slug(s).replace('-', "")
}

fn mtime_s(m: &std::fs::Metadata) -> u64 {
    m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())
}

/// Taille cumulée et dernière modification d'un dossier (liens symboliques ignorés).
fn dir_stats(dir: &Path) -> (u64, u64) {
    let (mut size, mut mtime) = (0, 0);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.filter_map(Result::ok) {
            let Ok(m) = e.metadata() else { continue };
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                stack.push(e.path());
            } else {
                size += m.len();
            }
            mtime = mtime.max(mtime_s(&m));
        }
    }
    (size, mtime)
}

/// Candidat trouvé sur le disque, avant rattachement à un jeu.
struct Candidate {
    console: &'static str,
    /// Nom sans extension (ou nom du dossier).
    stem: String,
    file: ScannedFile,
    tid: Option<u64>,
}

fn rel(parts: &[&str]) -> String {
    parts.join("/")
}

fn extension(name: &str) -> Option<String> {
    Path::new(name).extension().and_then(|e| e.to_str()).map(str::to_lowercase)
}

fn collect_addons(dir: &Path, rel_dir: &str, console: &'static str, out: &mut Vec<Candidate>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let Some(name) = e.file_name().to_str().map(String::from) else { continue };
        let Ok(ft) = e.file_type() else { continue };
        if name.starts_with('.') || ft.is_symlink() {
            continue;
        }
        let path = rel(&[rel_dir, &name]);
        if ft.is_dir() {
            if !name.to_lowercase().starts_with("zip") {
                collect_addons(&e.path(), &path, console, out);
            }
        } else if ft.is_file() {
            let Some(ext) = extension(&name).filter(|x| formats(console).contains(&x.as_str())) else { continue };
            let Ok(m) = e.metadata() else { continue };
            let stem = name[..name.len() - ext.len() - 1].to_string();
            out.push(Candidate {
                console,
                tid: title_id_in(&stem),
                stem,
                file: ScannedFile { path, role: FileRole::Maj, format: ext, size: m.len(), mtime: mtime_s(&m) },
            });
        }
    }
}

/// Parcourt les dossiers de consoles : jeux de base d'un côté, mises à jour et DLC de l'autre.
fn collect(root: &Path) -> Result<(Vec<Candidate>, Vec<Candidate>), StoreError> {
    let (mut bases, mut addons) = (Vec::new(), Vec::new());
    for &console in CONSOLES {
        // Dossier de console absent : normal. Présent mais illisible : erreur (jamais « zéro jeu »).
        let rd = match std::fs::read_dir(root.join(console)) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(StoreError::RomsUnavailable(format!("{console} : {e}"))),
        };
        let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let Some(name) = e.file_name().to_str().map(String::from) else {
                tracing::warn!(console, "nom de fichier non UTF-8 ignoré");
                continue;
            };
            let Ok(ft) = e.file_type() else { continue };
            if name.starts_with('.') || ft.is_symlink() {
                continue;
            }
            let path = rel(&[console, &name]);
            if ft.is_dir() {
                let lower = name.to_lowercase();
                if lower.starts_with("zip") {
                    continue;
                }
                if lower == "maj-dlc" && has_addons(console) {
                    collect_addons(&e.path(), &path, console, &mut addons);
                } else if dir_is_game(console) {
                    let (size, mtime) = dir_stats(&e.path());
                    bases.push(Candidate {
                        console,
                        tid: title_id_in(&name),
                        stem: name,
                        file: ScannedFile { path, role: FileRole::Base, format: "dir".into(), size, mtime },
                    });
                }
            } else if ft.is_file() {
                let Some(ext) = extension(&name).filter(|x| formats(console).contains(&x.as_str())) else { continue };
                let Ok(m) = e.metadata() else { continue };
                let stem = name[..name.len() - ext.len() - 1].to_string();
                bases.push(Candidate {
                    console,
                    tid: title_id_in(&stem),
                    stem,
                    file: ScannedFile { path, role: FileRole::Base, format: ext, size: m.len(), mtime: mtime_s(&m) },
                });
            }
        }
    }
    Ok((bases, addons))
}

fn load_catalog(root: &Path) -> Result<Vec<CatalogEntry>, StoreError> {
    let path = root.join("catalogue.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let file: CatalogFile = toml::from_str(&text).map_err(|e| StoreError::Catalog(format!("catalogue.toml : {e}")))?;
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for g in file.game {
        if !crate::store::valid_id(&g.id) {
            tracing::warn!(id = %g.id, "catalogue.toml : identifiant invalide, jeu ignoré");
        } else if !CONSOLES.contains(&g.console.as_str()) {
            tracing::warn!(id = %g.id, console = %g.console, "catalogue.toml : console inconnue, jeu ignoré");
        } else if !seen.insert(g.id.clone()) {
            tracing::warn!(id = %g.id, "catalogue.toml : identifiant en double, jeu ignoré");
        } else {
            out.push(g);
        }
    }
    Ok(out)
}

/// Chemin de l'entrée du catalogue : relatif à la racine, sinon au dossier de la console.
fn entry_path(root: &Path, e: &CatalogEntry) -> String {
    let clean = e.entry.trim_start_matches("./").replace('\\', "/");
    if root.join(&clean).exists() {
        clean
    } else {
        format!("{}/{clean}", e.console)
    }
}

struct Draft {
    game: ScannedGame,
    tid: Option<u64>,
    /// Nom comparable du premier fichier de base, pour rattacher les mises à jour sans identifiant.
    name: String,
}

fn draft_from_entry(e: &CatalogEntry, cand: &Candidate) -> Draft {
    let art = match &e.art {
        Some(a) if valid_art(a) => a.clone(),
        Some(_) => {
            tracing::warn!(id = %e.id, "catalogue.toml : jaquette invalide, jaquette de repli");
            preset_art(&e.id)
        }
        None => preset_art(&e.id),
    };
    let tid = e.title_id.as_deref().and_then(parse_title_id).or(cand.tid);
    Draft {
        game: ScannedGame {
            id: e.id.clone(),
            console: e.console.clone(),
            title: e.title.clone(),
            short: e.short.clone().unwrap_or_else(|| e.title.clone()).replace("\\n", "\n"),
            title_id: tid.map(|t| format!("{t:016x}")),
            art,
            catalogued: true,
            files: Vec::new(),
        },
        tid,
        name: norm(&clean_name(&cand.stem)),
    }
}

fn unlisted_id(c: &Candidate) -> String {
    match c.tid {
        Some(t) => format!("{}-{t:016x}", c.console),
        None => {
            let s = slug(&clean_name(&c.stem));
            let s = if s.is_empty() { format!("{:08x}", fnv1a(&c.stem) as u32) } else { s };
            let id = format!("{}-{s}", c.console);
            id.chars().take(128).collect()
        }
    }
}

fn draft_unlisted(id: String, c: &Candidate) -> Draft {
    let title = match clean_name(&c.stem) {
        t if t.is_empty() => c.stem.clone(),
        t => t,
    };
    Draft {
        game: ScannedGame {
            art: preset_art(&id),
            id,
            console: c.console.into(),
            short: title.clone(),
            title,
            title_id: c.tid.map(|t| format!("{t:016x}")),
            catalogued: false,
            files: Vec::new(),
        },
        tid: c.tid,
        name: norm(&clean_name(&c.stem)),
    }
}

/// Balaye la racine des ROM et renvoie les jeux reconnus, triés par identifiant.
/// Résultat d'un balayage : jeux reconnus et éléments écartés (avec la raison).
pub struct ScanResult {
    pub games: Vec<ScannedGame>,
    pub ignored: Vec<String>,
}

pub fn scan(root: &Path) -> Result<ScanResult, StoreError> {
    // Racine absente ou illisible (partage non monté) : erreur, jamais un catalogue vide.
    std::fs::read_dir(root).map_err(|e| StoreError::RomsUnavailable(format!("{} : {e}", root.display())))?;
    let entries = load_catalog(root)?;
    let (bases, addons) = collect(root)?;
    let mut ignored = Vec::new();
    let mut drafts: BTreeMap<String, Draft> = BTreeMap::new();
    let by_entry: BTreeMap<String, &CatalogEntry> = entries.iter().map(|e| (entry_path(root, e), e)).collect();
    for (path, e) in &by_entry {
        if !bases.iter().any(|b| &b.file.path == path) {
            tracing::warn!(id = %e.id, entry = %e.entry, "catalogue.toml : fichier d'entrée absent du disque, jeu ignoré");
            ignored.push(format!("{} : fichier d'entrée introuvable ({})", e.id, e.entry));
        }
    }

    // Passe 1 : fichiers nommés par le catalogue.
    let mut rest = Vec::new();
    for c in bases {
        match by_entry.get(&c.file.path) {
            Some(e) => {
                let d = drafts.entry(e.id.clone()).or_insert_with(|| draft_from_entry(e, &c));
                d.game.files.push(c.file);
            }
            None => rest.push(c),
        }
    }
    // Passe 2 : autres fichiers, rattachés à un jeu du catalogue de même titre, sinon jeu déduit du nom.
    for c in rest {
        let known = c.tid.and_then(|t| {
            drafts.values().find(|d| d.game.catalogued && d.game.console == c.console && d.tid == Some(t)).map(|d| d.game.id.clone())
        });
        let id = known.unwrap_or_else(|| unlisted_id(&c));
        let d = drafts.entry(id.clone()).or_insert_with(|| draft_unlisted(id, &c));
        d.game.files.push(c.file);
    }

    // Mises à jour et DLC : par identifiant de titre (MAJ = base | 0x800, DLC = (base & !0xFFF) + 0x1000 + n), sinon par nom.
    for a in addons {
        let target = drafts
            .values()
            .filter(|d| d.game.console == a.console)
            .find_map(|d| {
                let (b, t) = (d.tid?, a.tid?);
                if t == (b | 0x800) {
                    Some((d.game.id.clone(), FileRole::Maj))
                } else if t >> 12 == (b >> 12) + 1 {
                    Some((d.game.id.clone(), FileRole::Dlc))
                } else {
                    None
                }
            })
            .or_else(|| {
                let n = norm(&clean_name(&a.stem));
                let role = if a.stem.to_lowercase().contains("dlc") { FileRole::Dlc } else { FileRole::Maj };
                drafts
                    .values()
                    // Par nom seulement si l'un des deux identifiants de titre manque : deux identifiants
                    // connus et différents désignent deux jeux différents (« Mario » ≠ « Mario Kart »).
                    .filter(|d| d.game.console == a.console && !(d.tid.is_some() && a.tid.is_some()))
                    .filter(|d| !d.name.is_empty() && n.starts_with(&d.name))
                    .max_by_key(|d| d.name.len())
                    .map(|d| (d.game.id.clone(), role))
            });
        match target {
            Some((id, role)) => {
                if let Some(d) = drafts.get_mut(&id) {
                    d.game.files.push(ScannedFile { role, ..a.file });
                }
            }
            None => {
                tracing::warn!(path = %a.file.path, "mise à jour ou DLC sans jeu correspondant, ignoré");
                ignored.push(format!("{} : mise à jour ou DLC sans jeu correspondant", a.file.path));
            }
        }
    }

    let games = drafts
        .into_values()
        .map(|mut d| {
            d.game.files.sort_by(|a, b| (a.role.as_str(), &a.path).cmp(&(b.role.as_str(), &b.path)));
            d.game
        })
        .collect();
    Ok(ScanResult { games, ignored })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noms_nettoyes() {
        assert_eq!(clean_name("Mario Kart 8 Deluxe [0100152000022000][v0]"), "Mario Kart 8 Deluxe");
        assert_eq!(clean_name("Pokemon_Diamant (France) (v1.0)"), "Pokemon Diamant");
        assert_eq!(clean_name("[0100152000022000]"), "");
    }

    #[test]
    fn identifiants_de_titre() {
        assert_eq!(title_id_in("Zelda [01007EF00011E000][v0]"), Some(0x01007EF00011E000));
        assert_eq!(title_id_in("Sans identifiant"), None);
        assert_eq!(title_id_in("[0200152000022000]"), None);
    }

    #[test]
    fn identifiants_de_jeu() {
        assert_eq!(slug("Pokémon Épée & Bouclier"), "pokemon-epee-bouclier");
        assert_eq!(slug("!!!"), "");
    }

    #[test]
    fn jaquette_deterministe_et_valide() {
        assert_eq!(preset_art("ds-pokemon"), preset_art("ds-pokemon"));
        assert!(PRESETS.iter().all(|(m, f, t, i)| valid_art(&GameArt {
            motif: (*m).into(),
            from: (*f).into(),
            to: (*t).into(),
            ink: i.map(String::from)
        })));
    }
}
