//! Profils côté appareil : dernier profil choisi (retenu par appareil) et lancement au démarrage
//! de la session (Linux : entrée XDG dans `~/.config/autostart`).

use std::path::{Path, PathBuf};

/// Fichier du dernier profil choisi, à côté du fichier de configuration (`~/.config/ludo/dernier-profil`
/// sur le PC et le Deck ; dossier externe de l'app sur Android).
pub fn remembered_path(config: &Path) -> PathBuf {
    config.parent().unwrap_or_else(|| Path::new(".")).join("dernier-profil")
}

/// Identifiant de profil retenu : 1 à 64 caractères `a-z`, `0-9`, `-`, `_` (sous-ensemble prudent de ce que
/// le serveur accepte ; les profils du foyer s'y tiennent).
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Dernier profil choisi sur l'appareil ; absent, illisible ou invalide : `None`.
pub fn remembered(path: &Path) -> Option<String> {
    let id = std::fs::read_to_string(path).ok()?;
    let id = id.trim();
    valid_id(id).then(|| id.to_string())
}

/// Écriture atomique : fichier temporaire au nom unique, synchronisé sur disque, puis renommage
/// (deux écritures simultanées ne se marchent pas dessus, une coupure ne laisse pas un fichier vide).
fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.{}.{}.tmp", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let res = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// Retient le profil choisi (écriture atomique).
pub fn remember(path: &Path, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err(format!("Profil invalide : {id}"));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Profil non retenu : {e}"))?;
    }
    write_atomic(path, &format!("{id}\n")).map_err(|e| format!("Profil non retenu : {e}"))
}

/// Entrée de démarrage automatique (spécification XDG Autostart) : `$XDG_CONFIG_HOME/autostart`, sinon
/// `~/.config/autostart`.
pub fn autostart_path(xdg_config_home: Option<&str>, home: &Path) -> PathBuf {
    let base = match xdg_config_home {
        Some(x) if Path::new(x).is_absolute() => PathBuf::from(x),
        _ => home.join(".config"),
    };
    base.join("autostart").join("ludotheque.desktop")
}

/// Programme à lancer : l'AppImage elle-même (`$APPIMAGE`, chemin stable) et non le binaire extrait dans
/// `/tmp/.mount_…`, qui disparaît à la fermeture ; hors AppImage, l'exécutable courant.
pub fn autostart_exec(appimage: Option<&str>, current_exe: &Path) -> PathBuf {
    match appimage {
        Some(a) if Path::new(a).is_absolute() => PathBuf::from(a),
        _ => current_exe.to_path_buf(),
    }
}

/// Valeur `Exec` d'une entrée .desktop (spécification Desktop Entry, « Exec key ») : argument entre
/// guillemets doubles si besoin, `"` `` ` `` `$` `\` précédés d'une barre oblique inverse, `%` doublé ;
/// puis la règle générale des chaînes du fichier double chaque barre oblique inverse (`$` → `\\$`).
pub fn desktop_exec(program: &Path) -> String {
    let s = program.to_string_lossy();
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                // Couche « guillemets » : \c ; couche « chaîne du fichier » : la barre devient \\.
                out.push_str("\\\\");
                if c == '\\' {
                    out.push_str("\\\\");
                } else {
                    out.push(c);
                }
            }
            '%' => out.push_str("%%"),
            _ => out.push(c),
        }
    }
    if s.chars().any(|c| c.is_whitespace() || "\"'`$\\><~|&;*?#()".contains(c)) {
        format!("\"{out}\"")
    } else {
        out
    }
}

/// Contenu de l'entrée de démarrage automatique.
pub fn desktop_entry(program: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Ludothèque\nComment=Bibliothèque de jeux et sauvegardes synchronisées\nExec={}\nIcon=ludotheque\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        desktop_exec(program)
    )
}

/// Le démarrage automatique est actif : l'entrée existe et n'est pas désactivée (`Hidden=true`).
pub fn autostart_enabled(path: &Path) -> bool {
    match std::fs::read_to_string(path) {
        Ok(t) => !t.lines().map(str::trim).any(|l| l.eq_ignore_ascii_case("hidden=true") || l.eq_ignore_ascii_case("x-gnome-autostart-enabled=false")),
        Err(_) => false,
    }
}

/// L'entrée active vise-t-elle un autre programme (AppImage déplacée ou renommée) ? Elle est alors réécrite.
pub fn autostart_stale(path: &Path, program: &Path) -> bool {
    let want = format!("Exec={}", desktop_exec(program));
    autostart_enabled(path) && std::fs::read_to_string(path).map(|t| !t.lines().any(|l| l.trim() == want)).unwrap_or(false)
}

/// Active (écrit l'entrée) ou coupe (retire l'entrée écrite par la Ludothèque) le démarrage automatique.
pub fn set_autostart(path: &Path, program: &Path, on: bool) -> Result<(), String> {
    if on {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("Lancement au démarrage impossible : {e}"))?;
        }
        if program.starts_with("/tmp") && program.to_string_lossy().contains("/.mount_") {
            return Err("Lancement au démarrage impossible : programme extrait temporairement (AppImage sans $APPIMAGE)".into());
        }
        write_atomic(path, &desktop_entry(program)).map_err(|e| format!("Lancement au démarrage impossible : {e}"))
    } else {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("Lancement au démarrage non retiré : {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ludo-profils-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn profil_retenu_ecrit_relu_et_valide() {
        let d = dir("retenu");
        let p = remembered_path(&d.join("config.toml"));
        assert_eq!(remembered(&p), None);
        remember(&p, "claire").unwrap();
        assert_eq!(remembered(&p), Some("claire".into()));
        remember(&p, "guilhem").unwrap();
        assert_eq!(remembered(&p), Some("guilhem".into()));
        assert!(remember(&p, "../etc").is_err());
        assert!(remember(&p, "").is_err());
        std::fs::write(&p, "Pas Valide\n").unwrap();
        assert_eq!(remembered(&p), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn chemin_autostart_suit_xdg() {
        let home = Path::new("/home/u");
        assert_eq!(autostart_path(None, home), PathBuf::from("/home/u/.config/autostart/ludotheque.desktop"));
        assert_eq!(autostart_path(Some("/x/cfg"), home), PathBuf::from("/x/cfg/autostart/ludotheque.desktop"));
        // Valeur relative : ignorée (spécification XDG).
        assert_eq!(autostart_path(Some("cfg"), home), PathBuf::from("/home/u/.config/autostart/ludotheque.desktop"));
    }

    #[test]
    fn exec_vise_l_appimage_et_s_echappe() {
        let exe = Path::new("/tmp/.mount_LudoXYZ/usr/bin/ludo-ui");
        assert_eq!(autostart_exec(Some("/home/u/Applications/Ludotheque.AppImage"), exe), PathBuf::from("/home/u/Applications/Ludotheque.AppImage"));
        assert_eq!(autostart_exec(None, exe), exe.to_path_buf());
        assert_eq!(desktop_exec(Path::new("/home/u/Applications/Ludotheque.AppImage")), "/home/u/Applications/Ludotheque.AppImage");
        assert_eq!(desktop_exec(Path::new("/home/u/Mes Jeux/Ludo$1.AppImage")), "\"/home/u/Mes Jeux/Ludo\\\\$1.AppImage\"");
        assert_eq!(desktop_exec(Path::new("/a\\b")), "\"/a\\\\\\\\b\"");
        assert_eq!(desktop_exec(Path::new("/a/100%")), "/a/100%%");
    }

    #[test]
    fn autostart_active_puis_retire() {
        let d = dir("autostart");
        let p = autostart_path(Some(d.to_str().unwrap()), Path::new("/nulle-part"));
        assert!(!autostart_enabled(&p));
        set_autostart(&p, Path::new("/opt/Ludotheque.AppImage"), true).unwrap();
        assert!(autostart_enabled(&p));
        let t = std::fs::read_to_string(&p).unwrap();
        assert!(t.contains("\nExec=/opt/Ludotheque.AppImage\n"), "{t}");
        set_autostart(&p, Path::new("/opt/Ludotheque.AppImage"), false).unwrap();
        assert!(!p.exists());
        // Retirer deux fois n'est pas une erreur.
        set_autostart(&p, Path::new("/opt/Ludotheque.AppImage"), false).unwrap();
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "[Desktop Entry]\nHidden=true\n").unwrap();
        assert!(!autostart_enabled(&p));
        std::fs::write(&p, "[Desktop Entry]\nX-GNOME-Autostart-enabled=false\n").unwrap();
        assert!(!autostart_enabled(&p));
        // AppImage déplacée : entrée périmée détectée.
        set_autostart(&p, Path::new("/opt/Ludotheque.AppImage"), true).unwrap();
        assert!(!autostart_stale(&p, Path::new("/opt/Ludotheque.AppImage")));
        assert!(autostart_stale(&p, Path::new("/home/u/Ludotheque.AppImage")));
        // Binaire extrait dans /tmp/.mount_… : refusé.
        assert!(set_autostart(&p, Path::new("/tmp/.mount_LudoX/usr/bin/ludo-ui"), true).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Essai réel sur le poste (lancé à la main : `LUDO_ESSAI_AUTOSTART=1 cargo test -p ludo-ui -- --ignored
    /// autostart_reel`) : écrit l'entrée dans le vrai `~/.config/autostart` avec `$APPIMAGE`, la relit, la retire.
    #[test]
    #[ignore]
    fn autostart_reel() {
        if std::env::var("LUDO_ESSAI_AUTOSTART").as_deref() != Ok("1") {
            return;
        }
        let home = PathBuf::from(std::env::var("HOME").unwrap());
        let path = autostart_path(std::env::var("XDG_CONFIG_HOME").ok().as_deref(), &home);
        assert!(!path.exists(), "entrée déjà présente : essai annulé pour ne rien écraser");
        let program = autostart_exec(std::env::var("APPIMAGE").ok().as_deref(), Path::new("/inconnu"));
        set_autostart(&path, &program, true).unwrap();
        assert!(autostart_enabled(&path));
        println!("ÉCRIT {}\n{}", path.display(), std::fs::read_to_string(&path).unwrap());
        set_autostart(&path, &program, false).unwrap();
        assert!(!path.exists());
        println!("RETIRÉ {}", path.display());
    }
}
