//! Décision de synchronisation d'une sauvegarde (conception 4.3 et 10.1).
//!
//! Fonction pure : elle compare l'état local (révision de base et empreintes) à la tête du
//! serveur et dit quoi faire. Elle n'utilise jamais l'heure. Les empreintes sont toutes celles
//! de l'archive neutre ([`crate::snapshot::Snapshot::hash`]), identiques côté serveur.
//!
//! Invariants garantis (et testés) :
//! - une modification locale non envoyée n'est jamais écrasée par un téléchargement ;
//! - un dossier local vide ou absent n'est jamais envoyé s'il existait une sauvegarde ;
//! - un conflit n'est déclaré que si les deux côtés ont changé depuis la base commune ;
//! - un serveur revenu en arrière (restauré) n'écrase jamais une copie locale plus récente.

use ludo_proto::{ContentHash, Revision, SaveRevision};

/// Ce que le client sait de sa copie locale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalState {
    /// Révision serveur dont la copie locale descend (0 = jamais synchronisée).
    pub base_rev: Revision,
    /// Empreinte de la copie locale au moment de la dernière synchronisation (`None` si jamais).
    pub base_hash: Option<ContentHash>,
    /// Empreinte actuelle du dossier local (`None` = dossier absent ou vide).
    pub current_hash: Option<ContentHash>,
}

/// Action à mener.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncAction {
    /// Rien à faire : local et serveur sont alignés (ou tous deux vides).
    UpToDate,
    /// Le contenu local est déjà celui de la révision `rev` du serveur : seule la base locale
    /// doit être mise à jour (`base_rev = rev`, `base_hash = current_hash`), sans transfert.
    Rebase { rev: Revision },
    /// Le serveur est plus récent et le local n'a pas bougé : télécharger cette révision.
    Download { rev: Revision },
    /// Le local a changé et le serveur n'a pas bougé : envoyer avec cette base.
    Upload { base_rev: Revision },
    /// Les deux côtés ont changé depuis la base : le joueur choisit.
    Conflict { base_rev: Revision, server_rev: Revision },
    /// Le serveur est revenu en arrière (restauration) ou a perdu la sauvegarde alors que le
    /// local descend d'une révision plus récente : renvoyer la copie locale sur la tête actuelle,
    /// jamais télécharger l'ancienne.
    ServerBehind { server_rev: Revision },
    /// Le dossier local a disparu ou s'est vidé alors qu'une sauvegarde existait :
    /// ne rien envoyer, ne rien écraser, signaler (carte SD retirée, dossier supprimé…).
    LocalMissing,
}

/// Décide de l'action pour une sauvegarde.
pub fn decide(local: &LocalState, server_head: Option<&SaveRevision>) -> SyncAction {
    let server_rev = server_head.map_or(0, |h| h.rev);

    // Dossier local absent ou vide.
    if local.current_hash.is_none() {
        let had_something = local.base_hash.is_some() || local.base_rev > 0;
        return match (had_something, server_head) {
            (true, _) => SyncAction::LocalMissing,
            (false, Some(h)) => SyncAction::Download { rev: h.rev },
            (false, None) => SyncAction::UpToDate,
        };
    }

    // Contenu local identique à la tête : aucun transfert, au plus une mise à jour de la base.
    if let Some(h) = server_head {
        if local.current_hash.as_deref() == Some(h.hash.as_str()) {
            return if local.base_rev == h.rev && local.base_hash == local.current_hash {
                SyncAction::UpToDate
            } else {
                SyncAction::Rebase { rev: h.rev }
            };
        }
    }

    // Le serveur est derrière la base locale : jamais de retour en arrière silencieux.
    if server_rev < local.base_rev {
        return SyncAction::ServerBehind { server_rev };
    }

    let local_changed = local.current_hash != local.base_hash;
    let server_moved = server_rev != local.base_rev;
    match (local_changed, server_moved) {
        (false, false) => SyncAction::UpToDate,
        (true, false) => SyncAction::Upload { base_rev: local.base_rev },
        (false, true) => SyncAction::Download { rev: server_rev },
        (true, true) => SyncAction::Conflict { base_rev: local.base_rev, server_rev },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(rev: Revision, hash: &str) -> SaveRevision {
        SaveRevision {
            rev,
            parent: rev.saturating_sub(1),
            hash: hash.into(),
            size: 1,
            device: "pc".into(),
            emulator: "ryujinx".into(),
            playtime_s: 0,
            created_at: 0,
            discarded: false,
        }
    }

    fn local(base_rev: Revision, base: Option<&str>, cur: Option<&str>) -> LocalState {
        LocalState { base_rev, base_hash: base.map(Into::into), current_hash: cur.map(Into::into) }
    }

    #[test]
    fn premier_lancement_recupere_la_sauvegarde_du_serveur() {
        assert_eq!(decide(&local(0, None, None), Some(&head(3, "s"))), SyncAction::Download { rev: 3 });
    }

    #[test]
    fn rien_nulle_part() {
        assert_eq!(decide(&local(0, None, None), None), SyncAction::UpToDate);
    }

    #[test]
    fn premiere_sauvegarde_locale_est_envoyee() {
        assert_eq!(decide(&local(0, None, Some("a")), None), SyncAction::Upload { base_rev: 0 });
    }

    #[test]
    fn partie_jouee_localement_est_envoyee() {
        assert_eq!(decide(&local(2, Some("a"), Some("b")), Some(&head(2, "a"))), SyncAction::Upload { base_rev: 2 });
    }

    #[test]
    fn partie_jouee_ailleurs_est_telechargee() {
        assert_eq!(decide(&local(2, Some("a"), Some("a")), Some(&head(3, "c"))), SyncAction::Download { rev: 3 });
    }

    #[test]
    fn deux_parties_divergentes_donnent_un_conflit() {
        assert_eq!(
            decide(&local(2, Some("a"), Some("b")), Some(&head(3, "c"))),
            SyncAction::Conflict { base_rev: 2, server_rev: 3 }
        );
    }

    #[test]
    fn meme_contenu_des_deux_cotes_rebase_sans_transfert() {
        assert_eq!(decide(&local(2, Some("a"), Some("c")), Some(&head(3, "c"))), SyncAction::Rebase { rev: 3 });
        assert_eq!(decide(&local(3, Some("c"), Some("c")), Some(&head(3, "c"))), SyncAction::UpToDate);
    }

    #[test]
    fn dossier_disparu_ne_declenche_ni_envoi_ni_ecrasement() {
        assert_eq!(decide(&local(2, Some("a"), None), Some(&head(2, "a"))), SyncAction::LocalMissing);
        assert_eq!(decide(&local(2, Some("a"), None), Some(&head(5, "z"))), SyncAction::LocalMissing);
    }

    #[test]
    fn serveur_restaure_en_arriere_nest_jamais_telecharge() {
        assert_eq!(decide(&local(5, Some("a"), Some("a")), Some(&head(3, "old"))), SyncAction::ServerBehind { server_rev: 3 });
        assert_eq!(decide(&local(5, Some("a"), Some("b")), None), SyncAction::ServerBehind { server_rev: 0 });
    }
}
