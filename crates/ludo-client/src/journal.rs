//! Journal local : pour chaque (profil, jeu), la révision serveur dont la copie locale descend
//! et l'empreinte de la copie locale à ce moment. Écrit d'un bloc (temporaire puis renommage).
//!
//! Plusieurs processus le partagent (l'app Ludothèque et l'outil `ludo`) : chaque écriture prend un
//! verrou de fichier exclusif (`journal.json.lock`), relit le journal sur le disque, ne change que
//! son entrée puis réécrit. Chaque lecture relit aussi le disque. Ainsi un processus ne réécrit
//! jamais d'anciennes entrées gardées en mémoire par-dessus celles d'un autre.

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
        Ok(Self { path: path.to_path_buf(), data: Self::read(path)? })
    }

    fn read(path: &Path) -> std::io::Result<Data> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(std::io::Error::other),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Data::default()),
            Err(e) => Err(e),
        }
    }

    /// Verrou exclusif du journal, libéré quand le fichier renvoyé est fermé.
    fn lock(&self) -> std::io::Result<std::fs::File> {
        let dir = self.path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)?;
        let mut name = self.path.file_name().unwrap_or_default().to_os_string();
        name.push(".lock");
        let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(name))?;
        f.lock()?;
        Ok(f)
    }

    /// Entrée la plus récente (relue sur le disque ; en cas d'échec de lecture, la dernière connue).
    pub fn get(&mut self, key: &SaveKey) -> Entry {
        if let Ok(d) = Self::read(&self.path) {
            self.data = d;
        }
        self.data.entries.get(&id(key)).cloned().unwrap_or_default()
    }

    pub fn entries(&self) -> impl Iterator<Item = (&String, &Entry)> {
        self.data.entries.iter()
    }

    pub fn set(&mut self, key: &SaveKey, entry: Entry) -> std::io::Result<()> {
        let _lock = self.lock()?;
        self.data = Self::read(&self.path)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn key(game: &str) -> SaveKey {
        SaveKey { profile: "claire".into(), game: game.into() }
    }

    #[test]
    fn deux_processus_ne_s_ecrasent_pas() {
        // Deux journaux ouverts sur le même fichier (l'app et `ludo`), chacun avec son état en mémoire.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.json");
        let mut app = Journal::open(&path).unwrap();
        let mut cli = Journal::open(&path).unwrap();
        app.set(&key("lm3"), Entry { base_rev: 3, ..Entry::default() }).unwrap();
        cli.set(&key("mk8"), Entry { base_rev: 7, ..Entry::default() }).unwrap();
        // L'écriture de `cli` a gardé celle de `app`, et chacun lit la dernière valeur de l'autre.
        let mut relu = Journal::open(&path).unwrap();
        assert_eq!(relu.get(&key("lm3")).base_rev, 3);
        assert_eq!(relu.get(&key("mk8")).base_rev, 7);
        assert_eq!(app.get(&key("mk8")).base_rev, 7);
        assert!(path.with_file_name("journal.json.lock").exists());
    }
}
