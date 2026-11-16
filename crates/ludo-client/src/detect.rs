//! Détection des émulateurs installés sur l'appareil, d'après les moyens listés dans chaque fiche
//! (`[[detection]]`). Aucun cache : la détection relit le disque à chaque appel, donc un émulateur
//! installé ou retiré est vu au prochain lancement ou à la prochaine ouverture des Réglages.
//!
//! Lecture seule : rien n'est lancé (pas de `flatpak list`), on regarde seulement si les fichiers
//! existent.

use crate::config::expand_in;
use ludo_core::fiche::{Candidate, Fiche};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Émulateur trouvé : commande de lancement (`{bin}`), dossier de données (`{data}`) et moyen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub bin: String,
    pub data: PathBuf,
    /// « Flatpak io.github… », « AppImage ~/Applications/… », « PATH /usr/bin/… », « fichier … ».
    pub source: String,
}

/// Où chercher : dossier personnel et `PATH` (injectables pour les tests).
#[derive(Debug, Clone)]
pub struct Host {
    pub home: PathBuf,
    pub path: Option<OsString>,
    /// Dossier Flatpak du système (`/var/lib/flatpak`).
    pub system_flatpak: PathBuf,
}

impl Host {
    pub fn current() -> Self {
        Host { home: crate::config::home(), path: std::env::var_os("PATH"), system_flatpak: PathBuf::from("/var/lib/flatpak") }
    }
}

/// Dossiers où l'on cherche les AppImage (EmuDeck range les siennes dans `~/Applications` et
/// `~/Emulation/tools`).
const APPIMAGE_DIRS: &[&str] = &["Applications", "Emulation/tools", ".local/bin", "bin", "AppImages"];

/// Premier moyen de la fiche présent sur l'appareil, ou `None`.
pub fn detect(fiche: &Fiche, host: &Host) -> Option<Found> {
    fiche.detection.iter().find_map(|c| probe(c, host))
}

fn probe(c: &Candidate, host: &Host) -> Option<Found> {
    let (bin, source) = if let Some(id) = &c.flatpak {
        let user = host.home.join(".local/share/flatpak/app").join(id);
        let system = host.system_flatpak.join("app").join(id);
        if !user.is_dir() && !system.is_dir() {
            return None;
        }
        (format!("flatpak run {id}"), format!("Flatpak {id}"))
    } else if let Some(pattern) = &c.appimage {
        let p = newest_match(host, pattern)?;
        (p.display().to_string(), format!("AppImage {}", p.display()))
    } else if let Some(name) = &c.commande {
        let p = in_path(host, name)?;
        (p.display().to_string(), format!("PATH {}", p.display()))
    } else if let Some(file) = &c.fichier {
        let p = expand_in(&host.home, file);
        if !is_executable(&p) {
            return None;
        }
        (p.display().to_string(), format!("fichier {}", p.display()))
    } else {
        return None;
    };
    // Données : le premier dossier qui existe, sinon le premier de la liste (l'émulateur le crée).
    let dirs: Vec<PathBuf> = c.data.iter().map(|d| expand_in(&host.home, d)).collect();
    let data = dirs.iter().find(|d| d.is_dir()).or(dirs.first()).cloned()?;
    Some(Found { bin, data, source })
}

/// AppImage la plus récente (date de modification) dont le nom correspond au motif `préfixe*suffixe`.
fn newest_match(host: &Host, pattern: &str) -> Option<PathBuf> {
    let (prefix, suffix) = pattern.split_once('*').unwrap_or((pattern, ""));
    let lower = |s: &str| s.to_lowercase();
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for dir in APPIMAGE_DIRS {
        let Ok(entries) = std::fs::read_dir(host.home.join(dir)) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            // Insensible à la casse : « eden-linux….AppImage » comme « Eden-Linux….AppImage ».
            if !(lower(&name).starts_with(&lower(prefix)) && lower(&name).ends_with(&lower(suffix))) {
                continue;
            }
            let p = e.path();
            if !is_executable(&p) {
                continue;
            }
            let t = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            if best.as_ref().is_none_or(|(bt, _)| t > *bt) {
                best = Some((t, p));
            }
        }
    }
    best.map(|(_, p)| p)
}

fn in_path(host: &Host, name: &str) -> Option<PathBuf> {
    let path = host.path.as_ref()?;
    std::env::split_paths(path).map(|d| d.join(name)).find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn exe(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn fiche(extra: &str) -> Fiche {
        Fiche::parse(&format!(
            "id = \"x\"\nconsoles = [\"switch\"]\nformats = [\"nsp\"]\nlancement = \"{{bin}} {{rom}}\"\nprofils = \"natifs\"\nsauvegardes = \"{{data}}\"\n{extra}"
        ))
        .unwrap()
    }

    const CANDIDATS: &str = r#"
[[detection]]
flatpak = "org.exemple.Emu"
data = ["~/.var/app/org.exemple.Emu/data"]

[[detection]]
appimage = "Emu*.AppImage"
data = ["~/Emulation/storage/emu", "~/.local/share/emu"]

[[detection]]
commande = "emu"
data = ["~/.local/share/emu"]
"#;

    fn host(home: &Path, path: Option<&Path>) -> Host {
        Host { home: home.into(), path: path.map(|p| p.as_os_str().to_owned()), system_flatpak: home.join("système-flatpak") }
    }

    #[test]
    fn rien_d_installe() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(detect(&fiche(CANDIDATS), &host(t.path(), None)), None);
    }

    #[test]
    fn flatpak_prioritaire_sur_appimage() {
        let t = tempfile::tempdir().unwrap();
        exe(&t.path().join("Applications/Emu-1.0.AppImage"));
        std::fs::create_dir_all(t.path().join(".local/share/flatpak/app/org.exemple.Emu")).unwrap();
        let f = detect(&fiche(CANDIDATS), &host(t.path(), None)).unwrap();
        assert_eq!(f.bin, "flatpak run org.exemple.Emu");
        assert_eq!(f.data, t.path().join(".var/app/org.exemple.Emu/data"));
    }

    #[test]
    fn flatpak_systeme() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("système-flatpak/app/org.exemple.Emu")).unwrap();
        assert!(detect(&fiche(CANDIDATS), &host(t.path(), None)).unwrap().source.starts_with("Flatpak"));
    }

    #[test]
    fn appimage_la_plus_recente_et_dossier_existant() {
        let t = tempfile::tempdir().unwrap();
        let old = t.path().join("Applications/emu-0.9.AppImage");
        let new = t.path().join("Emulation/tools/Emu-1.2.AppImage");
        exe(&old);
        exe(&new);
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options().write(true).open(&old).unwrap().set_modified(past).unwrap();
        // Une AppImage non exécutable est ignorée.
        std::fs::write(t.path().join("Applications/Emu-9.AppImage"), b"x").unwrap();
        std::fs::create_dir_all(t.path().join(".local/share/emu")).unwrap();
        let f = detect(&fiche(CANDIDATS), &host(t.path(), None)).unwrap();
        assert_eq!(f.bin, new.display().to_string());
        // Le premier dossier de données n'existe pas : le second, présent, est retenu.
        assert_eq!(f.data, t.path().join(".local/share/emu"));
    }

    #[test]
    fn commande_du_path_et_fichier_fixe() {
        let t = tempfile::tempdir().unwrap();
        let bin = t.path().join("usr/bin");
        exe(&bin.join("emu"));
        let f = detect(&fiche(CANDIDATS), &host(t.path(), Some(&bin))).unwrap();
        assert_eq!((f.bin, f.source.starts_with("PATH")), (bin.join("emu").display().to_string(), true));
        // Aucun dossier de données n'existe : le premier de la liste est retenu.
        assert_eq!(f.data, t.path().join(".local/share/emu"));

        let fixe = fiche("[[detection]]\nfichier = \"~/Applications/publish/Emu\"\ndata = [\"~/d\"]\n");
        assert_eq!(detect(&fixe, &host(t.path(), None)), None);
        exe(&t.path().join("Applications/publish/Emu"));
        assert!(detect(&fixe, &host(t.path(), None)).unwrap().source.starts_with("fichier"));
    }
}
