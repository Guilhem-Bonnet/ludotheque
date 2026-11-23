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
//! la Ludothèque dépose le fichier [`DEMANDE`] dans son dossier `Android/data/<paquet>/files` (écrit à
//! côté puis renommé) ; `ludo adb-watch` le reçoit par `inotifyd` (événement `m`), le lit, le retire
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
    /// Demande de fin du flux déposée (renommée vers [`DEMANDE`]) dans ce dossier.
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

/// Processus vu dans `/proc` : (pid, ppid, groupe, nom).
pub type Proc = (i32, i32, i32, String);

/// Cibles : groupes de processus des enfants directs de Sunshine (une appli lancée par Sunshine est
/// chef de son groupe ; à défaut, le processus seul, signalé par un pid positif).
pub fn cibles(procs: &[Proc]) -> Vec<(i32, String)> {
    let sunshine: Vec<i32> = procs.iter().filter(|p| p.3 == "sunshine").map(|p| p.0).collect();
    let mut out: Vec<(i32, String)> = procs
        .iter()
        .filter(|p| sunshine.contains(&p.1) && p.3 != "sunshine")
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
            Some((pid, ppid, pgrp, nom))
        })
        .collect()
}

/// Termine l'appli que Sunshine fait tourner (SIGTERM, comme « Quitter l'appli »). Rend les lignes à
/// afficher.
pub fn fermer_appli_sunshine() -> Vec<String> {
    let cibles = cibles(&processus());
    if cibles.is_empty() {
        return vec!["aucune appli Sunshine en cours : rien à fermer".into()];
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
        let procs: Vec<Proc> = vec![
            (1, 0, 1, "systemd".into()),
            (100, 1, 100, "sunshine".into()),
            (200, 100, 200, "ludo".into()),      // appli (chef de groupe)
            (201, 200, 200, "Ryujinx".into()),   // petit-enfant : couvert par le groupe
            (300, 100, 100, "sh".into()),        // enfant hors groupe propre
            (400, 1, 400, "ludo".into()),        // partie locale, pas Sunshine
        ];
        assert_eq!(cibles(&procs), vec![(-200, "ludo".to_string()), (300, "sh".to_string())]);
        assert!(cibles(&procs[..1]).is_empty());
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
