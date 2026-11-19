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

use crate::catalog;
use ludo_core::snapshot::{archive_hash, validate_archive, SnapshotError};
use ludo_proto::{DeviceGame, DeviceInventory, 
    CatalogGame, FileRole, GameArt, GameFile, Lease, LeaseOutcome, Profile, RescanReport, SaveSummary, ProfilePrefs, PushOutcome, PushParams, SaveKey, SaveRevision, AVATAR_KINDS, LEASE_TTL_S,
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
    #[error("catalogue illisible : {0}")]
    Catalog(String),
    #[error("dossier des ROM non configuré (variable LUDO_ROMS)")]
    NoRoms,
    #[error("dossier des ROM indisponible : {0}")]
    RomsUnavailable(String),
    #[error("image refusée : {0}")]
    InvalidCover(String),
    #[error("introuvable")]
    NotFound,
    #[error("chemin refusé : {0}")]
    Forbidden(String),
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

/// Formats d'image acceptés : extension de stockage et type MIME servi.
const COVER_TYPES: &[(&str, &str)] = &[("png", "image/png"), ("jpg", "image/jpeg"), ("webp", "image/webp")];

/// Taille maximale d'une image de jeu (5 Mo).
pub const MAX_COVER_BYTES: usize = 5 * 1024 * 1024;

/// Format d'une image d'après ses premiers octets.
pub fn sniff_cover(d: &[u8]) -> Option<(&'static str, &'static str)> {
    if d.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("png", "image/png"))
    } else if d.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(("jpg", "image/jpeg"))
    } else if d.len() >= 12 && &d[..4] == b"RIFF" && &d[8..12] == b"WEBP" {
        Some(("webp", "image/webp"))
    } else {
        None
    }
}

/// Sorte d'image d'un jeu : la jaquette (tuile) ou le fond (grand format, affiché flouté derrière
/// l'écran). Chacune dans son dossier du volume de données (`covers/`, `backgrounds/`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Cover,
    Background,
}

impl ImageKind {
    pub const ALL: [ImageKind; 2] = [ImageKind::Cover, ImageKind::Background];
    pub fn dir(self) -> &'static str {
        match self {
            ImageKind::Cover => "covers",
            ImageKind::Background => "backgrounds",
        }
    }
    /// Dernier segment de la route (`/api/v1/games/{id}/<segment>`).
    pub fn segment(self) -> &'static str {
        match self {
            ImageKind::Cover => "cover",
            ImageKind::Background => "background",
        }
    }
}

pub struct Store {
    db: Mutex<Connection>,
    blobs: PathBuf,
    /// Volume de données : images des jeux sous `<data>/<sorte>/<id>.<ext>` (sauvegardées avec lui).
    data_dir: PathBuf,
    clock: Box<dyn Clock>,
    /// Racine des ROM (`LUDO_ROMS`) ; absente = catalogue vide.
    roms: Option<PathBuf>,
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
CREATE TABLE IF NOT EXISTS games (
    id         TEXT    NOT NULL PRIMARY KEY,
    console    TEXT    NOT NULL,
    title      TEXT    NOT NULL,
    short      TEXT    NOT NULL,
    title_id   TEXT,
    art_motif  TEXT    NOT NULL,
    art_from   TEXT    NOT NULL,
    art_to     TEXT    NOT NULL,
    art_ink    TEXT,
    catalogued INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS files (
    id     INTEGER PRIMARY KEY AUTOINCREMENT,
    game   TEXT    NOT NULL,
    role   TEXT    NOT NULL,
    path   TEXT    NOT NULL UNIQUE,
    format TEXT    NOT NULL,
    size   INTEGER NOT NULL,
    mtime  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS files_game ON files (game);
CREATE TABLE IF NOT EXISTS devices (
    device     TEXT    NOT NULL PRIMARY KEY,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS device_games (
    device    TEXT    NOT NULL,
    game      TEXT    NOT NULL,
    installed INTEGER NOT NULL,
    stream    INTEGER NOT NULL,
    save      INTEGER NOT NULL,
    PRIMARY KEY (device, game)
);
";

/// Colonnes ajoutées après la création des tables (base existante : ALTER TABLE au démarrage).
const ADDED_COLUMNS: &[(&str, &str, &str)] = &[("device_games", "stream_app", "TEXT")];

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

fn row_to_file(r: &rusqlite::Row<'_>, at: usize) -> rusqlite::Result<GameFile> {
    let role: String = r.get(at + 1)?;
    Ok(GameFile {
        id: r.get::<_, i64>(at)? as u64,
        role: FileRole::parse(&role).unwrap_or(FileRole::Base),
        path: r.get(at + 2)?,
        format: r.get(at + 3)?,
        size: r.get::<_, i64>(at + 4)? as u64,
        mtime: r.get::<_, i64>(at + 5)? as u64,
    })
}

/// Joint un chemin relatif à la racine en refusant toute évasion : chemin absolu, `..`, ou lien
/// symbolique qui sortirait de la racine (le chemin canonique doit rester sous la racine canonique).
pub fn safe_join(root: &Path, rel: &str) -> Result<PathBuf, StoreError> {
    use std::path::Component;
    let relp = Path::new(rel);
    if rel.is_empty() || !relp.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(StoreError::Forbidden(rel.into()));
    }
    let root = root.canonicalize()?;
    let full = root.join(relp).canonicalize().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => StoreError::NotFound,
        _ => StoreError::Io(e),
    })?;
    if full.starts_with(&root) {
        Ok(full)
    } else {
        Err(StoreError::Forbidden(rel.into()))
    }
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
        for kind in ImageKind::ALL {
            let dir = data_dir.join(kind.dir());
            std::fs::create_dir_all(&dir)?;
            // Restes d'un envoi d'image interrompu (disque plein, arrêt) : jamais lus, on les balaie.
            for e in std::fs::read_dir(&dir)?.flatten() {
                let name = e.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.') && name.ends_with(".tmp") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        let db = Connection::open(data_dir.join("ludotheque.db"))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "FULL")?;
        db.execute_batch(SCHEMA)?;
        for (table, column, ty) in ADDED_COLUMNS {
            let present = db.prepare(&format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name=?1"))?.exists(params![column])?;
            if !present {
                db.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {ty}"))?;
            }
        }
        Ok(Self { db: Mutex::new(db), blobs: data_dir.join("blobs"), data_dir: data_dir.to_path_buf(), clock, roms: None })
    }

    /// Indique le dossier des ROM (catalogue, téléchargements). Le balayage se lance avec [`Store::rescan`].
    pub fn with_roms(mut self, root: PathBuf) -> Self {
        self.roms = Some(root);
        self
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

    /// Remplace l'inventaire d'un appareil. Les jeux absents du catalogue sont ignorés (catalogue
    /// rebalayé entre-temps) ; un identifiant malformé est refusé.
    pub fn put_inventory(&self, device: &str, games: &[DeviceGame]) -> Result<DeviceInventory, StoreError> {
        check(device)?;
        for g in games {
            check(&g.game)?;
        }
        let now = self.clock.now();
        let mut db = self.db.lock().expect("verrou base");
        let tx = db.transaction()?;
        tx.execute("DELETE FROM device_games WHERE device=?1", params![device])?;
        let mut kept = Vec::new();
        for g in games {
            let known = tx.query_row("SELECT 1 FROM games WHERE id=?1", params![g.game], |_| Ok(())).optional()?;
            if known.is_some() && (g.installed || g.stream || g.save) {
                tx.execute(
                    "INSERT OR REPLACE INTO device_games (device, game, installed, stream, save, stream_app) VALUES (?1,?2,?3,?4,?5,?6)",
                    params![device, g.game, g.installed, g.stream, g.save, g.stream_app],
                )?;
                kept.push(g.clone());
            }
        }
        tx.execute("INSERT OR REPLACE INTO devices (device, updated_at) VALUES (?1, ?2)", params![device, now as i64])?;
        tx.commit()?;
        kept.sort_by(|a, b| a.game.cmp(&b.game));
        Ok(DeviceInventory { device: device.to_string(), updated_at: now, games: kept })
    }

    /// Inventaires de tous les appareils connus.
    pub fn inventories(&self) -> Result<Vec<DeviceInventory>, StoreError> {
        let db = self.db.lock().expect("verrou base");
        let mut st = db.prepare("SELECT device, updated_at FROM devices ORDER BY device")?;
        let devices: Vec<(String, u64)> = st.query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)))?.collect::<Result<_, _>>()?;
        let mut out = Vec::new();
        for (device, updated_at) in devices {
            let mut st = db.prepare("SELECT game, installed, stream, save, stream_app FROM device_games WHERE device=?1 ORDER BY game")?;
            let games = st
                .query_map(params![device], |r| Ok(DeviceGame { game: r.get(0)?, installed: r.get(1)?, stream: r.get(2)?, save: r.get(3)?, stream_app: r.get(4)? }))?
                .collect::<Result<_, _>>()?;
            out.push(DeviceInventory { device, updated_at, games });
        }
        Ok(out)
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

    /// Têtes de sauvegarde d'un profil (une par jeu, versions écartées exclues), plus récentes d'abord.
    pub fn save_summaries(&self, profile: &str) -> Result<Vec<SaveSummary>, StoreError> {
        check(profile)?;
        let db = self.db.lock().expect("verrou base");
        let mut st = db.prepare(
            "SELECT s.game, s.rev, s.playtime_s, s.device, s.emulator, s.created_at FROM save_revisions s
             WHERE s.profile=?1 AND s.discarded=0
               AND s.rev = (SELECT MAX(rev) FROM save_revisions t WHERE t.profile=s.profile AND t.game=s.game AND t.discarded=0)
             ORDER BY s.created_at DESC, s.game",
        )?;
        let rows = st.query_map(params![profile], |r| {
            Ok(SaveSummary {
                game: r.get(0)?,
                rev: r.get::<_, i64>(1)? as u64,
                playtime_s: r.get::<_, i64>(2)? as u64,
                device: r.get(3)?,
                emulator: r.get(4)?,
                created_at: r.get::<_, i64>(5)? as u64,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Balaie le dossier des ROM et remplace le catalogue en base. Les identifiants de fichiers
    /// restent stables tant que le chemin ne change pas.
    pub fn rescan(&self) -> Result<RescanReport, StoreError> {
        let root = self.roms.as_ref().ok_or(StoreError::NoRoms)?;
        let catalog::ScanResult { games: scanned, ignored } = catalog::scan(root)?;
        let mut db = self.db.lock().expect("verrou base");
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Un balayage vide ne remplace jamais un catalogue existant : partage des ROM non monté,
        // disque débranché… On garde l'ancien catalogue (et les identifiants de fichiers, pour la reprise).
        if scanned.is_empty() {
            let existing: i64 = tx.query_row("SELECT COUNT(*) FROM games", [], |r| r.get(0))?;
            if existing > 0 {
                return Err(StoreError::RomsUnavailable(format!(
                    "aucun jeu trouvé dans {} ; catalogue actuel ({existing} jeux) conservé",
                    root.display()
                )));
            }
        }
        let ids: Vec<&str> = scanned.iter().map(|g| g.id.as_str()).collect();
        let paths: std::collections::HashSet<&str> = scanned.iter().flat_map(|g| g.files.iter().map(|f| f.path.as_str())).collect();
        let old_games: Vec<String> = tx.prepare("SELECT id FROM games")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        for id in old_games.iter().filter(|id| !ids.contains(&id.as_str())) {
            tx.execute("DELETE FROM games WHERE id=?1", params![id])?;
        }
        let old_files: Vec<String> = tx.prepare("SELECT path FROM files")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        for p in old_files.iter().filter(|p| !paths.contains(p.as_str())) {
            tx.execute("DELETE FROM files WHERE path=?1", params![p])?;
        }
        let mut files = 0;
        for g in &scanned {
            tx.execute(
                "INSERT INTO games (id, console, title, short, title_id, art_motif, art_from, art_to, art_ink, catalogued)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
                 ON CONFLICT(id) DO UPDATE SET console=excluded.console, title=excluded.title, short=excluded.short,
                   title_id=excluded.title_id, art_motif=excluded.art_motif, art_from=excluded.art_from,
                   art_to=excluded.art_to, art_ink=excluded.art_ink, catalogued=excluded.catalogued",
                params![g.id, g.console, g.title, g.short, g.title_id, g.art.motif, g.art.from, g.art.to, g.art.ink, g.catalogued as i64],
            )?;
            for f in &g.files {
                tx.execute(
                    "INSERT INTO files (game, role, path, format, size, mtime) VALUES (?1,?2,?3,?4,?5,?6)
                     ON CONFLICT(path) DO UPDATE SET game=excluded.game, role=excluded.role, format=excluded.format,
                       size=excluded.size, mtime=excluded.mtime",
                    params![g.id, f.role.as_str(), f.path, f.format, f.size as i64, f.mtime as i64],
                )?;
                files += 1;
            }
        }
        tx.commit()?;
        Ok(RescanReport { games: scanned.len(), files, ignored })
    }

    /// Catalogue des jeux, triés par titre, avec leurs fichiers.
    pub fn games(&self) -> Result<Vec<CatalogGame>, StoreError> {
        let db = self.db.lock().expect("verrou base");
        let mut by_game: std::collections::HashMap<String, Vec<GameFile>> = std::collections::HashMap::new();
        let mut st = db.prepare("SELECT game, id, role, path, format, size, mtime FROM files ORDER BY role, path")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, row_to_file(r, 1)?)))?;
        for row in rows {
            let (game, file) = row?;
            by_game.entry(game).or_default().push(file);
        }
        let mut st = db.prepare(
            "SELECT id, console, title, short, title_id, art_motif, art_from, art_to, art_ink, catalogued
             FROM games ORDER BY title COLLATE NOCASE, id",
        )?;
        let rows = st.query_map([], |r| {
            Ok(CatalogGame {
                id: r.get(0)?,
                console: r.get(1)?,
                title: r.get(2)?,
                short: r.get(3)?,
                title_id: r.get(4)?,
                art: GameArt { motif: r.get(5)?, from: r.get(6)?, to: r.get(7)?, ink: r.get(8)? },
                cover: None,
                background: None,
                catalogued: r.get::<_, i64>(9)? != 0,
                files: Vec::new(),
            })
        })?;
        let mut games: Vec<CatalogGame> = rows.collect::<Result<_, _>>()?;
        for g in &mut games {
            g.files = by_game.remove(&g.id).unwrap_or_default();
            let url = |kind: ImageKind| self.image_info(kind, &g.id).map(|(_, _, v)| format!("/api/v1/games/{}/{}?v={v}", g.id, kind.segment()));
            g.cover = url(ImageKind::Cover);
            g.background = url(ImageKind::Background);
        }
        Ok(games)
    }

    /// Jaquette du jeu (voir `image_info`).
    pub fn cover_info(&self, game: &str) -> Option<(PathBuf, &'static str, String)> {
        self.image_info(ImageKind::Cover, game)
    }

    /// Enregistre la jaquette d'un jeu (voir `put_image`).
    pub fn put_cover(&self, game: &str, data: &[u8]) -> Result<(), StoreError> {
        self.put_image(ImageKind::Cover, game, data)
    }

    /// Image d'un jeu : (chemin, type MIME, jeton de version `taille-date` en hexadécimal).
    pub fn image_info(&self, kind: ImageKind, game: &str) -> Option<(PathBuf, &'static str, String)> {
        let dir = self.data_dir.join(kind.dir());
        // Id validé ici aussi : la route GET passe l'id décodé (`%2F..` compris) tel quel.
        if !valid_id(game) {
            return None;
        }
        for (ext, mime) in COVER_TYPES {
            let path = dir.join(format!("{game}.{ext}"));
            if let Ok(m) = std::fs::metadata(&path) {
                if m.is_file() {
                    let mtime = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis());
                    return Some((path, mime, format!("{:x}-{mtime:x}", m.len())));
                }
            }
        }
        None
    }

    /// Enregistre l'image d'un jeu (remplace la précédente, quel que soit son format). Le jeu doit
    /// exister au catalogue ; le contenu est validé par ses octets magiques, jamais par l'en-tête du client.
    pub fn put_image(&self, kind: ImageKind, game: &str, data: &[u8]) -> Result<(), StoreError> {
        check(game)?;
        let dir = self.data_dir.join(kind.dir());
        let Some((ext, _)) = sniff_cover(data) else {
            return Err(StoreError::InvalidCover("format d'image non reconnu (png, jpeg ou webp attendu)".into()));
        };
        if data.len() > MAX_COVER_BYTES {
            return Err(StoreError::InvalidCover(format!("image trop lourde (plus de {} Mo)", MAX_COVER_BYTES / 1_000_000)));
        }
        let known = self.db.lock().expect("verrou base").query_row("SELECT 1 FROM games WHERE id=?1", params![game], |_| Ok(())).optional()?;
        if known.is_none() {
            return Err(StoreError::NotFound);
        }
        // Nom temporaire unique : deux envois simultanés du même jeu n'écrivent jamais le même fichier.
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = dir.join(format!(".{game}.{}.{n}.tmp", std::process::id()));
        let written = std::fs::write(&tmp, data).and_then(|()| std::fs::rename(&tmp, dir.join(format!("{game}.{ext}"))));
        if let Err(e) = written {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        // Les autres formats ne partent qu'une fois la nouvelle image en place.
        for (other, _) in COVER_TYPES {
            if *other != ext {
                let _ = std::fs::remove_file(dir.join(format!("{game}.{other}")));
            }
        }
        sync_dir(&dir)?;
        Ok(())
    }

    /// Emplacement sur le disque d'un fichier du catalogue, vérifié : le chemin résolu doit rester
    /// sous la racine des ROM (liens symboliques et `..` compris). Renvoie aussi la fiche du fichier.
    pub fn file_location(&self, game: &str, file_id: u64) -> Result<(PathBuf, GameFile), StoreError> {
        check(game)?;
        let root = self.roms.as_ref().ok_or(StoreError::NoRoms)?;
        let file = {
            let db = self.db.lock().expect("verrou base");
            db.query_row(
                "SELECT id, role, path, format, size, mtime FROM files WHERE game=?1 AND id=?2",
                params![game, file_id as i64],
                |r| row_to_file(r, 0),
            )
            .optional()?
        }
        .ok_or(StoreError::NotFound)?;
        Ok((safe_join(root, &file.path)?, file))
    }
}
