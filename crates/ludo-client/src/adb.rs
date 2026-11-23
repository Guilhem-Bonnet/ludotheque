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

    /// Accès « tous les fichiers » accordé aux apps Ludothèque installées (release et essai) : elles
    /// lancent les jeux par le chemin de l'inventaire, sans jamais demander l'accès à un dossier. Rend les
    /// paquets traités. Sans effet sur une version qui ne déclare pas la permission (avant 0.8.1).
    pub fn grant_ludo_files(&self) -> Result<Vec<String>, AdbError> {
        Ok(self.shell(&grant_script())?.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
    }

    /// Retour à la Ludothèque en un appui après une partie (lot menu-jeu2) : service d'accessibilité
    /// `RetourService` AJOUTÉ à ceux déjà actifs (celui d'AYN reste), `SYSTEM_ALERT_WINDOW` accordé, HOME
    /// en un appui (`prevent_press_home_accidentally` = 0, valeur d'origine notée une fois dans
    /// `Android/data/<paquet>/files/reglages-origine.txt`). Idempotent. Rend les lignes du script
    /// (valeur relue des services actifs).
    pub fn activer_retour(&self) -> Result<Vec<String>, AdbError> {
        Ok(self.shell(&retour_script("/sdcard/Android/data"))?.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
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

    /// Comme [`Adb::rom_title_id`] pour plusieurs fichiers à en-tête (pas les dossiers Wii U), en UN
    /// seul appel `adb shell` : un bloc d'octets par fichier, séparés par une ligne `=`. Rend un
    /// résultat par chemin, dans l'ordre ; `None` pour un fichier illisible ou sans identifiant.
    pub fn rom_title_ids(&self, remotes: &[String]) -> Vec<Option<u64>> {
        let readable: Vec<(usize, String)> = remotes
            .iter()
            .enumerate()
            .filter_map(|(i, r)| rom_ext(r).filter(|e| ludo_core::rom::HEADER_EXTS.contains(&e.as_str())).map(|e| (i, e)))
            .collect();
        let mut out = vec![None; remotes.len()];
        if readable.is_empty() {
            return out;
        }
        // Par paquets : une commande `sh -c` trop longue serait refusée (limite d'un argument).
        for chunk in readable.chunks(100) {
            let script: String = chunk
                .iter()
                .map(|(i, _)| format!("dd if={} bs={} count=1 2>/dev/null | od -An -tx1 -v; echo =;", sq(&remotes[*i]), ludo_core::rom::HEADER_LEN))
                .collect();
            match self.shell(&script) {
                Ok(text) => {
                    for ((i, ext), block) in chunk.iter().zip(split_blocks(&text)) {
                        out[*i] = ludo_core::rom::title_id_from_header(ext, &parse_od(&block));
                    }
                }
                Err(e) => eprintln!("lecture des en-têtes de ROM impossible : {e}"),
            }
        }
        out
    }
}

/// Blocs de sortie de [`Adb::rom_title_ids`] : une ligne `=` termine chaque bloc (un fichier
/// illisible donne un bloc vide). Les fins de ligne `\r\n` d'anciens adb sont tolérées.
fn split_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        if line.trim() == "=" {
            blocks.push(std::mem::take(&mut cur));
        } else {
            cur.push_str(line);
            cur.push(' ');
        }
    }
    blocks
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

/// Paquets de l'app Ludothèque sur un appareil Android (release, puis version d'essai).
pub const LUDO_PACKAGES: &[&str] = &["fr.srvdreamer.ludotheque", "fr.srvdreamer.ludotheque.debug"];

/// Script shell : `appops set … MANAGE_EXTERNAL_STORAGE allow` pour chaque app Ludothèque installée.
pub fn grant_script() -> String {
    LUDO_PACKAGES
        .iter()
        .map(|p| format!("pm path {p} >/dev/null 2>&1 && appops set {p} MANAGE_EXTERNAL_STORAGE allow && echo {p}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Classe du service de retour après une partie (`RetourService.kt`), commune aux deux paquets.
pub const RETOUR_SERVICE: &str = "fr.srvdreamer.ludotheque.RetourService";

/// Script shell de l'appareil pour [`Adb::activer_retour`] : première app Ludothèque installée (release,
/// sinon essai). La liste `enabled_accessibility_services` est lue, complétée (séparateur `:`) et réécrite
/// en entier, puis relue : un service déjà présent (AYN) n'est jamais retiré ni remplacé. `data` : dossier
/// `Android/data` (paramètre pour les tests).
pub fn retour_script(data: &str) -> String {
    let pkgs = LUDO_PACKAGES.join(" ");
    format!(
        r#"for p in {pkgs}; do pm path "$p" >/dev/null 2>&1 || continue
c="$p/{RETOUR_SERVICE}"
v="$(settings get secure enabled_accessibility_services)"
case ":$v:" in
  *":$c:"*) ;;
  *) if [ -z "$v" ] || [ "$v" = null ]; then n="$c"; else n="${{v}}:${{c}}"; fi
     settings put secure enabled_accessibility_services "$n" ;;
esac
settings put secure accessibility_enabled 1
appops set "$p" SYSTEM_ALERT_WINDOW allow
d="{data}/$p/files"
mkdir -p "$d"
[ -f "$d/reglages-origine.txt" ] || echo "prevent_press_home_accidentally=$(settings get system prevent_press_home_accidentally)" > "$d/reglages-origine.txt"
settings put system prevent_press_home_accidentally 0
echo "services : $(settings get secure enabled_accessibility_services)"
break
done"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exécute `retour_script` avec un `sh` local et des commandes Android simulées (réglages en fichiers).
    fn retour_simule(services: &str, installes: &[&str]) -> (String, String, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::write(d.join("secure.enabled_accessibility_services"), services).unwrap();
        std::fs::write(d.join("system.prevent_press_home_accidentally"), "1").unwrap();
        let mocks = format!(
            r#"R={r}
settings() {{ if [ "$1" = get ]; then cat "$R/$2.$3" 2>/dev/null || echo null; else printf %s "$4" > "$R/$2.$3"; fi; }}
pm() {{ case " {inst} " in *" $2 "*) return 0 ;; *) return 1 ;; esac; }}
appops() {{ echo "$2 $3 $4" >> "$R/appops"; }}
"#,
            r = d.display(),
            inst = installes.join(" ")
        );
        let script = format!("{mocks}{}", retour_script(&d.join("data").display().to_string()));
        let out = Command::new("sh").arg("-c").arg(&script).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let lire = |n: &str| std::fs::read_to_string(d.join(n)).unwrap_or_default();
        let origine = std::fs::read_to_string(d.join("data/fr.srvdreamer.ludotheque/files/reglages-origine.txt")).unwrap_or_default();
        (lire("secure.enabled_accessibility_services"), lire("system.prevent_press_home_accidentally"), lire("appops"), origine)
    }

    #[test]
    fn retour_ajoute_le_service_sans_retirer_celui_d_ayn() {
        let ayn = "com.odin.gameassistant/com.ro.gameassistant.service.ForegroundAppMonitorV4Service";
        let (s, home, appops, origine) = retour_simule(ayn, &["fr.srvdreamer.ludotheque", "fr.srvdreamer.ludotheque.debug"]);
        assert_eq!(s, format!("{ayn}:fr.srvdreamer.ludotheque/fr.srvdreamer.ludotheque.RetourService"));
        assert_eq!(home, "0");
        assert_eq!(appops.trim(), "fr.srvdreamer.ludotheque SYSTEM_ALERT_WINDOW allow");
        assert_eq!(origine.trim(), "prevent_press_home_accidentally=1");
    }

    #[test]
    fn retour_idempotent_et_liste_vide() {
        let c = "fr.srvdreamer.ludotheque/fr.srvdreamer.ludotheque.RetourService";
        let deja = format!("a/b.C:{c}");
        assert_eq!(retour_simule(&deja, &["fr.srvdreamer.ludotheque"]).0, deja);
        assert_eq!(retour_simule("null", &["fr.srvdreamer.ludotheque"]).0, c);
        // Seule l'app d'essai installée : son propre composant.
        assert_eq!(retour_simule("", &["fr.srvdreamer.ludotheque.debug"]).0, "fr.srvdreamer.ludotheque.debug/fr.srvdreamer.ludotheque.RetourService");
        // Aucune app Ludothèque : rien ne change.
        assert_eq!(retour_simule("x/y.Z", &[]).0, "x/y.Z");
    }

    #[test]
    fn acces_fichiers_des_apps_ludotheque() {
        let s = grant_script();
        assert_eq!(s.matches("appops set").count(), 2);
        assert!(s.contains("pm path fr.srvdreamer.ludotheque >/dev/null 2>&1 && appops set fr.srvdreamer.ludotheque MANAGE_EXTERNAL_STORAGE allow"));
        assert!(s.contains("fr.srvdreamer.ludotheque.debug MANAGE_EXTERNAL_STORAGE allow"));
    }

    /// Sortie de `rom_title_ids` : un bloc `od` par fichier, fichier illisible = bloc vide, `\r\n` toléré.
    #[test]
    fn blocs_de_lecture_groupee() {
        let text = " 4e 43\n 43 48\n=\n=\r\n 01 02\r\n=\n";
        let b = split_blocks(text);
        assert_eq!(b.len(), 3);
        assert_eq!(parse_od(&b[0]), vec![0x4e, 0x43, 0x43, 0x48]);
        assert!(parse_od(&b[1]).is_empty());
        assert_eq!(parse_od(&b[2]), vec![1, 2]);
    }

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
