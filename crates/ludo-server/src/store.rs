//! Stockage du serveur : SQLite pour les métadonnées, blobs adressés par empreinte sur disque.
//!
//! Garanties :
//! - une archive n'est acceptée que si elle est saine (fichiers et dossiers seulement, chemins
//!   relatifs) et contient au moins un fichier ;
//! - le blob est écrit dans un temporaire unique, synchronisé sur disque, renommé, puis le
//!   dossier est synchronisé, **avant** la ligne SQLite : une révision ne pointe jamais vers un
//!   blob absent ou tronqué ; un blob existant est revérifié avant d'être réutilisé ;
//! - l'envoi est un compare-and-swap sur la tête (versions écartées exclues) ;
//! - une version écartée lors d'un conflit est archivée dans l'historique sans devenir tête.

use ludo_core::snapshot::{archive_hash, validate_archive, SnapshotError};
use ludo_proto::{Lease, LeaseOutcome, PushOutcome, PushParams, SaveKey, SaveRevision, LEASE_TTL_S};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("base de données : {0}")]
    Db(#[from] rusqlite::Error),
    #[error("entrée-sortie : {0}")]
    Io(#[from] std::io::Error),
    #[error("identifiant invalide : {0}")]
    InvalidId(String),
    #[error("archive refusée : {0}")]
    InvalidArchive(#[from] SnapshotError),
    #[error("archive refusée : aucun fichier")]
    EmptyArchive,
    #[error("blob corrompu sur le disque : {0}")]
    CorruptBlob(String),
}

/// Horloge injectable (les tests avancent le temps sans dormir).
pub trait Clock: Send + Sync {
    fn now(&self) -> u64;
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }
}

pub struct Store {
    db: Mutex<Connection>,
    blobs: PathBuf,
    clock: Box<dyn Clock>,
}

/// Les identifiants servent dans les routes : jeu de caractères restreint.
pub fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        && !s.starts_with('.')
        && s != "history"
}

fn check(id: &str) -> Result<(), StoreError> {
    if valid_id(id) {
        Ok(())
    } else {
        Err(StoreError::InvalidId(id.into()))
    }
}

fn check_key(key: &SaveKey) -> Result<(), StoreError> {
    check(&key.profile)?;
    check(&key.game)
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS save_revisions (
    profile    TEXT    NOT NULL,
    game       TEXT    NOT NULL,
    rev        INTEGER NOT NULL,
    parent     INTEGER NOT NULL,
    hash       TEXT    NOT NULL,
    size       INTEGER NOT NULL,
    device     TEXT    NOT NULL,
    emulator   TEXT    NOT NULL,
    playtime_s INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    discarded  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (profile, game, rev)
);
CREATE TABLE IF NOT EXISTS leases (
    profile    TEXT    NOT NULL,
    game       TEXT    NOT NULL,
    device     TEXT    NOT NULL,
    expires_at INTEGER NOT NULL,
    PRIMARY KEY (profile, game)
);
";

const COLS: &str = "rev, parent, hash, size, device, emulator, playtime_s, created_at, discarded";

fn row_to_rev(r: &rusqlite::Row<'_>) -> rusqlite::Result<SaveRevision> {
    Ok(SaveRevision {
        rev: r.get::<_, i64>(0)? as u64,
        parent: r.get::<_, i64>(1)? as u64,
        hash: r.get(2)?,
        size: r.get::<_, i64>(3)? as u64,
        device: r.get(4)?,
        emulator: r.get(5)?,
        playtime_s: r.get::<_, i64>(6)? as u64,
        created_at: r.get::<_, i64>(7)? as u64,
        discarded: r.get::<_, i64>(8)? != 0,
    })
}

fn head_tx(tx: &Transaction<'_>, key: &SaveKey) -> rusqlite::Result<Option<SaveRevision>> {
    tx.query_row(
        &format!(
            "SELECT {COLS} FROM save_revisions WHERE profile=?1 AND game=?2 AND discarded=0 ORDER BY rev DESC LIMIT 1"
        ),
        params![key.profile, key.game],
        row_to_rev,
    )
    .optional()
}

#[cfg(unix)]
fn sync_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}
#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}

impl Store {
    pub fn open(data_dir: &Path, clock: Box<dyn Clock>) -> Result<Self, StoreError> {
        std::fs::create_dir_all(data_dir.join("blobs"))?;
        let db = Connection::open(data_dir.join("ludotheque.db"))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "FULL")?;
        db.execute_batch(SCHEMA)?;
        Ok(Self { db: Mutex::new(db), blobs: data_dir.join("blobs"), clock })
    }

    fn blob_path(&self, hash: &str) -> PathBuf {
        self.blobs.join(&hash[..2]).join(hash)
    }

    /// Écrit le blob de façon durable, ou vérifie celui déjà présent.
    fn put_blob(&self, hash: &str, data: &[u8]) -> Result<(), StoreError> {
        let path = self.blob_path(hash);
        let dir = path.parent().expect("parent").to_path_buf();
        if path.exists() {
            let existing = std::fs::read(&path)?;
            if archive_hash(&existing) == hash {
                return Ok(());
            }
            // Blob tronqué (coupure de courant…) : on le remplace par le bon contenu.
            tracing::warn!(%hash, "blob corrompu remplacé");
        }
        std::fs::create_dir_all(&dir)?;
        let mut tmp = tempfile::NamedTempFile::new_in(&dir)?;
        tmp.write_all(data)?;
        tmp.as_file().sync_all()?;
        tmp.persist(&path).map_err(|e| e.error)?;
        sync_dir(&dir)?;
        Ok(())
    }

    pub fn head(&self, key: &SaveKey) -> Result<Option<SaveRevision>, StoreError> {
        check_key(key)?;
        let mut db = self.db.lock().expect("verrou base");
        let tx = db.transaction()?;
        Ok(head_tx(&tx, key)?)
    }

    /// Historique complet (versions écartées comprises), plus récent d'abord.
    pub fn history(&self, key: &SaveKey) -> Result<Vec<SaveRevision>, StoreError> {
        check_key(key)?;
        let db = self.db.lock().expect("verrou base");
        let mut st = db.prepare(&format!(
            "SELECT {COLS} FROM save_revisions WHERE profile=?1 AND game=?2 ORDER BY rev DESC"
        ))?;
        let rows = st.query_map(params![key.profile, key.game], row_to_rev)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Contenu d'une révision (`None` si elle n'existe pas). Le contenu est revérifié.
    pub fn blob(&self, key: &SaveKey, rev: u64) -> Result<Option<Vec<u8>>, StoreError> {
        let Some(r) = self.history(key)?.into_iter().find(|r| r.rev == rev) else {
            return Ok(None);
        };
        let data = std::fs::read(self.blob_path(&r.hash))?;
        if archive_hash(&data) != r.hash {
            return Err(StoreError::CorruptBlob(r.hash));
        }
        Ok(Some(data))
    }

    /// Envoi d'une sauvegarde : compare-and-swap sur la tête, ou archivage d'une version écartée.
    pub fn push(&self, key: &SaveKey, p: &PushParams, data: &[u8]) -> Result<PushOutcome, StoreError> {
        check_key(key)?;
        check(&p.device)?;
        if validate_archive(data)? == 0 {
            return Err(StoreError::EmptyArchive);
        }
        let hash = archive_hash(data);
        self.put_blob(&hash, data)?;

        let mut db = self.db.lock().expect("verrou base");
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let head = head_tx(&tx, key)?;
        let head_rev = head.as_ref().map_or(0, |h| h.rev);
        let max_rev: i64 = tx.query_row(
            "SELECT COALESCE(MAX(rev), 0) FROM save_revisions WHERE profile=?1 AND game=?2",
            params![key.profile, key.game],
            |r| r.get(0),
        )?;

        if !p.discarded {
            if let Some(h) = &head {
                if h.hash == hash {
                    return Ok(PushOutcome::Unchanged { revision: h.clone() });
                }
            }
            if head_rev != p.base_rev {
                return Ok(PushOutcome::Conflict { head: head.expect("tête présente si différente de la base") });
            }
        }

        let rev = SaveRevision {
            rev: max_rev as u64 + 1,
            parent: p.base_rev,
            hash,
            size: data.len() as u64,
            device: p.device.clone(),
            emulator: p.emulator.clone(),
            playtime_s: p.playtime_s,
            created_at: self.clock.now(),
            discarded: p.discarded,
        };
        tx.execute(
            &format!("INSERT INTO save_revisions (profile, game, {COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"),
            params![
                key.profile,
                key.game,
                rev.rev as i64,
                rev.parent as i64,
                rev.hash,
                rev.size as i64,
                rev.device,
                rev.emulator,
                rev.playtime_s as i64,
                rev.created_at as i64,
                rev.discarded as i64
            ],
        )?;
        tx.commit()?;
        Ok(PushOutcome::Accepted { revision: rev })
    }

    /// Prise ou renouvellement du verrou souple « partie en cours ».
    pub fn acquire_lease(&self, key: &SaveKey, device: &str, force: bool) -> Result<LeaseOutcome, StoreError> {
        check_key(key)?;
        check(device)?;
        let now = self.clock.now();
        let mut db = self.db.lock().expect("verrou base");
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: Option<(String, u64)> = tx
            .query_row(
                "SELECT device, expires_at FROM leases WHERE profile=?1 AND game=?2",
                params![key.profile, key.game],
                |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)),
            )
            .optional()?;
        if let Some((holder, exp)) = current {
            if holder != device && exp > now && !force {
                return Ok(LeaseOutcome::HeldElsewhere {
                    lease: Lease { key: key.clone(), device: holder, expires_at: exp },
                });
            }
        }
        let expires_at = now + LEASE_TTL_S;
        tx.execute(
            "INSERT INTO leases (profile, game, device, expires_at) VALUES (?1,?2,?3,?4)
             ON CONFLICT(profile, game) DO UPDATE SET device=excluded.device, expires_at=excluded.expires_at",
            params![key.profile, key.game, device, expires_at as i64],
        )?;
        tx.commit()?;
        Ok(LeaseOutcome::Granted { lease: Lease { key: key.clone(), device: device.into(), expires_at } })
    }

    /// Libère le verrou s'il appartient à cet appareil.
    pub fn release_lease(&self, key: &SaveKey, device: &str) -> Result<(), StoreError> {
        check_key(key)?;
        let db = self.db.lock().expect("verrou base");
        db.execute(
            "DELETE FROM leases WHERE profile=?1 AND game=?2 AND device=?3",
            params![key.profile, key.game, device],
        )?;
        Ok(())
    }
}
