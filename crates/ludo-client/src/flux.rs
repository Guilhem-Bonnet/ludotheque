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
//! la Ludothèque dépose le fichier [`DEMANDE`] dans son dossier `Android/data/<paquet>/files` ;
//! `ludo adb-watch` le reçoit par `inotifyd` (événement `w`, fichier fermé après écriture ; un
//! renommage n'y produit pas `m`, mesuré sur la Thor le 23/11), le lit, le retire
//! (dossier de la Ludothèque) et n'agit que sur une demande récente ([`FRAICHEUR_S`]).

use crate::adb::LUDO_PACKAGES;

/// Nom du fichier de demande déposé par la Ludothèque Android (`MenuJeuPlugin.demanderFinDuFlux`).
pub const DEMANDE: &str = "fermer-flux";
/// Âge maximal d'une demande (horloge de l'appareil) : une demande restée là pendant une coupure
/// n'arrête jamais une partie lancée plus tard.
pub const FRAICHEUR_S: i64 = 120;

/// Dossiers de demande des apps Ludothèque (release, essai) sur l'appareil.
pub fn dossiers() -> Vec<String> {
    LUDO_PACKAGES.iter().map(|p| format!("/sdcard/Android/data/{p}/files")).collect()
}

/// Classement d'une ligne d'`inotifyd -` (« <événements>\t<dossier>[\t<nom>] »).
#[derive(Debug, PartialEq, Eq)]
pub enum Ligne {
    /// Demande de fin du flux déposée (écrite, ou renommée vers [`DEMANDE`]) dans ce dossier.
    Demande(String),
    /// Autre fichier d'un dossier de demande (config.toml…) : ignoré, ce n'est pas une sauvegarde.
    Ignoree,
    /// Ligne d'un autre dossier surveillé (sauvegardes).
    Autre,
}

pub fn classer(ligne: &str) -> Ligne {
    let mut parts = ligne.trim_end_matches(['\r', '\n']).split('\t');
    let (Some(ev), Some(dir)) = (parts.next(), parts.next()) else { return Ligne::Autre };
    if !dossiers().iter().any(|d| d == dir) {
        return Ligne::Autre;
    }
    match parts.next() {
        Some(DEMANDE) if ev.contains('m') || ev.contains('w') => Ligne::Demande(dir.to_string()),
        _ => Ligne::Ignoree,
    }
}

/// Script de l'appareil : rend « <heure de l'appareil> <contenu de la demande> » et retire la demande
/// (elle est dans le dossier de la Ludothèque). Rien si elle a déjà été traitée.
pub fn script_lire(dossier: &str) -> String {
    let f = format!("{dossier}/{DEMANDE}");
    format!("[ -f '{f}' ] || exit 0; echo \"$(date +%s) $(head -c 200 '{f}')\"; rm -f '{f}'")
}

/// Jeu demandé si la demande est récente : sortie de [`script_lire`] = « maintenant horodatage jeu ».
pub fn demande_fraiche(sortie: &str) -> Option<String> {
    let mut it = sortie.split_whitespace();
    let maintenant: i64 = it.next()?.parse().ok()?;
    let quand: i64 = it.next()?.parse().ok()?;
    let jeu = it.next().unwrap_or("").to_string();
    (-30..=FRAICHEUR_S).contains(&(maintenant - quand)).then_some(jeu)
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
    fn classement_des_lignes_inotifyd() {
        let d = "/sdcard/Android/data/fr.srvdreamer.ludotheque/files";
        assert_eq!(classer(&format!("m\t{d}\tfermer-flux")), Ligne::Demande(d.into()));
        assert_eq!(classer("m\t/sdcard/Android/data/fr.srvdreamer.ludotheque.debug/files\tfermer-flux\r"), Ligne::Demande("/sdcard/Android/data/fr.srvdreamer.ludotheque.debug/files".into()));
        // Fichier temporaire, config, création : ignorés (pas une sauvegarde, pas une demande).
        assert_eq!(classer(&format!("w\t{d}\tfermer-flux.tmp")), Ligne::Ignoree);
        assert_eq!(classer(&format!("w\t{d}\tconfig.toml")), Ligne::Ignoree);
        assert_eq!(classer(&format!("n\t{d}\tfermer-flux")), Ligne::Ignoree);
        // Dossier de sauvegarde : laissé au classement des sauvegardes.
        assert_eq!(classer("w\t/sdcard/Android/data/org.azahar_emu.azahar/files/sdmc\tx"), Ligne::Autre);
    }

    #[test]
    fn seule_une_demande_recente_compte() {
        assert_eq!(demande_fraiche("1795440000 1795439990 mk8-flux"), Some("mk8-flux".into()));
        assert_eq!(demande_fraiche("1795440000 1795430000 mk8-flux"), None, "demande périmée");
        assert_eq!(demande_fraiche("1795440000 1795440500 mk8-flux"), None, "horloge incohérente");
        assert_eq!(demande_fraiche(""), None);
        assert_eq!(demande_fraiche("1795440000 abc"), None);
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

    #[test]
    fn script_de_lecture() {
        let s = script_lire("/sdcard/Android/data/fr.srvdreamer.ludotheque/files");
        assert!(s.contains("rm -f '/sdcard/Android/data/fr.srvdreamer.ludotheque/files/fermer-flux'"));
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().display().to_string();
        std::fs::write(dir.path().join(DEMANDE), "1795440000 mk8\n").unwrap();
        let out = std::process::Command::new("sh").arg("-c").arg(script_lire(&d)).output().unwrap();
        let t = String::from_utf8_lossy(&out.stdout);
        assert!(t.trim().ends_with("1795440000 mk8"), "{t}");
        assert!(!dir.path().join(DEMANDE).exists(), "demande retirée");
        // Deuxième lecture : rien.
        let out = std::process::Command::new("sh").arg("-c").arg(script_lire(&d)).output().unwrap();
        assert!(out.stdout.is_empty());
    }
}
