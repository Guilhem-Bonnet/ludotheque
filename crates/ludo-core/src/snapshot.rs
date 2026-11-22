//! Instantané, archivage et restauration d'un dossier de sauvegarde.
//!
//! - **Une seule empreinte** : `sha256` de l'archive neutre. Le client et le serveur calculent
//!   donc exactement la même valeur (`SaveRevision.hash` = [`Snapshot::hash`]).
//! - L'archive est un tar déterministe (ordre trié, métadonnées fixes), produit en **un seul
//!   parcours** : l'empreinte décrit toujours l'archive envoyée.
//! - Les fichiers exclus par la fiche (config réécrite par l'émulateur, temporaires) ne sont ni
//!   archivés ni écrasés : la restauration les conserve.
//! - La restauration écrit dans un dossier temporaire voisin, pose un marqueur « complet », puis
//!   bascule. Un arrêt brutal pendant la bascule est réparé par [`recover_interrupted`].
//!   Les versions précédentes sont gardées en rotation (`<dossier>.ludo-prev.1` … `.N`).

use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

/// Nombre de versions locales précédentes conservées après chaque restauration.
pub const PREV_KEEP: usize = 5;
/// Marqueur posé dans le dossier temporaire quand il est complet (jamais archivé).
const MARKER: &str = ".ludo-complete";

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("entrée-sortie : {0}")]
    Io(#[from] io::Error),
    #[error("parcours : {0}")]
    Walk(#[from] walkdir::Error),
    #[error("chemin refusé dans l'archive : {0}")]
    UnsafePath(String),
    #[error("type d'entrée refusé dans l'archive (seuls fichiers et dossiers) : {0}")]
    UnsupportedEntry(String),
    #[error("nom de fichier non UTF-8 : {0}")]
    NonUtf8(String),
    #[error("lien symbolique dans le dossier de sauvegarde : {0}")]
    Symlink(String),
    #[error("archive sans aucun fichier : refusée")]
    EmptyArchive,
}

/// Instantané d'un dossier : archive neutre et son empreinte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub hash: String,
    pub archive: Vec<u8>,
}

/// Empreinte d'une archive (même calcul côté client et côté serveur).
pub fn archive_hash(archive: &[u8]) -> String {
    hex::encode(Sha256::digest(archive))
}

/// Correspondance d'un nom avec un motif où `*` vaut n'importe quelle suite et `?` un caractère ;
/// `\` rend littéral le caractère suivant (voir [`escape_wildcards`]). Les autres caractères,
/// crochets et parenthèses compris, sont pris tels quels.
pub fn wildcard(pattern: &str, name: &str) -> bool {
    #[derive(Clone, Copy, PartialEq)]
    enum Tok {
        Any,
        One,
        Lit(char),
    }
    let mut p = Vec::new();
    let mut it = pattern.chars();
    while let Some(c) = it.next() {
        p.push(match c {
            '*' => Tok::Any,
            '?' => Tok::One,
            '\\' => Tok::Lit(it.next().unwrap_or('\\')),
            c => Tok::Lit(c),
        });
    }
    let n: Vec<char> = name.chars().collect();
    let (mut pi, mut ni, mut star, mut mark) = (0, 0, None, 0);
    while ni < n.len() {
        match p.get(pi) {
            Some(Tok::One) => {
                pi += 1;
                ni += 1;
            }
            Some(Tok::Lit(c)) if *c == n[ni] => {
                pi += 1;
                ni += 1;
            }
            Some(Tok::Any) => {
                star = Some(pi);
                mark = ni;
                pi += 1;
            }
            _ => match star {
                Some(s) => {
                    pi = s + 1;
                    mark += 1;
                    ni = mark;
                }
                None => return false,
            },
        }
    }
    p[pi..].iter().all(|&t| t == Tok::Any)
}

/// Rend littéraux `*`, `?` et `\` d'un texte inséré dans un motif (nom de ROM « Qui ? », etc.).
pub fn escape_wildcards(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '*' | '?' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Vrai si `rel` (chemin relatif, séparateurs `/`) correspond à un motif d'exclusion.
///
/// Motifs : `*.ext` (tout fichier de cette extension, à tout niveau) ; `nom/` (tout dossier de
/// ce nom, à tout niveau) ; `a/b` (chemin exact depuis la racine, fichier ou dossier) ;
/// `nom` (tout fichier ou dossier de ce nom, à tout niveau).
///
/// Motifs « ne garder que » `!motif` (jokers `*` et `?`, sur un fichier à la racine) : dès qu'il y
/// en a un, tout fichier qui ne correspond à aucun d'eux est exclu. Sert aux dossiers partagés par
/// plusieurs jeux (carte mémoire GameCube en dossier GCI, `<rom>.sav` de melonDS) : la
/// sauvegarde d'un jeu n'y est qu'une partie des fichiers, et la restauration conserve les autres
/// (fichiers exclus).
pub fn excluded(rel: &str, patterns: &[String]) -> bool {
    let keep: Vec<&str> = patterns.iter().filter_map(|p| p.strip_prefix('!')).collect();
    if !keep.is_empty() && !keep.iter().any(|k| !rel.contains('/') && wildcard(k, rel)) {
        return true;
    }
    let comps: Vec<&str> = rel.split('/').collect();
    patterns.iter().filter(|p| !p.starts_with('!')).any(|p| {
        if let Some(ext) = p.strip_prefix("*.") {
            comps.last().is_some_and(|f| f.ends_with(&format!(".{ext}")))
        } else if let Some(dir) = p.strip_suffix('/') {
            comps[..comps.len().saturating_sub(1)].contains(&dir)
        } else if p.contains('/') {
            rel == p || rel.starts_with(&format!("{p}/"))
        } else {
            comps.contains(&p.as_str())
        }
    })
}

fn rel_of(root: &Path, path: &Path) -> Result<String, SnapshotError> {
    let rel = path.strip_prefix(root).expect("reste sous la racine");
    let mut parts = Vec::new();
    for c in rel.components() {
        let s = c.as_os_str().to_str().ok_or_else(|| SnapshotError::NonUtf8(rel.display().to_string()))?;
        parts.push(s.to_owned());
    }
    Ok(parts.join("/"))
}

/// Fichiers du dossier, triés : (chemin relatif, chemin absolu, exclu ?).
fn walk(dir: &Path, exclude: &[String]) -> Result<Vec<(String, PathBuf, bool)>, SnapshotError> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in WalkDir::new(dir).follow_links(false).follow_root_links(true) {
        let entry = entry?;
        if entry.depth() == 0 {
            continue;
        }
        let rel = rel_of(dir, entry.path())?;
        if entry.file_type().is_symlink() {
            if excluded(&rel, exclude) {
                continue;
            }
            return Err(SnapshotError::Symlink(rel));
        }
        if !entry.file_type().is_file() || rel == MARKER {
            continue;
        }
        let ex = excluded(&rel, exclude);
        out.push((rel, entry.path().to_path_buf(), ex));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Instantané du dossier (`None` si absent ou sans fichier retenu).
pub fn snapshot(dir: &Path, exclude: &[String]) -> Result<Option<Snapshot>, SnapshotError> {
    let kept: Vec<_> = walk(dir, exclude)?.into_iter().filter(|(_, _, ex)| !ex).collect();
    if kept.is_empty() {
        return Ok(None);
    }
    let mut builder = tar::Builder::new(Vec::new());
    builder.mode(tar::HeaderMode::Deterministic);
    for (rel, abs, _) in kept {
        let mut data = Vec::new();
        fs::File::open(&abs)?.read_to_end(&mut data)?;
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder.append_data(&mut header, &rel, data.as_slice())?;
    }
    let archive = builder.into_inner()?;
    Ok(Some(Snapshot { hash: archive_hash(&archive), archive }))
}

/// Empreinte seule du dossier.
pub fn hash_dir(dir: &Path, exclude: &[String]) -> Result<Option<String>, SnapshotError> {
    Ok(snapshot(dir, exclude)?.map(|s| s.hash))
}

/// Vérifie une archive : uniquement des fichiers et dossiers, chemins relatifs sans `..`,
/// noms UTF-8. Renvoie le nombre de fichiers. Utilisée par le serveur à la réception et par le
/// client avant restauration.
pub fn validate_archive(archive: &[u8]) -> Result<usize, SnapshotError> {
    let mut files = 0;
    let mut ar = tar::Archive::new(archive);
    for entry in ar.entries()? {
        let entry = entry?;
        let path = entry.path()?.into_owned();
        let shown = path.display().to_string();
        if path.to_str().is_none() {
            return Err(SnapshotError::NonUtf8(shown));
        }
        if path.as_os_str().is_empty() || !path.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir)) {
            return Err(SnapshotError::UnsafePath(shown));
        }
        match entry.header().entry_type() {
            tar::EntryType::Regular | tar::EntryType::Continuous => files += 1,
            tar::EntryType::Directory => {}
            _ => return Err(SnapshotError::UnsupportedEntry(shown)),
        }
    }
    Ok(files)
}

fn sibling(dir: &Path, suffix: &str, hidden: bool) -> PathBuf {
    let parent = dir.parent().unwrap_or(Path::new("."));
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    parent.join(if hidden { format!(".{name}{suffix}") } else { format!("{name}{suffix}") })
}

/// Dossier réel à manipuler : si `dir` est un lien symbolique (cas d'EmuDeck), sa cible.
fn real_dir(dir: &Path) -> Result<PathBuf, SnapshotError> {
    if fs::symlink_metadata(dir).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        return Ok(fs::canonicalize(dir)?);
    }
    Ok(dir.to_path_buf())
}

/// Répare une restauration interrompue entre les deux renommages : si le dossier manque mais
/// que le temporaire est marqué complet, on termine la bascule. Sans risque si rien n'est en cours.
pub fn recover_interrupted(dir: &Path) -> Result<(), SnapshotError> {
    let dir = real_dir(dir)?;
    let tmp = sibling(&dir, ".ludo-tmp", true);
    if !dir.exists() && tmp.join(MARKER).exists() {
        fs::rename(&tmp, &dir)?;
    }
    let marker = dir.join(MARKER);
    if marker.exists() {
        fs::remove_file(marker)?;
    }
    Ok(())
}

/// Restaure une archive dans `dir`. Les fichiers exclus déjà présents sont conservés ; l'ancien
/// contenu part en `<dir>.ludo-prev.1` (rotation sur [`PREV_KEEP`] versions).
pub fn restore(dir: &Path, archive: &[u8], exclude: &[String]) -> Result<(), SnapshotError> {
    let dir = real_dir(dir)?;
    recover_interrupted(&dir)?;
    if validate_archive(archive)? == 0 {
        return Err(SnapshotError::EmptyArchive);
    }
    let old = walk(&dir, exclude)?;

    let tmp = sibling(&dir, ".ludo-tmp", true);
    if tmp.exists() {
        fs::remove_dir_all(&tmp)?;
    }
    fs::create_dir_all(&tmp)?;

    let mut ar = tar::Archive::new(archive);
    for entry in ar.entries()? {
        let mut entry = entry?;
        let shown = entry.path()?.display().to_string();
        if !entry.unpack_in(&tmp)? {
            fs::remove_dir_all(&tmp)?;
            return Err(SnapshotError::UnsafePath(shown));
        }
    }

    // Les fichiers exclus (config de l'émulateur…) restent en place.
    for (rel, abs, ex) in &old {
        let target = tmp.join(rel);
        if *ex && !target.exists() {
            if let Some(p) = target.parent() {
                fs::create_dir_all(p)?;
            }
            fs::copy(abs, &target)?;
        }
    }
    fs::write(tmp.join(MARKER), b"")?;

    if dir.exists() {
        rotate_prev(&dir)?;
        fs::rename(&dir, sibling(&dir, ".ludo-prev.1", false))?;
    }
    fs::rename(&tmp, &dir)?;
    fs::remove_file(dir.join(MARKER))?;
    Ok(())
}

/// Décale `.ludo-prev.1..N-1` d'un cran ; la plus ancienne au-delà de N est supprimée.
fn rotate_prev(dir: &Path) -> Result<(), SnapshotError> {
    let oldest = sibling(dir, &format!(".ludo-prev.{PREV_KEEP}"), false);
    if oldest.exists() {
        fs::remove_dir_all(&oldest)?;
    }
    for i in (1..PREV_KEEP).rev() {
        let from = sibling(dir, &format!(".ludo-prev.{i}"), false);
        if from.exists() {
            fs::rename(&from, sibling(dir, &format!(".ludo-prev.{}", i + 1), false))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jokers_et_motifs_ne_garder_que() {
        assert!(wildcard("??-GZ2P-*.gci", "01-GZ2P-gczelda2.gci"));
        assert!(!wildcard("??-GZ2P-*.gci", "01-GALE-smash.gci"));
        assert!(wildcard("Pokemon - Version Diamant (France) (Rev 5).sav", "Pokemon - Version Diamant (France) (Rev 5).sav"));
        assert!(!wildcard("a.sav", "a.sav.bak"));
        assert!(wildcard("*", ""));
        // Texte inséré échappé : « ? » d'un nom de ROM ne vaut plus n'importe quel caractère.
        let rom = escape_wildcards("Qui ? *Jeu*");
        assert!(wildcard(&format!("{rom}.sav"), "Qui ? *Jeu*.sav"));
        assert!(!wildcard(&format!("{rom}.sav"), "Qui X AJeuB.sav"));
        assert!(wildcard(&escape_wildcards("a\\b"), "a\\b"));
        let keep = vec!["!??-GZ2P-*.gci".to_string(), "*.tmp".to_string()];
        assert!(!excluded("01-GZ2P-gczelda2.gci", &keep));
        assert!(excluded("01-GALE-smash.gci", &keep), "autre jeu de la carte : exclu");
        assert!(excluded("sous/01-GZ2P-x.gci", &keep), "seulement à la racine");
        assert!(excluded("01-GZ2P-x.tmp", &["!01-GZ2P-*".to_string(), "*.tmp".to_string()]), "les exclusions s'appliquent en plus");
    }

    #[test]
    fn restauration_partielle_garde_les_autres_jeux_du_dossier() {
        let t = tempfile::tempdir().unwrap();
        let card = t.path().join("Card A");
        write(&card, "01-GZ2P-gczelda2.gci", b"zelda-v1");
        write(&card, "01-GALE-smash.gci", b"smash");
        let keep = vec!["!??-GZ2P-*.gci".to_string()];
        let s1 = snapshot(&card, &keep).unwrap().unwrap();
        write(&card, "01-GZ2P-gczelda2.gci", b"zelda-v2");
        write(&card, "01-GALE-smash.gci", b"smash-v2");
        restore(&card, &s1.archive, &keep).unwrap();
        assert_eq!(fs::read(card.join("01-GZ2P-gczelda2.gci")).unwrap(), b"zelda-v1");
        assert_eq!(fs::read(card.join("01-GALE-smash.gci")).unwrap(), b"smash-v2", "l'autre jeu n'est pas touché");
        assert_eq!(snapshot(&card, &keep).unwrap().unwrap().hash, s1.hash);
        // Aucun fichier du jeu : pas de sauvegarde, même si la carte contient d'autres jeux.
        assert!(snapshot(&card, &["!??-GZ2E-*.gci".to_string()]).unwrap().is_none());
    }

    fn write(dir: &Path, rel: &str, data: &[u8]) {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, data).unwrap();
    }

    fn raw_tar(name: &[u8], kind: tar::EntryType, link: Option<&str>) -> Vec<u8> {
        let mut b = tar::Builder::new(Vec::new());
        let mut h = tar::Header::new_gnu();
        h.set_size(if kind == tar::EntryType::Regular { 1 } else { 0 });
        h.set_mode(0o644);
        h.set_entry_type(kind);
        h.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name);
        if let Some(l) = link {
            h.set_link_name(l).unwrap();
        }
        h.set_cksum();
        let data: &[u8] = if kind == tar::EntryType::Regular { b"x" } else { b"" };
        b.append(&h, data).unwrap();
        b.into_inner().unwrap()
    }

    #[test]
    fn dossier_absent_ou_vide_na_pas_dinstantane() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(hash_dir(&t.path().join("absent"), &[]).unwrap(), None);
        assert_eq!(hash_dir(t.path(), &[]).unwrap(), None);
    }

    #[test]
    fn empreinte_egale_a_celle_de_larchive() {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "a/save.bin", b"1");
        let s = snapshot(t.path(), &[]).unwrap().unwrap();
        assert_eq!(s.hash, archive_hash(&s.archive));
    }

    #[test]
    fn empreinte_stable_et_sensible_au_contenu_et_aux_noms() {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "a/save.bin", b"1");
        write(t.path(), "b.dat", b"2");
        let h1 = hash_dir(t.path(), &[]).unwrap();
        assert_eq!(h1, hash_dir(t.path(), &[]).unwrap());
        write(t.path(), "b.dat", b"3");
        let h2 = hash_dir(t.path(), &[]).unwrap();
        assert_ne!(h1, h2);
        fs::rename(t.path().join("b.dat"), t.path().join("c.dat")).unwrap();
        assert_ne!(h2, hash_dir(t.path(), &[]).unwrap());
    }

    #[test]
    fn motifs_dexclusion() {
        let p = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(excluded("x/y.tmp", &p(&["*.tmp"])));
        assert!(excluded("cache/a", &p(&["cache/"])));
        assert!(excluded("sub/cache/a", &p(&["cache/"])));
        assert!(!excluded("cache", &p(&["cache/"])), "un fichier nommé cache n'est pas un dossier");
        assert!(excluded("system/prod.keys", &p(&["system/prod.keys"])));
        assert!(!excluded("other/system/prod.keys", &p(&["system/prod.keys"])));
        assert!(excluded("a/Config.json", &p(&["Config.json"])));
    }

    #[test]
    fn les_fichiers_exclus_ne_comptent_pas() {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "save.bin", b"1");
        let h = hash_dir(t.path(), &[]).unwrap();
        let ex = vec!["*.tmp".to_string(), "config.ini".to_string(), "cache/".to_string()];
        write(t.path(), "x.tmp", b"t");
        write(t.path(), "sub/config.ini", b"c");
        write(t.path(), "deep/cache/a", b"c");
        assert_eq!(hash_dir(t.path(), &ex).unwrap(), h);
    }

    #[test]
    fn aller_retour_et_conservation_des_exclus() {
        let src = tempfile::tempdir().unwrap();
        write(src.path(), "0000/save.bin", b"progression");
        let s = snapshot(src.path(), &[]).unwrap().unwrap();

        let root = tempfile::tempdir().unwrap();
        let dst = root.path().join("save");
        write(&dst, "ancien.bin", b"vieux");
        write(&dst, "config.ini", b"reglages");
        let ex = vec!["config.ini".to_string()];
        restore(&dst, &s.archive, &ex).unwrap();

        assert_eq!(hash_dir(&dst, &ex).unwrap().unwrap(), s.hash);
        assert_eq!(fs::read(dst.join("config.ini")).unwrap(), b"reglages", "config conservée");
        assert!(!dst.join("ancien.bin").exists());
        assert!(root.path().join("save.ludo-prev.1/ancien.bin").exists());
        assert!(!dst.join(MARKER).exists());
    }

    #[test]
    fn les_versions_precedentes_tournent_sans_ecraser_la_derniere_partie() {
        let src = tempfile::tempdir().unwrap();
        write(src.path(), "s", b"serveur");
        let s = snapshot(src.path(), &[]).unwrap().unwrap();
        let root = tempfile::tempdir().unwrap();
        let dst = root.path().join("save");
        write(&dst, "s", b"PARTIE-NON-ENVOYEE");
        restore(&dst, &s.archive, &[]).unwrap();
        restore(&dst, &s.archive, &[]).unwrap();
        // La partie non envoyée a glissé en .2 au lieu d'être détruite.
        assert_eq!(fs::read(root.path().join("save.ludo-prev.2/s")).unwrap(), b"PARTIE-NON-ENVOYEE");
        for _ in 0..PREV_KEEP + 2 {
            restore(&dst, &s.archive, &[]).unwrap();
        }
        assert!(root.path().join(format!("save.ludo-prev.{PREV_KEEP}")).exists());
        assert!(!root.path().join(format!("save.ludo-prev.{}", PREV_KEEP + 1)).exists());
    }

    #[test]
    fn archive_vide_refusee_et_dossier_intact() {
        let empty = tar::Builder::new(Vec::new()).into_inner().unwrap();
        let root = tempfile::tempdir().unwrap();
        let dst = root.path().join("save");
        write(&dst, "s", b"precieux");
        assert!(matches!(restore(&dst, &empty, &[]), Err(SnapshotError::EmptyArchive)));
        assert_eq!(fs::read(dst.join("s")).unwrap(), b"precieux");
    }

    #[test]
    fn traversee_par_point_point_refusee() {
        let ar = raw_tar(b"../evil", tar::EntryType::Regular, None);
        let root = tempfile::tempdir().unwrap();
        assert!(matches!(validate_archive(&ar), Err(SnapshotError::UnsafePath(_))));
        assert!(restore(&root.path().join("save"), &ar, &[]).is_err());
        assert!(!root.path().join("evil").exists());
    }

    #[test]
    fn chemin_absolu_refuse() {
        let ar = raw_tar(b"/tmp/evil", tar::EntryType::Regular, None);
        assert!(matches!(validate_archive(&ar), Err(SnapshotError::UnsafePath(_))));
    }

    #[test]
    fn lien_symbolique_dans_larchive_refuse() {
        let outside = tempfile::tempdir().unwrap();
        let ar = raw_tar(b"lnk", tar::EntryType::Symlink, Some(outside.path().to_str().unwrap()));
        assert!(matches!(validate_archive(&ar), Err(SnapshotError::UnsupportedEntry(_))));
        let root = tempfile::tempdir().unwrap();
        assert!(restore(&root.path().join("save"), &ar, &[]).is_err());
        let ar = raw_tar(b"hard", tar::EntryType::Link, Some("/etc/passwd"));
        assert!(matches!(validate_archive(&ar), Err(SnapshotError::UnsupportedEntry(_))));
    }

    #[cfg(unix)]
    #[test]
    fn dossier_de_sauvegarde_lien_symbolique_preserve() {
        let src = tempfile::tempdir().unwrap();
        write(src.path(), "s", b"neuf");
        let s = snapshot(src.path(), &[]).unwrap().unwrap();
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("storage");
        write(&target, "s", b"vieux");
        let link = root.path().join("saves");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        restore(&link, &s.archive, &[]).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "le lien reste un lien");
        assert_eq!(fs::read(target.join("s")).unwrap(), b"neuf", "la cible est mise à jour");
    }

    #[cfg(unix)]
    #[test]
    fn lien_symbolique_dans_le_dossier_signale() {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "s", b"1");
        std::os::unix::fs::symlink("/etc/hostname", t.path().join("lien")).unwrap();
        assert!(matches!(snapshot(t.path(), &[]), Err(SnapshotError::Symlink(_))));
    }

    #[test]
    fn reprise_apres_arret_brutal_pendant_la_bascule() {
        let root = tempfile::tempdir().unwrap();
        let dst = root.path().join("save");
        let tmp = root.path().join(".save.ludo-tmp");
        write(&tmp, "s", b"nouveau");
        write(&tmp, MARKER, b"");
        // Le dossier a déjà été déplacé en .prev, le second renommage n'a pas eu lieu.
        recover_interrupted(&dst).unwrap();
        assert_eq!(fs::read(dst.join("s")).unwrap(), b"nouveau");
        assert!(!dst.join(MARKER).exists());
    }
}
