//! Moteur de synchronisation : applique la décision de `ludo_core::sync::decide` avec le serveur
//! et l'adaptateur de rangement. Seul composant qui écrit dans les dossiers de sauvegarde.

use crate::api::{ApiError, Client};
use crate::journal::{Entry, Journal};
use crate::layout::{LayoutError, SaveLayout};
use ludo_core::sync::{decide, LocalState, SyncAction};
use ludo_proto::{PushOutcome, PushParams, SaveKey, SaveRevision};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Layout(#[from] LayoutError),
    #[error(transparent)]
    Api(ApiError),
    #[error("journal : {0}")]
    Journal(#[from] std::io::Error),
    /// La révision demandée n'existe pas sur le serveur.
    #[error("version introuvable : révision {0}")]
    UnknownRevision(u64),
    /// La tête a bougé pendant la restauration : rien n'a été remplacé.
    #[error("sauvegarde modifiée entre-temps sur {} (révision {}) : restauration annulée", .0.device, .0.rev)]
    RestoreConflict(SaveRevision),
}

/// Résultat d'une synchronisation, tel que l'interface l'affiche.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    /// « À jour »
    UpToDate,
    /// Sauvegarde du serveur installée localement.
    Downloaded { rev: u64 },
    /// Sauvegarde locale envoyée.
    Uploaded { rev: u64 },
    /// « Hors ligne » : rien n'a été transféré ; un envoi reste en attente si le local a changé.
    Offline { pending_upload: bool },
    /// « Conflit de sauvegarde » : le joueur doit choisir.
    Conflict { server: SaveRevision },
    /// Dossier local disparu alors qu'une sauvegarde existait : rien n'est touché.
    LocalMissing,
}

/// Choix du joueur face à un conflit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// Garder la sauvegarde de cet appareil (elle devient la tête).
    KeepLocal,
    /// Utiliser la sauvegarde du serveur (la locale est archivée comme écartée).
    KeepServer,
}

pub struct Engine<'a> {
    pub client: &'a Client,
    pub journal: &'a mut Journal,
    pub device: String,
    pub emulator: String,
    pub playtime_s: u64,
}

impl Engine<'_> {
    fn params(&self, base_rev: u64, discarded: bool) -> PushParams {
        PushParams {
            base_rev,
            device: self.device.clone(),
            emulator: self.emulator.clone(),
            playtime_s: self.playtime_s,
            discarded,
        }
    }

    fn offline(&mut self, key: &SaveKey, entry: Entry, local_hash: Option<&String>) -> Result<SyncOutcome, EngineError> {
        let pending = local_hash.is_some() && local_hash != entry.base_hash.as_ref();
        self.journal.set(key, Entry { pending_upload: pending, ..entry })?;
        Ok(SyncOutcome::Offline { pending_upload: pending })
    }

    /// Télécharge et installe une révision ; met à jour la base.
    fn download(&mut self, key: &SaveKey, layout: &dyn SaveLayout, rev: u64) -> Result<SyncOutcome, EngineError> {
        let entry = self.journal.get(key);
        let data = match self.client.blob(key, rev) {
            Ok(d) => d,
            Err(ApiError::Offline(_)) => return self.offline(key, entry, None),
            Err(e) => return Err(EngineError::Api(e)),
        };
        layout.import(&data)?;
        let hash = layout.export()?.map(|s| s.hash);
        self.journal.set(key, Entry { base_rev: rev, base_hash: hash, pending_upload: false, conflict: false })?;
        Ok(SyncOutcome::Downloaded { rev })
    }

    /// Synchronise une sauvegarde (avant ou après une partie).
    pub fn sync(&mut self, key: &SaveKey, layout: &dyn SaveLayout) -> Result<SyncOutcome, EngineError> {
        let entry = self.journal.get(key);
        let local = layout.export()?;
        let local_hash = local.as_ref().map(|s| s.hash.clone());
        let head = match self.client.head(key) {
            Ok(h) => h,
            Err(ApiError::Offline(_)) => return self.offline(key, entry, local_hash.as_ref()),
            Err(e) => return Err(EngineError::Api(e)),
        };
        let state = LocalState { base_rev: entry.base_rev, base_hash: entry.base_hash.clone(), current_hash: local_hash.clone() };

        match decide(&state, head.as_ref()) {
            SyncAction::UpToDate => {
                self.journal.set(key, Entry { pending_upload: false, conflict: false, ..entry })?;
                Ok(SyncOutcome::UpToDate)
            }
            SyncAction::Rebase { rev } => {
                self.journal.set(key, Entry { base_rev: rev, base_hash: local_hash, pending_upload: false, conflict: false })?;
                Ok(SyncOutcome::UpToDate)
            }
            SyncAction::Download { rev } => self.download(key, layout, rev),
            SyncAction::Upload { base_rev } | SyncAction::ServerBehind { server_rev: base_rev } => {
                let snap = local.expect("decide n'envoie jamais un dossier vide");
                match self.client.push(key, &self.params(base_rev, false), &snap.archive) {
                    Ok(PushOutcome::Accepted { revision }) | Ok(PushOutcome::Unchanged { revision }) => {
                        self.journal.set(key, Entry { base_rev: revision.rev, base_hash: Some(snap.hash), pending_upload: false, conflict: false })?;
                        Ok(SyncOutcome::Uploaded { rev: revision.rev })
                    }
                    Ok(PushOutcome::Conflict { head }) => {
                        self.journal.set(key, Entry { conflict: true, ..entry })?;
                        Ok(SyncOutcome::Conflict { server: head })
                    }
                    Err(ApiError::Offline(_)) => self.offline(key, entry, local_hash.as_ref()),
                    Err(e) => Err(EngineError::Api(e)),
                }
            }
            SyncAction::Conflict { .. } => {
                self.journal.set(key, Entry { conflict: true, ..entry })?;
                Ok(SyncOutcome::Conflict { server: head.expect("conflit = tête présente") })
            }
            SyncAction::LocalMissing => Ok(SyncOutcome::LocalMissing),
        }
    }

    /// Applique le choix du joueur après un conflit.
    pub fn resolve(&mut self, key: &SaveKey, layout: &dyn SaveLayout, choice: Resolution) -> Result<SyncOutcome, EngineError> {
        let entry = self.journal.get(key);
        let head = self.client.head(key).map_err(EngineError::Api)?;
        let local = layout.export()?;
        match (choice, local, head) {
            (Resolution::KeepLocal, Some(snap), head) => {
                let base = head.map_or(0, |h| h.rev);
                match self.client.push(key, &self.params(base, false), &snap.archive).map_err(EngineError::Api)? {
                    PushOutcome::Accepted { revision } | PushOutcome::Unchanged { revision } => {
                        self.journal.set(key, Entry { base_rev: revision.rev, base_hash: Some(snap.hash), pending_upload: false, conflict: false })?;
                        Ok(SyncOutcome::Uploaded { rev: revision.rev })
                    }
                    // La tête a encore bougé pendant le choix : nouveau conflit.
                    PushOutcome::Conflict { head } => Ok(SyncOutcome::Conflict { server: head }),
                }
            }
            (Resolution::KeepServer, local, Some(head)) => {
                // La version écartée part d'abord dans l'historique du serveur.
                if let Some(snap) = local {
                    self.client.push(key, &self.params(entry.base_rev, true), &snap.archive).map_err(EngineError::Api)?;
                }
                self.download(key, layout, head.rev)
            }
            // Rien à garder d'un côté : une synchronisation normale suffit.
            _ => self.sync(key, layout),
        }
    }

    /// Restaure une version de l'historique : son contenu devient une **nouvelle** tête
    /// (compare-and-swap contre la tête actuelle), puis remplace la sauvegarde locale si un
    /// adaptateur est fourni.
    ///
    /// Une copie locale qui n'est pas sur le serveur (modifiée depuis la base) est d'abord
    /// archivée comme version écartée : rien n'est perdu. Sans adaptateur (jeu non configuré sur
    /// cet appareil), seul le serveur change ; la prochaine synchronisation récupérera la version.
    pub fn restore(&mut self, key: &SaveKey, layout: Option<&dyn SaveLayout>, rev: u64) -> Result<SaveRevision, EngineError> {
        let history = self.client.history(key).map_err(EngineError::Api)?;
        let target = history.iter().find(|r| r.rev == rev).cloned().ok_or(EngineError::UnknownRevision(rev))?;
        let head_rev = history.iter().find(|r| !r.discarded).map_or(0, |h| h.rev);
        let data = self.client.blob(key, rev).map_err(EngineError::Api)?;

        let entry = self.journal.get(key);
        let local = match layout {
            Some(l) => l.export()?,
            None => None,
        };
        if let Some(snap) = &local {
            if Some(&snap.hash) != entry.base_hash.as_ref() && snap.hash != target.hash {
                self.client.push(key, &self.params(entry.base_rev, true), &snap.archive).map_err(EngineError::Api)?;
            }
        }

        // La version restaurée garde son émulateur et son temps de jeu ; l'appareil est celui-ci.
        let params = PushParams {
            base_rev: head_rev,
            device: self.device.clone(),
            emulator: target.emulator.clone(),
            playtime_s: target.playtime_s,
            discarded: false,
        };
        let revision = match self.client.push(key, &params, &data).map_err(EngineError::Api)? {
            PushOutcome::Accepted { revision } | PushOutcome::Unchanged { revision } => revision,
            PushOutcome::Conflict { head } => return Err(EngineError::RestoreConflict(head)),
        };

        if let Some(l) = layout {
            l.import(&data)?;
            let hash = l.export()?.map(|s| s.hash);
            self.journal.set(key, Entry { base_rev: revision.rev, base_hash: hash, pending_upload: false, conflict: false })?;
        }
        Ok(revision)
    }
}
