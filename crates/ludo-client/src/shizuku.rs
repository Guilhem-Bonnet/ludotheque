//! Shizuku sur l'appareil Android (lot arret, 23/11/2026) : « Fermer le jeu » de la Ludothèque arrête
//! l'émulateur sans aucune page d'Android (`ArretDirect.kt`), grâce au serveur Shizuku qui tourne à
//! l'identité shell (adb).
//!
//! Le PC, à chaque connexion de l'appareil (`ludo adb-sync`, `ludo shizuku`), rend ce chemin prêt,
//! de façon idempotente :
//! - **accès des apps Ludothèque** : entrée « autorisée » dans `shizuku.json`, le fichier que le serveur
//!   lit à son démarrage (`ShizukuConfigManager` de la fork thedjchi, `FLAG_ALLOWED = 1 shl 1`). Le
//!   fichier appartient à l'uid shell : adb l'écrit. Les autres apps du fichier restent intactes ;
//! - **tenue sans le PC** : droits d'arrière-plan de Shizuku et port adb TCP persistant
//!   (`persist.adb.tcp.port`) : au démarrage, Shizuku se relance seul en mode TCP, sans appairage du
//!   débogage sans fil, si « Start on boot » est actif dans l'app. Cet interrupteur (composant
//!   `BootCompleteReceiver`) n'est que CONSTATÉ : Android 13 refuse au shell de changer l'état d'un
//!   composant d'une autre app (« Shell cannot change component state », mesuré le 23/11) ;
//! - **serveur démarré** s'il ne tourne pas, ou redémarré si l'accès vient d'être ajouté (le serveur ne
//!   relit pas son fichier en marche). Le démarreur de Shizuku remplace lui-même l'ancien serveur.
//!
//! Rien n'est supprimé : une sauvegarde `.bak` du fichier (qui primerait sur lui) est renommée en
//! `.ludo-old-<horodatage>`.

use crate::adb::{sq, Adb, AdbError, LUDO_PACKAGES};
use serde_json::{json, Value};

/// Paquet de Shizuku (fork thedjchi comme officiel).
pub const PAQUET: &str = "moe.shizuku.privileged.api";
/// Fichier des autorisations, lu par le serveur à son démarrage.
pub const CONFIG: &str = "/data/user_de/0/com.android.shell/shizuku.json";
/// Nom du processus serveur (`starter.cpp`, `SERVER_NAME`).
pub const SERVEUR: &str = "shizuku_server";
/// Composant activé par l'interrupteur « Start on boot » de Shizuku.
pub const DEMARRAGE_AU_BOOT: &str = "moe.shizuku.manager.receiver.BootCompleteReceiver";
/// Port adb TCP rendu persistant (celui que le PC utilise déjà en Wi-Fi).
pub const PORT_TCP: u16 = 5555;

const FLAG_ALLOWED: u64 = 1 << 1;
const FLAG_DENIED: u64 = 1 << 2;

/// État de Shizuku lu sur l'appareil (un seul appel adb).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Etat {
    /// Chemin de l'APK de Shizuku ; `None` : non installé.
    pub apk: Option<String>,
    /// PID du serveur s'il tourne.
    pub pid: Option<u32>,
    /// Apps Ludothèque installées et leur uid.
    pub apps: Vec<(String, u32)>,
    /// Contenu actuel de `shizuku.json` (vide s'il n'existe pas).
    pub config: String,
    /// Une sauvegarde `shizuku.json.bak` existe (elle primerait au prochain démarrage du serveur).
    pub bak: bool,
}

/// Script de lecture : lignes `clé=valeur`, puis le fichier après le marqueur `--config--`.
pub fn script_etat() -> String {
    let apps = LUDO_PACKAGES.join(" ");
    format!(
        r#"p="$(pm path {PAQUET} 2>/dev/null | head -n 1)"; echo "apk=${{p#package:}}"
echo "pid=$(pidof {SERVEUR})"
for a in {apps}; do cmd package list packages -U "$a" 2>/dev/null | grep -x "package:$a uid:[0-9]*"; done
[ -f {CONFIG}.bak ] && echo "bak=1"
echo "--config--"
cat {CONFIG} 2>/dev/null; true"#
    )
}

/// Lit la sortie de [`script_etat`].
pub fn lire_etat(sortie: &str) -> Etat {
    let (tete, config) = sortie.split_once("--config--").unwrap_or((sortie, ""));
    let mut e = Etat { config: config.trim().to_string(), ..Etat::default() };
    for l in tete.lines().map(str::trim) {
        if let Some(v) = l.strip_prefix("apk=") {
            e.apk = Some(v.to_string()).filter(|v| v.ends_with(".apk"));
        } else if let Some(v) = l.strip_prefix("pid=") {
            e.pid = v.split_whitespace().next().and_then(|p| p.parse().ok());
        } else if l == "bak=1" {
            e.bak = true;
        } else if let Some(v) = l.strip_prefix("package:") {
            if let Some((p, u)) = v.split_once(" uid:") {
                if let Ok(uid) = u.parse() {
                    e.apps.push((p.to_string(), uid));
                }
            }
        }
    }
    e
}

/// Fichier des autorisations avec chaque app de `apps` autorisée. `None` : rien à changer. Erreur :
/// fichier illisible (il n'est alors jamais réécrit).
pub fn fusion(actuel: &str, apps: &[(String, u32)]) -> Result<Option<String>, String> {
    let origine: Value = if actuel.trim().is_empty() {
        json!({ "version": 2, "packages": [] })
    } else {
        serde_json::from_str(actuel).map_err(|e| format!("{CONFIG} illisible ({e}) : rien écrit"))?
    };
    let mut v = origine.clone();
    let entrees = v
        .as_object_mut()
        .ok_or("racine inattendue")?
        .entry("packages")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| format!("{CONFIG} : « packages » n'est pas une liste : rien écrit"))?;
    for (paquet, uid) in apps {
        match entrees.iter_mut().find(|e| e.get("uid").and_then(Value::as_u64) == Some(u64::from(*uid))) {
            Some(e) => {
                let flags = e.get("flags").and_then(Value::as_u64).unwrap_or(0);
                e["flags"] = json!((flags & !(FLAG_ALLOWED | FLAG_DENIED)) | FLAG_ALLOWED);
                let liste = e.as_object_mut().ok_or("entrée inattendue")?.entry("packages").or_insert_with(|| json!([]));
                if let Some(l) = liste.as_array_mut() {
                    if !l.iter().any(|p| p.as_str() == Some(paquet)) {
                        l.push(json!(paquet));
                    }
                }
            }
            None => entrees.push(json!({ "uid": uid, "flags": FLAG_ALLOWED, "packages": [paquet] })),
        }
    }
    Ok((v != origine).then(|| v.to_string()))
}

/// Script de mise en place, d'après l'état lu. `nouvelle` : fichier des autorisations à écrire.
pub fn script_preparation(e: &Etat, nouvelle: Option<&str>) -> Option<String> {
    let apk = e.apk.as_deref()?;
    let dossier = apk.rsplit_once('/').map_or(apk, |(d, _)| d);
    let mut s = format!(
        r#"pm grant {PAQUET} android.permission.WRITE_SECURE_SETTINGS 2>/dev/null
appops set {PAQUET} RUN_IN_BACKGROUND allow
appops set {PAQUET} RUN_ANY_IN_BACKGROUND allow
dumpsys deviceidle whitelist +{PAQUET} >/dev/null 2>&1
if dumpsys package {PAQUET} | sed -n '/^ *enabledComponents:/,/^ *disabledComponents:/p' | grep -q {DEMARRAGE_AU_BOOT}; then echo "démarrage au boot : actif"; else echo "démarrage au boot : inactif (Shizuku › Réglages › Start on boot)"; fi
[ -n "$(getprop persist.adb.tcp.port)" ] || setprop persist.adb.tcp.port {PORT_TCP}
echo "port adb persistant : $(getprop persist.adb.tcp.port)"
"#
    );
    let demarrer = match nouvelle {
        Some(cfg) => {
            if let Some(pid) = e.pid {
                // L'ancien serveur ne doit pas réécrire son fichier par-dessus le nouveau.
                s += &format!("kill -9 {pid} 2>/dev/null\n");
            }
            if e.bak {
                s += &format!("mv {CONFIG}.bak {CONFIG}.bak.ludo-old-$(date +%Y%m%d-%H%M%S)\n");
            }
            s += &format!("printf '%s' {} > {CONFIG}.ludo-tmp && mv {CONFIG}.ludo-tmp {CONFIG} && echo \"accès accordé : {}\"\n", sq(cfg), e.apps.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>().join(", "));
            true
        }
        None => e.pid.is_none(),
    };
    if demarrer {
        // Le démarreur rend la main une fois le serveur lancé et donne son PID (`starter.cpp`).
        s += &format!("r=\"$({dossier}/lib/arm64/libshizuku.so 2>&1)\"; n=\"$(echo \"$r\" | sed -n 's/.*pid is \\([0-9]*\\).*/\\1/p')\"\n");
        s += "if [ -n \"$n\" ]; then echo \"serveur : démarré (pid $n)\"; else echo \"serveur : échec du démarrage ($(echo \"$r\" | tail -n 1))\"; fi\n";
    } else {
        s += "echo \"serveur : déjà démarré, accès déjà accordé\"\n";
    }
    Some(s)
}

/// Prépare Shizuku sur l'appareil (voir l'en-tête du module). Rend les lignes à afficher ; Shizuku
/// absent : une ligne qui le dit, rien d'autre.
pub fn preparer(dev: &Adb) -> Result<Vec<String>, AdbError> {
    let etat = lire_etat(&dev.shell(&script_etat())?);
    if etat.apk.is_none() {
        return Ok(vec!["Shizuku absent : arrêt direct indisponible (scripts/shizuku-thor.sh l'installe)".into()]);
    }
    let nouvelle = fusion(&etat.config, &etat.apps).map_err(AdbError::Failed)?;
    let script = script_preparation(&etat, nouvelle.as_deref()).unwrap_or_default();
    Ok(dev.shell(&script)?.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apps() -> Vec<(String, u32)> {
        vec![("fr.srvdreamer.ludotheque".into(), 10154), ("fr.srvdreamer.ludotheque.debug".into(), 10153)]
    }

    #[test]
    fn fichier_absent_puis_idempotent() {
        let n = fusion("", &apps()).unwrap().expect("écriture attendue");
        let v: Value = serde_json::from_str(&n).unwrap();
        assert_eq!(v["version"], 2);
        assert_eq!(v["packages"][0], json!({"uid": 10154, "flags": 2, "packages": ["fr.srvdreamer.ludotheque"]}));
        assert_eq!(v["packages"][1]["uid"], 10153);
        // Deuxième passage : rien à écrire.
        assert_eq!(fusion(&n, &apps()).unwrap(), None);
    }

    #[test]
    fn autres_apps_gardees_et_refus_leve() {
        let actuel = r#"{"version":2,"packages":[{"uid":10200,"flags":2,"packages":["autre.app"]},{"uid":10154,"flags":4,"packages":["fr.srvdreamer.ludotheque"]}]}"#;
        let n = fusion(actuel, &apps()[..1]).unwrap().unwrap();
        let v: Value = serde_json::from_str(&n).unwrap();
        assert_eq!(v["packages"][0], json!({"uid": 10200, "flags": 2, "packages": ["autre.app"]}));
        // Refus antérieur (FLAG_DENIED) remplacé par l'autorisation.
        assert_eq!(v["packages"][1]["flags"], 2);
        assert_eq!(v["packages"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn fichier_illisible_jamais_reecrit() {
        assert!(fusion("{pas du json", &apps()).is_err());
        assert!(fusion(r#"{"packages":3}"#, &apps()).is_err());
    }

    #[test]
    fn lecture_de_l_etat() {
        let sortie = "apk=/data/app/~~a==/moe.shizuku.privileged.api-b==/base.apk\npid=4321\npackage:fr.srvdreamer.ludotheque uid:10154\nbak=1\n--config--\n{\"version\":2,\"packages\":[]}\n";
        let e = lire_etat(sortie);
        assert_eq!(e.apk.as_deref(), Some("/data/app/~~a==/moe.shizuku.privileged.api-b==/base.apk"));
        assert_eq!(e.pid, Some(4321));
        assert_eq!(e.apps, vec![("fr.srvdreamer.ludotheque".to_string(), 10154)]);
        assert!(e.bak);
        assert_eq!(e.config, "{\"version\":2,\"packages\":[]}");
        let absent = lire_etat("apk=\npid=\n--config--\n");
        assert_eq!(absent, Etat::default());
    }

    #[test]
    fn preparation_selon_l_etat() {
        let base = Etat { apk: Some("/data/app/x/moe.shizuku.privileged.api-y/base.apk".into()), apps: apps(), ..Etat::default() };
        // Absent : aucun script.
        assert_eq!(script_preparation(&Etat::default(), None), None);
        // Serveur arrêté, accès déjà là : démarrage seul.
        let s = script_preparation(&base, None).unwrap();
        assert!(s.contains("/data/app/x/moe.shizuku.privileged.api-y/lib/arm64/libshizuku.so"));
        assert!(!s.contains("kill") && !s.contains("printf"));
        assert!(s.contains("grep -q moe.shizuku.manager.receiver.BootCompleteReceiver") && !s.contains("pm enable"));
        // Serveur en marche, accès déjà là : rien n'est relancé.
        let marche = Etat { pid: Some(77), ..base.clone() };
        let s = script_preparation(&marche, None).unwrap();
        assert!(!s.contains("libshizuku.so") && s.contains("déjà démarré"));
        // Accès ajouté, serveur en marche, sauvegarde .bak : arrêt, renommage (jamais supprimé), écriture, démarrage.
        let s = script_preparation(&Etat { bak: true, ..marche }, Some(r#"{"a":1}"#)).unwrap();
        let (k, b, w, d) = (s.find("kill -9 77").unwrap(), s.find(".bak.ludo-old-").unwrap(), s.find("printf '%s' '{\"a\":1}'").unwrap(), s.find("libshizuku.so").unwrap());
        assert!(k < b && b < w && w < d);
        assert!(!s.contains("rm "));
    }

    /// Scripts exécutés par un `sh` local avec des commandes Android simulées : syntaxe valide et
    /// fichier écrit à l'endroit voulu.
    #[test]
    fn scripts_executables() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().display().to_string();
        let mocks = format!(
            "pm() {{ case \"$1\" in path) echo package:/data/app/x/{PAQUET}-y/base.apk ;; esac; }}\npidof() {{ return 1; }}\ncmd() {{ echo \"package:$5 uid:10154\"; }}\nappops() {{ :; }}\ndumpsys() {{ :; }}\ngetprop() {{ echo 5555; }}\nsetprop() {{ :; }}\n"
        );
        let out = std::process::Command::new("sh").arg("-c").arg(format!("{mocks}{}", script_etat().replace(CONFIG, &format!("{d}/c.json")))).output().unwrap();
        assert!(out.status.success());
        let e = lire_etat(&String::from_utf8_lossy(&out.stdout));
        assert_eq!(e.apk.as_deref(), Some("/data/app/x/moe.shizuku.privileged.api-y/base.apk"));
        assert_eq!(e.apps.len(), LUDO_PACKAGES.len());
        let n = fusion(&e.config, &e.apps).unwrap();
        let s = script_preparation(&e, n.as_deref()).unwrap().replace(CONFIG, &format!("{d}/c.json")).replace("/data/app/x/moe.shizuku.privileged.api-y/lib/arm64/libshizuku.so", "true");
        let out = std::process::Command::new("sh").arg("-c").arg(format!("{mocks}{s}")).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let ecrit = std::fs::read_to_string(dir.path().join("c.json")).unwrap();
        assert_eq!(fusion(&ecrit, &e.apps).unwrap(), None, "fichier écrit = fichier voulu");
    }
}
