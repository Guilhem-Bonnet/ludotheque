//! Journal local : pour chaque (profil, jeu), la révision serveur dont la copie locale descend
//! et l'empreinte de la copie locale à ce moment. Écrit d'un bloc (temporaire puis renommage).

use ludo_proto::SaveKey;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub base_rev: u64,
    pub base_hash: Option<String>,
    /// Envoi en attente (dernier échec : serveur injoignable) — affichage « Envoi en attente ».
    #[serde(default)]
    pub pending_upload: bool,
    /// Conflit en attente d'un choix du joueur.
    #[serde(default)]
    pub conflict: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Data {
    entries: BTreeMap<String, Entry>,
}

pub struct Journal {
    path: PathBuf,
    data: Data,
}

fn id(key: &SaveKey) -> String {
    format!("{}/{}", key.profile, key.game)
}

impl Journal {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let data = match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(std::io::Error::other)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Data::default(),
            Err(e) => return Err(e),
        };
        Ok(Self { path: path.to_path_buf(), data })
    }

    pub fn get(&self, key: &SaveKey) -> Entry {
        self.data.entries.get(&id(key)).cloned().unwrap_or_default()
    }

    pub fn entries(&self) -> impl Iterator<Item = (&String, &Entry)> {
        self.data.entries.iter()
    }

    pub fn set(&mut self, key: &SaveKey, entry: Entry) -> std::io::Result<()> {
        self.data.entries.insert(id(key), entry);
        self.save()
    }

    fn save(&self) -> std::io::Result<()> {
        let dir = self.path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)?;
        let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
        tmp.write_all(&serde_json::to_vec_pretty(&self.data).map_err(std::io::Error::other)?)?;
        tmp.as_file().sync_all()?;
        tmp.persist(&self.path).map_err(|e| e.error)?;
        Ok(())
    }
}
