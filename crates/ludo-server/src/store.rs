//! Stockage du serveur : SQLite pour les métadonnées, blobs adressés par empreinte sur disque.
//!
//! Garanties :
//! - une archive n'est acceptée que si elle est saine (fichiers et dossiers seulement, chemins
//!   relatifs) et contient au moins un fichier ;
//! - le blob est écrit dans un temporaire unique, synchronisé sur disque, renommé, puis le
//!   dossier est synchronisé, **avant** la ligne SQLite : une révision ne pointe jamais vers un
//!   blob absent ou tronqué ; un blob existant est revérifié avant d'être réutilisé ;
//! - l'envoi est un compare-and-swap sur la tête (versions écartées exclues) ;
//! - une version écartée lors d'un conflit est archivée dans l'historique sans devenir tête ;
//! - un profil n'est enregistré que s'il est valide (nom, personnage, couleurs).

use ludo_core::snapshot::{archive_hash, validate_archive, SnapshotError};
use ludo_proto::{
    Lease, LeaseOutcome, Profile, ProfilePrefs, PushOutcome, PushParams, SaveKey, SaveRevision, AVATAR_KINDS, LEASE_TTL_S,
    PROFILE_NAME_MAX,
};
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
    #[error("profil refusé : {0}")]
    InvalidProfile(String),
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
CREATE TABLE IF NOT EXISTS profiles (
    id          TEXT    NOT NULL PRIMARY KEY,
    name        TEXT    NOT NULL,
    avatar_kind TEXT,
    accent      TEXT    NOT NULL,
    accent_soft TEXT    NOT NULL,
    accent_ink  TEXT    NOT NULL,
    sons        INTEGER NOT NULL DEFAULT 1,
    vibrations  INTEGER NOT NULL DEFAULT 1,
    updated_at  INTEGER NOT NULL
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

/// Couleur au format `#rrggbb`.
fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Valide un profil et le normalise (nom sans espaces autour). `id` vient de la route.
/// Caractères invisibles ou de contrôle du sens d'écriture (deux noms identiques à l'écran,
/// texte retourné) : espaces de largeur nulle, marques et isolats bidi, trait d'union conditionnel.
fn is_invisible(c: char) -> bool {
    matches!(c, '\u{00AD}' | '\u{034F}' | '\u{061C}' | '\u{115F}' | '\u{1160}' | '\u{17B4}' | '\u{17B5}' | '\u{180E}'
        | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206F}'
        | '\u{3164}' | '\u{FEFF}' | '\u{FFA0}')
}

pub fn normalize_profile(id: &str, p: &Profile) -> Result<Profile, StoreError> {
    check(id)?;
    if p.id != id {
        return Err(StoreError::InvalidProfile(format!("identifiant {} différent de la route {id}", p.id)));
    }
    let name = p.name.trim();
    let len = name.chars().count();
    if len == 0 || len > PROFILE_NAME_MAX {
        return Err(StoreError::InvalidProfile(format!("nom de 1 à {PROFILE_NAME_MAX} caractères attendu")));
    }
    if name.chars().any(char::is_control) {
        return Err(StoreError::InvalidProfile("caractère de contrôle dans le nom".into()));
    }
    if name.chars().any(is_invisible) {
        return Err(StoreError::InvalidProfile("caractère invisible dans le nom".into()));
    }
    if let Some(kind) = &p.avatar_kind {
        if !AVATAR_KINDS.contains(&kind.as_str()) {
            return Err(StoreError::InvalidProfile(format!("personnage inconnu : {kind}")));
        }
    }
    for (field, value) in [("accent", &p.accent), ("accentSoft", &p.accent_soft), ("accentInk", &p.accent_ink)] {
        if !is_hex_color(value) {
            return Err(StoreError::InvalidProfile(format!("{field} : couleur #rrggbb attendue")));
        }
    }
    Ok(Profile { name: name.to_string(), ..p.clone() })
}

fn row_to_profile(r: &rusqlite::Row<'_>) -> rusqlite::Result<Profile> {
    Ok(Profile {
        id: r.get(0)?,
        name: r.get(1)?,
        avatar_kind: r.get(2)?,
        accent: r.get(3)?,
        accent_soft: r.get(4)?,
        accent_ink: r.get(5)?,
        prefs: ProfilePrefs { sons: r.get::<_, i64>(6)? != 0, vibrations: r.get::<_, i64>(7)? != 0 },
    })
}

const PROFILE_COLS: &str = "id, name, avatar_kind, accent, accent_soft, accent_ink, sons, vibrations";

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

    /// Profils du foyer, dans l'ordre de leur création.
    pub fn profiles(&self) -> Result<Vec<Profile>, StoreError> {
        let db = self.db.lock().expect("verrou base");
        let mut st = db.prepare(&format!("SELECT {PROFILE_COLS} FROM profiles ORDER BY rowid"))?;
        let rows = st.query_map([], row_to_profile)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Crée ou met à jour un profil ; renvoie le profil tel qu'enregistré.
    pub fn put_profile(&self, id: &str, profile: &Profile) -> Result<Profile, StoreError> {
        let p = normalize_profile(id, profile)?;
        let db = self.db.lock().expect("verrou base");
        // Deux profils ne portent pas le même nom (sans tenir compte de la casse, accents compris).
        {
            let wanted = p.name.to_lowercase();
            let mut st = db.prepare("SELECT name FROM profiles WHERE id <> ?1")?;
            let names = st.query_map(params![p.id], |r| r.get::<_, String>(0))?;
            for n in names {
                if n?.to_lowercase() == wanted {
                    return Err(StoreError::InvalidProfile("nom déjà utilisé par un autre profil".into()));
                }
            }
        }
        db.execute(
            &format!(
                "INSERT INTO profiles ({PROFILE_COLS}, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
                 ON CONFLICT(id) DO UPDATE SET name=excluded.name, avatar_kind=excluded.avatar_kind,
                   accent=excluded.accent, accent_soft=excluded.accent_soft, accent_ink=excluded.accent_ink,
                   sons=excluded.sons, vibrations=excluded.vibrations, updated_at=excluded.updated_at"
            ),
            params![
                p.id,
                p.name,
                p.avatar_kind,
                p.accent,
                p.accent_soft,
                p.accent_ink,
                p.prefs.sons as i64,
                p.prefs.vibrations as i64,
                self.clock.now() as i64
            ],
        )?;
        Ok(p)
    }
}
