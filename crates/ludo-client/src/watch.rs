//! Surveillance des sauvegardes sur CET appareil (inotify, crate `notify`) : une sauvegarde écrite
//! par un émulateur lancé hors de la Ludothèque est synchronisée quand même. Ce module dit QUOI
//! surveiller et À QUI appartient un chemin modifié ; le démon (`ludo watch`) s'occupe du reste.

use crate::config::{self, Config};
use std::path::{Path, PathBuf};

/// Racine de sauvegardes surveillée (récursivement).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    pub emulator: String,
    pub path: PathBuf,
    pub kind: RootKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootKind {
    /// `bis/user/save/<index>/…` de Ryujinx : jeu et utilisateur lus dans `<index>/ExtraData0`.
    Ryujinx,
    /// `…/<TITLE_ID>/…` (Eden, Lichii…) : jeu = premier dossier sous la racine. `profiles` = profils
    /// concernés (un seul pour une racine de compte, tous pour la racine « appareil »).
    TitleDirs { profiles: Vec<String> },
    /// Dossier de sauvegarde d'un portage natif d'un seul jeu (titre fixé par la fiche).
    Fixed { title_id: String, profiles: Vec<String> },
}

/// Changement attribué : émulateur, identifiant de titre (16 hex, minuscules), profils concernés.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub emulator: String,
    pub title_id: String,
    pub profiles: Vec<String>,
}

/// Racines à surveiller : pour chaque émulateur trouvé sur l'appareil et chaque profil configuré,
/// les dossiers que son adaptateur lit (titre fictif pour obtenir les racines).
pub fn roots(cfg: &Config) -> Vec<Root> {
    let mut out: Vec<Root> = Vec::new();
    for emulator in cfg.fiche_ids() {
        for profile in cfg.profils.keys() {
            let Ok(p) = config::prepare(cfg, &emulator, profile, "0000000000000000") else { continue };
            if p.fiche.id == "ryujinx" {
                for d in &p.dirs {
                    if !out.iter().any(|r| r.path == *d) {
                        out.push(Root { emulator: emulator.clone(), path: d.clone(), kind: RootKind::Ryujinx });
                    }
                }
                continue;
            }
            // Portage d'un seul jeu : le dossier de sauvegarde lui-même, attribué au titre de la fiche.
            if let Some(title) = p.fiche.titre.clone() {
                for d in &p.dirs {
                    if !out.iter().any(|r| r.path == *d) {
                        out.push(Root { emulator: emulator.clone(), path: d.clone(), kind: RootKind::Fixed { title_id: title.to_lowercase(), profiles: vec![profile.clone()] } });
                    }
                }
                continue;
            }
            // DirLayout : [compte, appareil] ; la racine est le dossier parent du dossier du titre.
            for (i, d) in p.dirs.iter().enumerate() {
                let Some(root) = d.parent().map(Path::to_path_buf) else { continue };
                let device = i > 0;
                match out.iter_mut().find(|r| r.path == root) {
                    Some(Root { kind: RootKind::TitleDirs { profiles }, .. }) => {
                        if !profiles.contains(profile) {
                            profiles.push(profile.clone());
                        }
                    }
                    Some(_) => {}
                    None => out.push(Root {
                        emulator: emulator.clone(),
                        path: root,
                        kind: RootKind::TitleDirs { profiles: if device { cfg.profils.keys().cloned().collect() } else { vec![profile.clone()] } },
                    }),
                }
            }
        }
    }
    out
}

/// À quel jeu et quels profils appartient ce chemin modifié ? `None` : hors racines, ou fichier
/// sans rapport (index Ryujinx sans `ExtraData0`, dossier non hexadécimal…).
pub fn classify(cfg: &Config, roots: &[Root], path: &Path) -> Option<Change> {
    let root = roots.iter().filter(|r| path.starts_with(&r.path)).max_by_key(|r| r.path.as_os_str().len())?;
    if let RootKind::Fixed { title_id, profiles } = &root.kind {
        return Some(Change { emulator: root.emulator.clone(), title_id: title_id.clone(), profiles: profiles.clone() });
    }
    let first = path.strip_prefix(&root.path).ok()?.components().next()?.as_os_str().to_str()?.to_string();
    match &root.kind {
        RootKind::Fixed { .. } => None,
        RootKind::TitleDirs { profiles } => {
            (first.len() == 16 && first.chars().all(|c| c.is_ascii_hexdigit()))
                .then(|| Change { emulator: root.emulator.clone(), title_id: first.to_lowercase(), profiles: profiles.clone() })
        }
        RootKind::Ryujinx => {
            let extra = std::fs::read(root.path.join(&first).join("ExtraData0")).ok()?;
            if extra.len() < 33 {
                return None;
            }
            let title = u64::from_le_bytes(extra[0..8].try_into().ok()?);
            let uid: [u8; 16] = extra[8..24].try_into().ok()?;
            // Type 3 (appareil, utilisateur nul) : tous les profils ; sinon le profil de cet utilisateur.
            let profiles: Vec<String> = if uid == [0u8; 16] {
                cfg.profils.keys().cloned().collect()
            } else {
                cfg.profils
                    .iter()
                    .filter(|(_, ids)| ids.ryujinx_user_id.as_deref().and_then(crate::layout::ryujinx_uid_bytes) == Some(uid))
                    .map(|(name, _)| name.clone())
                    .collect()
            };
            (!profiles.is_empty()).then(|| Change { emulator: root.emulator.clone(), title_id: format!("{title:016x}"), profiles })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(home: &Path) -> Config {
        toml::from_str(&format!(
            r#"
            serveur = "x"
            appareil = "pc"
            [emulateurs.eden]
            bin = "eden"
            data = "{home}/eden"
            [emulateurs.ryujinx]
            bin = "ryujinx"
            data = "{home}/ryu"
            [profils.guilhem]
            eden_uuid = "A2D2DFD2700F51C158E87A336C77FE55"
            eden_index = 0
            ryujinx_user_id = "00000000000000010000000000000000"
            ryujinx_nom = "Thor"
            [profils.claire]
            eden_uuid = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"
            eden_index = 1
            ryujinx_user_id = "00000000000000020000000000000000"
            ryujinx_nom = "Claire"
            "#,
            home = home.display()
        ))
        .unwrap()
    }

    #[test]
    fn eden_compte_et_appareil() {
        let t = tempfile::tempdir().unwrap();
        let cfg = cfg_with(t.path());
        let roots = roots(&cfg);
        let base = t.path().join("eden/nand/user/save/0000000000000000");
        let g = classify(&cfg, &roots, &base.join("A2D2DFD2700F51C158E87A336C77FE55/0100152000022000/userdata.dat")).unwrap();
        assert_eq!((g.emulator.as_str(), g.title_id.as_str(), g.profiles), ("eden", "0100152000022000", vec!["guilhem".to_string()]));
        let d = classify(&cfg, &roots, &base.join("00000000000000000000000000000000/0100152000022000/x")).unwrap();
        let mut p = d.profiles.clone();
        p.sort();
        assert_eq!(p, ["claire", "guilhem"], "sauvegarde appareil : tous les profils");
        assert!(classify(&cfg, &roots, &base.join("A2D2DFD2700F51C158E87A336C77FE55/pas-un-titre/x")).is_none());
        assert!(classify(&cfg, &roots, &t.path().join("ailleurs/x")).is_none());
    }

    #[test]
    fn ryujinx_par_extradata() {
        let t = tempfile::tempdir().unwrap();
        let cfg = cfg_with(t.path());
        let roots = roots(&cfg);
        let idx = t.path().join("ryu/bis/user/save/0000000000000002");
        std::fs::create_dir_all(idx.join("0")).unwrap();
        let mut extra = vec![0u8; 64];
        extra[0..8].copy_from_slice(&0x0100_1520_0002_2000u64.to_le_bytes());
        extra[8..24].copy_from_slice(&crate::layout::ryujinx_uid_bytes("00000000000000020000000000000000").unwrap());
        extra[32] = 1;
        std::fs::write(idx.join("ExtraData0"), &extra).unwrap();
        let c = classify(&cfg, &roots, &idx.join("0/save.dat")).unwrap();
        assert_eq!((c.title_id.as_str(), c.profiles), ("0100152000022000", vec!["claire".to_string()]));
        // Index sans métadonnées : ignoré.
        assert!(classify(&cfg, &roots, &t.path().join("ryu/bis/user/save/0000000000000009/0/x")).is_none());
    }
}
