//! « Fermer le jeu » pour un jeu joué EN FLUX depuis l'appareil Android (Moonlight → Sunshine de ce PC),
//! lot arret du 23/11/2026.
//!
//! Arrêter Moonlight sur la Thor n'est qu'une déconnexion : Sunshine garde l'appli ouverte et le jeu
//! continue sur le PC. Il faut l'équivalent de « Quitter l'appli » de Sunshine : terminer l'appli qu'il a
//! lancée. Sunshine lance chaque appli dans son propre groupe de processus et la termine par SIGTERM au
//! groupe ; le PC fait exactement cela, sans identifiant de l'interface web de Sunshine. Pour une appli
//! `ludo run`, SIGTERM laisse à l'émulateur `EMULATOR_GRACE` pour écrire sa sauvegarde, puis la
//! synchro part (comme une fin de flux) ; Sunshine voit l'appli finie et clôt la session.
//!
//! Déclencheur EXPLICITE, jamais une simple mort de Moonlight (coupure réseau, Android qui le tue) :
//! la Ludothèque écrit la ligne de journal `LudoFlux: fermer <jeu>` ; `ludo adb-watch` la reçoit par un
//! flux `logcat -v uid -T 1` (événement poussé, seulement les lignes nouvelles) et n'accepte que les
//! lignes écrites par l'uid d'une app Ludothèque (une autre app ne peut pas déclencher la fermeture).
//!
//! Pourquoi pas un fichier dans `Android/data/<paquet>` : mesuré sur la Thor le 23/11, l'inotifyd du
//! shell ne voit pas les écritures qu'une app fait dans son propre dossier (montage qui contourne FUSE ;
//! seules les écritures du shell produisent `n`/`w`, et un renommage ne produit jamais `m`).

use crate::adb::LUDO_PACKAGES;

/// Étiquette de journal de la demande (`MenuJeuPlugin.demanderFinDuFlux`).
pub const ETIQUETTE: &str = "LudoFlux";

/// Flux des demandes : lignes nouvelles seulement, avec l'uid de l'app qui écrit.
pub fn script_logcat() -> String {
    format!("logcat -v uid -T 1 -s {ETIQUETTE}:I")
}

/// Script : uid des apps Ludothèque installées (« package:<paquet> uid:<n> »).
pub fn script_uids() -> String {
    let apps = LUDO_PACKAGES.join(" ");
    format!(r#"for a in {apps}; do cmd package list packages -U "$a" 2>/dev/null | grep -x "package:$a uid:[0-9]*"; done; true"#)
}

/// Uid des apps Ludothèque d'après la sortie de [`script_uids`].
pub fn lire_uids(sortie: &str) -> Vec<u32> {
    sortie.lines().filter_map(|l| l.trim().rsplit_once(" uid:")?.1.parse().ok()).collect()
}

/// Ligne de journal candidate (filtrée par étiquette) ; l'uid est vérifié par [`demande`].
pub fn est_demande(ligne: &str) -> bool {
    ligne.contains(&format!(" {ETIQUETTE}: fermer"))
}

/// Jeu demandé si la ligne (`logcat -v uid` : « date heure uid pid tid niveau étiquette: message »)
/// vient d'une app Ludothèque (`uids`).
pub fn demande(ligne: &str, uids: &[u32]) -> Option<String> {
    let uid: u32 = ligne.split_whitespace().nth(2)?.parse().ok()?;
    if !uids.contains(&uid) {
        return None;
    }
    let message = &ligne[ligne.find(&format!(" {ETIQUETTE}: fermer"))? + ETIQUETTE.len() + 3..];
    Some(message.trim_start_matches("fermer").trim().to_string())
}

/// Processus vu dans `/proc` : (pid, ppid, groupe, nom, `SUNSHINE_CLIENT_NAME` de son environnement).
pub type Proc = (i32, i32, i32, String, Option<String>);

/// Cibles : groupes de processus des enfants directs de Sunshine (une appli lancée par Sunshine est
/// chef de son groupe ; à défaut, le processus seul, signalé par un pid positif), et SEULEMENT ceux
/// dont le client Sunshine est cet appareil (`SUNSHINE_CLIENT_NAME` qui contient `appareil`, ex.
/// « AYN Thor » pour « thor ») : la partie d'un autre client (Deck, iPhone) n'est jamais touchée,
/// pas plus qu'un processus sans ce nom.
pub fn cibles(procs: &[Proc], appareil: &str) -> Vec<(i32, String)> {
    let sunshine: Vec<i32> = procs.iter().filter(|p| p.3 == "sunshine").map(|p| p.0).collect();
    let appareil = appareil.to_lowercase();
    let mut out: Vec<(i32, String)> = procs
        .iter()
        .filter(|p| sunshine.contains(&p.1) && p.3 != "sunshine")
        .filter(|p| !appareil.is_empty() && p.4.as_deref().is_some_and(|c| c.to_lowercase().contains(&appareil)))
        .map(|p| if p.2 == p.0 { (-p.2, p.3.clone()) } else { (p.0, p.3.clone()) })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Processus de cet utilisateur et des autres (`/proc/<pid>/stat` : nom entre parenthèses, puis état,
/// ppid, groupe).
fn processus() -> Vec<Proc> {
    let Ok(rd) = std::fs::read_dir("/proc") else { return Vec::new() };
    rd.filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
        .filter_map(|pid| {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            let (debut, fin) = (stat.find('(')?, stat.rfind(')')?);
            let nom = stat[debut + 1..fin].to_string();
            let mut champs = stat[fin + 1..].split_whitespace();
            let _etat = champs.next()?;
            let ppid = champs.next()?.parse().ok()?;
            let pgrp = champs.next()?.parse().ok()?;
            Some((pid, ppid, pgrp, nom, None))
        })
        .collect()
}

/// `SUNSHINE_CLIENT_NAME` de l'environnement d'un processus (posé par Sunshine pour ses applis).
fn client_sunshine(pid: i32) -> Option<String> {
    let env = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    env.split(|b| *b == 0)
        .find_map(|v| v.strip_prefix(b"SUNSHINE_CLIENT_NAME=".as_slice()))
        .map(|v| String::from_utf8_lossy(v).into_owned())
}

/// Termine l'appli que Sunshine fait tourner (SIGTERM, comme « Quitter l'appli »). Rend les lignes à
/// afficher.
pub fn fermer_appli_sunshine(appareil: &str) -> Vec<String> {
    let mut procs = processus();
    let sunshine: Vec<i32> = procs.iter().filter(|p| p.3 == "sunshine").map(|p| p.0).collect();
    // Environnement lu pour les seuls enfants de Sunshine (mêmes droits : processus de l'utilisateur).
    for p in procs.iter_mut().filter(|p| sunshine.contains(&p.1)) {
        p.4 = client_sunshine(p.0);
    }
    let cibles = cibles(&procs, appareil);
    if cibles.is_empty() {
        return vec![format!("aucune appli Sunshine en cours pour {appareil} : rien à fermer")];
    }
    cibles
        .into_iter()
        .map(|(cible, nom)| {
            // SAFETY: kill(2) sur un pid ou un groupe (négatif) lu à l'instant ; aucune mémoire partagée.
            let rc = unsafe { libc::kill(cible, libc::SIGTERM) };
            let quoi = if cible < 0 { format!("groupe {}", -cible) } else { format!("processus {cible}") };
            if rc == 0 {
                format!("appli Sunshine « {nom} » ({quoi}) : arrêt demandé (SIGTERM)")
            } else {
                format!("appli Sunshine « {nom} » ({quoi}) : SIGTERM refusé ({})", std::io::Error::last_os_error())
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demande_par_le_journal_et_uid_verifie() {
        let l = "11-23 10:41:23.930 10157  9038 11280 I LudoFlux: fermer switch2-the-legend-of-zelda-ocarina-of-time";
        assert!(est_demande(l));
        assert_eq!(demande(l, &[10154, 10157]), Some("switch2-the-legend-of-zelda-ocarina-of-time".into()));
        // Même ligne écrite par une autre app : refusée.
        assert_eq!(demande(&l.replace("10157", "10200"), &[10154, 10157]), None);
        assert_eq!(demande(l, &[]), None);
        // Autre étiquette ou autre message : pas une demande.
        assert!(!est_demande("11-23 10:41:23.930 10157 9038 11280 I LudoArret: arrêt de com.limelight"));
        assert!(!est_demande("--------- beginning of main"));
    }

    #[test]
    fn uids_des_apps_ludotheque() {
        let s = "package:fr.srvdreamer.ludotheque uid:10154\npackage:fr.srvdreamer.ludotheque.debug uid:10157\n";
        assert_eq!(lire_uids(s), vec![10154, 10157]);
        assert!(lire_uids("").is_empty());
        assert!(script_uids().contains("cmd package list packages -U"));
        assert_eq!(script_logcat(), "logcat -v uid -T 1 -s LudoFlux:I");
    }

    #[test]
    fn cibles_enfants_de_sunshine_seulement() {
        let thor = || Some("AYN Thor".to_string());
        let procs: Vec<Proc> = vec![
            (1, 0, 1, "systemd".into(), None),
            (100, 1, 100, "sunshine".into(), None),
            (200, 100, 200, "ludo".into(), thor()),               // appli (chef de groupe)
            (201, 200, 200, "Ryujinx".into(), thor()),            // petit-enfant : couvert par le groupe
            (300, 100, 100, "sh".into(), thor()),                 // enfant hors groupe propre
            (400, 1, 400, "ludo".into(), thor()),                 // pas un enfant de Sunshine
            (500, 100, 500, "ryu-stream".into(), Some("Steam Deck".into())), // autre client
            (600, 100, 600, "prep".into(), None),                 // sans nom de client
        ];
        assert_eq!(cibles(&procs, "thor"), vec![(-200, "ludo".to_string()), (300, "sh".to_string())]);
        assert_eq!(cibles(&procs, "deck"), vec![(-500, "ryu-stream".to_string())]);
        assert!(cibles(&procs, "").is_empty());
        assert!(cibles(&procs[..1], "thor").is_empty());
    }
}
