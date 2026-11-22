//! Appareil Android synchronisé PAR LE PC via adb (AYN Thor, Android 13 sans root) : les applis
//! Android ne peuvent pas lire les sauvegardes des émulateurs (`Android/data/<paquet>`), mais le
//! shell adb (groupe `ext_data_rw`) le peut. Le PC copie donc les dossiers de sauvegarde dans un
//! miroir temporaire, y applique le moteur de synchronisation habituel (verrou, comparaison avec
//! le serveur, conflits), puis repousse le résultat s'il a changé.
//!
//! Garde-fous : rien n'est copié si l'émulateur tourne sur l'appareil ; un dossier remplacé est
//! d'abord renommé en `<dossier>.ludo-old-<horodatage>` (jamais supprimé) ; le nouveau est posé à
//! côté puis mis en place par renommage ; droits ajustés pour que l'émulateur relise ses fichiers.

use crate::config::{self, Config, GameConf};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Appareil joignable par adb (`adb -s <série>`).
#[derive(Debug, Clone)]
pub struct Adb {
    pub serial: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AdbError {
    #[error("adb introuvable ou en échec : {0}")]
    Io(#[from] std::io::Error),
    #[error("adb : {0}")]
    Failed(String),
}

/// Échappe un argument pour le shell de l'appareil (guillemets simples).
fn sq(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn ok(out: Output, what: &str) -> Result<Output, AdbError> {
    if out.status.success() {
        Ok(out)
    } else {
        Err(AdbError::Failed(format!("{what} : {}", String::from_utf8_lossy(&out.stderr).trim())))
    }
}

impl Adb {
    /// Premier appareil de la liste qui répond (`adb connect` tenté pour une adresse `ip:port`).
    pub fn find(series: &[String]) -> Option<Adb> {
        for s in series {
            if s.contains(':') {
                let _ = Command::new("adb").args(["connect", s]).output();
            }
            let a = Adb { serial: s.clone() };
            if a.state().as_deref() == Some("device") {
                return Some(a);
            }
        }
        None
    }

    fn adb(&self) -> Command {
        let mut c = Command::new("adb");
        c.args(["-s", &self.serial]);
        c
    }

    pub fn state(&self) -> Option<String> {
        let out = self.adb().arg("get-state").output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Commande shell sur l'appareil (le texte est passé tel quel : échapper avec `sq`).
    pub fn shell(&self, script: &str) -> Result<String, AdbError> {
        let out = ok(self.adb().args(["shell", script]).output()?, "shell")?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Le paquet a-t-il un processus en cours ?
    pub fn running(&self, package: &str) -> bool {
        self.shell(&format!("pidof {} >/dev/null && echo oui || echo non", sq(package))).is_ok_and(|s| s.trim() == "oui")
    }

    pub fn is_dir(&self, remote: &str) -> bool {
        self.shell(&format!("[ -d {} ] && echo oui || echo non", sq(remote))).is_ok_and(|s| s.trim() == "oui")
    }

    /// Noms (fichiers et dossiers) d'un dossier distant ; vide s'il n'existe pas.
    pub fn list(&self, remote: &str) -> Vec<String> {
        self.shell(&format!("ls -1 {} 2>/dev/null", sq(remote))).map(|s| s.lines().map(str::to_string).filter(|l| !l.is_empty()).collect()).unwrap_or_default()
    }

    /// Copie le dossier distant `remote` vers le dossier local `local` (créé, contenu identique).
    pub fn pull_dir(&self, remote: &str, local: &Path) -> Result<(), AdbError> {
        let parent = local.parent().ok_or_else(|| AdbError::Failed("dossier local sans parent".into()))?;
        std::fs::create_dir_all(parent)?;
        // `adb pull <dossier> <parent>` crée <parent>/<nom du dossier distant>.
        ok(self.adb().args(["pull", "-a", remote]).arg(parent).output()?, "pull")?;
        let pulled = parent.join(Path::new(remote).file_name().ok_or_else(|| AdbError::Failed("chemin distant sans nom".into()))?);
        if pulled != local {
            std::fs::rename(&pulled, local)?;
        }
        Ok(())
    }

    /// Remplace le dossier distant `remote` par le dossier local `local` : envoi à côté, ancien
    /// renommé (gardé), nouveau mis en place, droits groupe (`ext_data_rw`) ajustés.
    pub fn push_replace(&self, local: &Path, remote: &str, stamp: &str) -> Result<(), AdbError> {
        let staging = format!("{remote}.ludo-new");
        let old = format!("{remote}.ludo-old-{stamp}");
        let parent = Path::new(remote).parent().and_then(Path::to_str).ok_or_else(|| AdbError::Failed("chemin distant sans parent".into()))?;
        self.shell(&format!("mkdir -p {}", sq(parent)))?;
        if self.is_dir(&staging) {
            // Reste d'un envoi interrompu : mis de côté, pas supprimé.
            self.shell(&format!("mv {} {}", sq(&staging), sq(&format!("{staging}-{stamp}"))))?;
        }
        // `adb push` ne peut pas créer de dossier dans Android/data (« secure_mkdirs : Operation not
        // permitted ») : dépôt dans /data/local/tmp (à l'utilisateur shell), puis copie par le shell,
        // qui a le droit d'écrire dans Android/data (groupe ext_data_rw).
        let tmp = format!("/data/local/tmp/ludo-{stamp}-{}", std::process::id());
        self.shell(&format!("mkdir -p {}", sq(&tmp)))?;
        let depot = format!("{tmp}/dossier");
        let pushed = ok(self.adb().arg("push").arg(local).arg(&depot).output()?, "push")
            .and_then(|_| self.shell(&format!("cp -r {} {} && chmod -R ug+rwX {}", sq(&depot), sq(&staging), sq(&staging))));
        let _ = self.shell(&format!("rm -r {}", sq(&tmp)));
        pushed?;
        let script = format!(
            "if [ -d {r} ]; then mv {r} {o}; fi && mv {s} {r}",
            r = sq(remote),
            o = sq(&old),
            s = sq(&staging)
        );
        self.shell(&script)?;
        Ok(())
    }
}

/// Paquet Android d'un émulateur, d'après sa commande de lancement sur l'appareil (`bin` de la
/// section `[appareil.<nom>]` de la fiche, ou `[emulateurs.<id>]` de la configuration).
pub fn package_of(cfg: &Config, emulator: &str) -> Option<String> {
    let fiche = cfg.fiche(emulator).ok()?;
    let bin = cfg.emulateurs.get(emulator).map(|s| s.bin.clone()).or_else(|| fiche.device(&cfg.appareil).ok().map(|d| d.bin.clone()))?;
    (!bin.contains('/') && bin.contains('.')).then_some(bin)
}

/// Octets d'une sortie `od -An -tx1` (« 4e 43 43 48 … »).
fn parse_od(text: &str) -> Vec<u8> {
    text.split_whitespace().filter_map(|b| u8::from_str_radix(b, 16).ok()).collect()
}

/// Extension en minuscules d'un chemin de ROM (`None` pour un dossier de jeu Wii U), même règle que
/// `local::match_key` : un nom de dossier peut contenir des points (« Super Smash Bros. for Wii U »).
fn rom_ext(path: &str) -> Option<String> {
    let base = path.trim_end_matches('/').rsplit('/').next().unwrap_or(path);
    let (_, ext) = base.rsplit_once('.')?;
    (!ext.is_empty() && ext.len() <= 4 && ext.chars().all(|c| c.is_ascii_alphanumeric())).then(|| ext.to_ascii_lowercase())
}

impl Adb {
    /// Identifiant du jeu lu dans l'en-tête de la ROM de l'appareil (`ludo_core::rom`) : quelques
    /// centaines d'octets lus par `dd`, rien n'est écrit. Dossier Wii U : `meta/meta.xml`.
    pub fn rom_title_id(&self, remote: &str) -> Option<u64> {
        match rom_ext(remote).filter(|e| ludo_core::rom::HEADER_EXTS.contains(&e.as_str())) {
            Some(ext) => {
                let out = self.shell(&format!("dd if={} bs={} count=1 2>/dev/null | od -An -tx1 -v", sq(remote), ludo_core::rom::HEADER_LEN)).ok()?;
                ludo_core::rom::title_id_from_header(&ext, &parse_od(&out))
            }
            None => ludo_core::rom::title_id_from_meta_xml(&self.shell(&format!("cat {} 2>/dev/null", sq(&format!("{remote}/meta/meta.xml")))).ok()?),
        }
    }
}

/// Correspondance miroir local → chemin distant : `<miroir>/<relatif>` ↔ `<data distant>/<relatif>`.
pub fn remote_of(dir: &Path, mirror: &Path, remote_data: &str) -> Option<String> {
    let rel = dir.strip_prefix(mirror).ok()?;
    let base = remote_data.trim_end_matches('/');
    Some(if rel.as_os_str().is_empty() { base.to_string() } else { format!("{base}/{}", rel.to_string_lossy()) })
}

/// Dossier de données distant d'un émulateur pour cet appareil.
pub fn remote_data(cfg: &Config, emulator: &str) -> Option<String> {
    let fiche = cfg.fiche(emulator).ok()?;
    cfg.emulateurs.get(emulator).map(|s| s.data.clone()).or_else(|| fiche.device(&cfg.appareil).ok().map(|d| d.data.clone()))
}

/// Configuration où l'émulateur pointe sur le miroir local (le moteur travaille sur des copies).
pub fn mirrored(cfg: &Config, emulator: &str, mirror: &Path) -> Option<Config> {
    let fiche = cfg.fiche(emulator).ok()?;
    let bin = cfg.emulateurs.get(emulator).map(|s| s.bin.clone()).or_else(|| fiche.device(&cfg.appareil).ok().map(|d| d.bin.clone()))?;
    let mut c = cfg.clone();
    c.emulateurs.insert(emulator.to_string(), ludo_core::fiche::DeviceSettings { bin, data: mirror.display().to_string() });
    Some(c)
}

/// Prépare (émulateur, profil, jeu) sur le miroir et donne les dossiers à recopier.
pub fn prepare_mirrored(cfg: &Config, conf: &GameConf, profile: &str, mirror: &Path) -> Result<(config::Prepared, Vec<(PathBuf, String)>), String> {
    prepare_mirrored_rom(cfg, conf, profile, mirror, None)
}

/// Comme [`prepare_mirrored`], avec le chemin de la ROM sur l'appareil (`{rom_nom}` de melonDS).
pub fn prepare_mirrored_rom(cfg: &Config, conf: &GameConf, profile: &str, mirror: &Path, rom: Option<&Path>) -> Result<(config::Prepared, Vec<(PathBuf, String)>), String> {
    let data = remote_data(cfg, &conf.emulateur).ok_or_else(|| format!("{} : dossier de données de l'appareil inconnu", conf.emulateur))?;
    let mc = mirrored(cfg, &conf.emulateur, mirror).ok_or_else(|| format!("{} : fiche ou lanceur inconnu pour cet appareil", conf.emulateur))?;
    let prepared = config::prepare_rom(&mc, &conf.emulateur, profile, &conf.title_id, rom).map_err(|e| e.to_string())?;
    let pairs = prepared.dirs.iter().filter_map(|d| remote_of(d, mirror, &data).map(|r| (d.clone(), r))).collect();
    Ok((prepared, pairs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echappement_shell() {
        assert_eq!(sq("/sdcard/a b"), "'/sdcard/a b'");
        assert_eq!(sq("l'œil"), r"'l'\''œil'");
    }

    #[test]
    fn miroir_et_chemins_distants() {
        let cfg: Config = toml::from_str(
            r#"
            serveur = "x"
            appareil = "thor"
            [profils.guilhem]
            eden_uuid = "A2D2DFD2700F51C158E87A336C77FE55"
            eden_index = 0
            "#,
        )
        .unwrap();
        assert_eq!(package_of(&cfg, "eden").as_deref(), Some("dev.eden.eden_emulator"));
        let data = remote_data(&cfg, "eden").unwrap();
        assert_eq!(data, "/sdcard/Android/data/dev.eden.eden_emulator/files");
        let t = tempfile::tempdir().unwrap();
        let conf = GameConf { emulateur: "eden".into(), title_id: "0100152000022000".into(), profils: None };
        let (_, pairs) = prepare_mirrored(&cfg, &conf, "guilhem", t.path()).unwrap();
        let remotes: Vec<&str> = pairs.iter().map(|(_, r)| r.as_str()).collect();
        assert_eq!(
            remotes,
            [
                "/sdcard/Android/data/dev.eden.eden_emulator/files/nand/user/save/0000000000000000/A2D2DFD2700F51C158E87A336C77FE55/0100152000022000",
                "/sdcard/Android/data/dev.eden.eden_emulator/files/nand/user/save/0000000000000000/00000000000000000000000000000000/0100152000022000",
            ]
        );
        assert!(pairs.iter().all(|(l, _)| l.starts_with(t.path())), "le moteur travaille sur le miroir local");
    }

    /// Chemins distants de chaque nouvelle fiche sur la Thor : ceux relevés par adb le 21/11/2026.
    #[test]
    fn fiches_3ds_wiiu_gc_ds_vers_les_chemins_mesures_sur_la_thor() {
        let cfg: Config = toml::from_str(
            r#"
            serveur = "x"
            appareil = "thor"
            profil_defaut = "guilhem"
            [profils.guilhem]
            eden_uuid = "A2D2DFD2700F51C158E87A336C77FE55"
            eden_index = 0
            "#,
        )
        .unwrap();
        let remote = |emu: &str, tid: &str, rom: Option<&str>| {
            let t = tempfile::tempdir().unwrap();
            let conf = GameConf { emulateur: emu.into(), title_id: tid.into(), profils: None };
            let (p, pairs) = prepare_mirrored_rom(&cfg, &conf, "guilhem", t.path(), rom.map(Path::new)).unwrap();
            assert!(pairs.iter().all(|(l, _)| l.starts_with(t.path())));
            (pairs.into_iter().map(|(_, r)| r).collect::<Vec<_>>(), p)
        };
        // Azahar : Majora's Mask 3D, dossier relevé `title/00040000/00125600/data`.
        let (r, _) = remote("azahar", "0004000000125600", None);
        assert_eq!(r, ["/sdcard/Documents/Azahar/sdmc/Nintendo 3DS/00000000000000000000000000000000/00000000000000000000000000000000/title/00040000/00125600/data"]);
        assert_eq!(package_of(&cfg, "azahar").as_deref(), Some("org.azahar_emu.azahar"));
        // Cemu : Wind Waker HD.
        let (r, _) = remote("cemu", "0005000010143500", None);
        assert_eq!(r, ["/sdcard/Android/data/info.cemu.cemu/files/mlc01/usr/save/00050000/10143500/user"]);
        // Dolphin : Twilight Princess (GZ2P01, Europe), seuls ses fichiers GCI.
        let (r, p) = remote("dolphin", "0000475a32503031", None);
        assert_eq!(r, ["/sdcard/Android/data/org.dolphinemu.dolphinemu/files/GC/EUR/Card A"]);
        assert_eq!(p.vars["code_jeu"], "GZ2P01");
        // melonDS : fichier nommé comme la ROM dans le dossier de sauvegardes de l'appli.
        let rom = "/sdcard/Documents/Roms/nds/Pokemon - Version Diamant (France) (Rev 5).nds";
        let (r, _) = remote("melonds", "0000000041444146", Some(rom));
        assert_eq!(r, ["/sdcard/Android/data/me.magnum.melondualds/files/saves"]);
        assert_eq!(package_of(&cfg, "melonds").as_deref(), Some("me.magnum.melondualds"));
    }

    #[test]
    fn chemin_distant_du_miroir_lui_meme() {
        let m = Path::new("/tmp/miroir");
        assert_eq!(remote_of(m, m, "/sdcard/x/").as_deref(), Some("/sdcard/x"));
        assert_eq!(remote_of(&m.join("a/b"), m, "/sdcard/x").as_deref(), Some("/sdcard/x/a/b"));
    }

    #[test]
    fn identifiant_lu_dans_les_octets_d_od() {
        assert_eq!(parse_od(" 4e 43 43 48 98 17\n 00 8f\n"), vec![0x4e, 0x43, 0x43, 0x48, 0x98, 0x17, 0x00, 0x8f]);
        assert!(parse_od("").is_empty());
        assert_eq!(rom_ext("/a/b/Jeu (Europe).CXI").as_deref(), Some("cxi"));
        assert_eq!(rom_ext("/a/b/The Legend of Zelda The Wind Waker HD"), None);
        assert_eq!(rom_ext("/a/b/Super Smash Bros. for Wii U"), None);
        assert_eq!(rom_ext("/a/b/Mario Kart 8 v1.1"), Some("1".into()), "même règle que match_key (≤ 4 alphanumériques)");
    }
}
