//! Adaptateurs de rangement : traduction entre la **forme neutre** d'une sauvegarde et
//! l'arborescence propre à chaque émulateur (le principe des « root overrides » de Steam Cloud).
//!
//! Forme neutre d'une sauvegarde Switch (et de tout émulateur) : une archive qui contient
//! - `account/…` : la sauvegarde liée au profil (Switch : sauvegarde « compte ») ;
//! - `device/…`  : la sauvegarde liée à la console, si le jeu en a une (Switch : « appareil »).
//!
//! Le contenu de chaque partie est exactement ce qu'a écrit le jeu : il passe tel quel d'Eden à
//! Ryujinx et inversement.

use ludo_core::snapshot::{self, Snapshot, SnapshotError};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error(transparent)]
    Snapshot(#[from] SnapshotError),
    #[error("entrée-sortie : {0}")]
    Io(#[from] std::io::Error),
    /// Ryujinx n'a pas encore créé l'emplacement de sauvegarde de ce jeu pour ce profil.
    #[error("emplacement de sauvegarde absent : lancer une première fois le jeu avec ce profil ({0})")]
    NeedsFirstLaunch(String),
}

/// Un adaptateur sait exporter la sauvegarde locale en forme neutre et l'importer.
pub trait SaveLayout {
    /// Instantané neutre de la sauvegarde locale (`None` si aucune).
    fn export(&self) -> Result<Option<Snapshot>, LayoutError>;
    /// Remplace la sauvegarde locale par l'archive neutre.
    fn import(&self, archive: &[u8]) -> Result<(), LayoutError>;
}

const PARTS: [&str; 2] = ["account", "device"];

/// Construit l'archive neutre à partir des dossiers de chaque partie.
fn export_parts(parts: &[(&str, Option<PathBuf>)], exclude: &[String]) -> Result<Option<Snapshot>, LayoutError> {
    let staging = tempfile::tempdir()?;
    let mut any = false;
    for (name, dir) in parts {
        let Some(dir) = dir else { continue };
        if let Some(s) = snapshot::snapshot(dir, exclude)? {
            snapshot::restore(&staging.path().join(name), &s.archive, &[])?;
            any = true;
        }
    }
    if !any {
        return Ok(None);
    }
    Ok(snapshot::snapshot(staging.path(), &[])?)
}

/// Découpe l'archive neutre et restaure chaque partie dans ses dossiers cibles.
fn import_parts(archive: &[u8], targets: &dyn Fn(&str) -> Result<Vec<PathBuf>, LayoutError>, exclude: &[String]) -> Result<(), LayoutError> {
    snapshot::validate_archive(archive)?;
    let staging = tempfile::tempdir()?;
    let unpacked = staging.path().join("neutre");
    snapshot::restore(&unpacked, archive, &[])?;
    for name in PARTS {
        let Some(part) = snapshot::snapshot(&unpacked.join(name), &[])? else { continue };
        for target in targets(name)? {
            snapshot::restore(&target, &part.archive, exclude)?;
        }
    }
    Ok(())
}

/// Rangement « un dossier par partie » : Eden, Lichii, Azahar, melonDS…
pub struct DirLayout {
    pub account: PathBuf,
    pub device: Option<PathBuf>,
    pub exclude: Vec<String>,
}

impl SaveLayout for DirLayout {
    fn export(&self) -> Result<Option<Snapshot>, LayoutError> {
        export_parts(&[("account", Some(self.account.clone())), ("device", self.device.clone())], &self.exclude)
    }

    fn import(&self, archive: &[u8]) -> Result<(), LayoutError> {
        import_parts(
            archive,
            &|name| {
                Ok(match name {
                    "account" => vec![self.account.clone()],
                    _ => self.device.clone().into_iter().collect(),
                })
            },
            &self.exclude,
        )
    }
}

/// Sauvegarde d'un seul fichier nommé d'après la ROM de l'appareil (melonDS : `<rom>.sav`). Dans
/// l'archive neutre, le fichier porte un nom fixe (`neutral`) : une partie passe ainsi entre deux
/// appareils dont la ROM est nommée différemment, ou après un renommage de la ROM.
pub struct NamedFileLayout {
    /// Dossier partagé, filtré sur le seul fichier du jeu (motif « ! » échappé).
    pub inner: DirLayout,
    /// Nom du fichier sur cet appareil.
    pub local: String,
    /// Nom du fichier dans l'archive neutre.
    pub neutral: String,
}

/// Copie de l'archive où `account/<from>` devient `account/<to>` (le reste à l'identique).
fn rename_in_archive(archive: &[u8], from: &str, to: &str) -> Result<Snapshot, LayoutError> {
    let staging = tempfile::tempdir()?;
    let dir = staging.path().join("neutre");
    snapshot::restore(&dir, archive, &[])?;
    let (src, dst) = (dir.join("account").join(from), dir.join("account").join(to));
    if src.is_file() && !dst.exists() {
        std::fs::rename(&src, &dst)?;
    }
    snapshot::snapshot(&dir, &[])?.ok_or(LayoutError::Snapshot(SnapshotError::EmptyArchive))
}

impl SaveLayout for NamedFileLayout {
    fn export(&self) -> Result<Option<Snapshot>, LayoutError> {
        match self.inner.export()? {
            Some(s) => Ok(Some(rename_in_archive(&s.archive, &self.local, &self.neutral)?)),
            None => Ok(None),
        }
    }

    fn import(&self, archive: &[u8]) -> Result<(), LayoutError> {
        self.inner.import(&rename_in_archive(archive, &self.neutral, &self.local)?.archive)
    }
}

/// Rangement Ryujinx : `bis/user/save/<index>/{0,1}`, l'index étant relié au jeu et au profil
/// par `ExtraData0` (mesuré le 14/11 sur Ryubing 1.3.3) :
/// octets 0..8 = identifiant du jeu (u64 LE), 8..24 = identifiant utilisateur,
/// octet 32 = type (1 = compte, 3 = appareil).
pub struct RyujinxLayout {
    /// Dossier `bis/user/save`.
    pub save_root: PathBuf,
    pub title_id: u64,
    /// Identifiant utilisateur Ryujinx (`user_id` de `Profiles.json`, 32 caractères hexadécimaux).
    pub user_id: String,
    pub exclude: Vec<String>,
}

const TYPE_ACCOUNT: u8 = 1;
const TYPE_DEVICE: u8 = 3;

/// Convertit l'identifiant utilisateur Ryujinx (« 00000000000000010000000000000000 ») vers les
/// 16 octets d'`ExtraData0` : deux u64 petit-boutistes, la moitié gauche d'abord.
pub fn ryujinx_uid_bytes(user_id: &str) -> Option<[u8; 16]> {
    if user_id.len() != 32 {
        return None;
    }
    let hi = u64::from_str_radix(&user_id[..16], 16).ok()?;
    let lo = u64::from_str_radix(&user_id[16..], 16).ok()?;
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&hi.to_le_bytes());
    out[8..].copy_from_slice(&lo.to_le_bytes());
    Some(out)
}

impl RyujinxLayout {
    /// Index de sauvegarde du jeu pour ce type (et ce profil pour le type compte).
    fn find(&self, kind: u8) -> Result<Option<PathBuf>, LayoutError> {
        let uid = if kind == TYPE_ACCOUNT {
            ryujinx_uid_bytes(&self.user_id).ok_or_else(|| LayoutError::NeedsFirstLaunch(format!("user_id invalide {}", self.user_id)))?
        } else {
            [0u8; 16]
        };
        let Ok(entries) = std::fs::read_dir(&self.save_root) else { return Ok(None) };
        let mut found = Vec::new();
        for e in entries {
            let dir = e?.path();
            let Ok(extra) = std::fs::read(dir.join("ExtraData0")) else { continue };
            if extra.len() < 33 {
                continue;
            }
            let program = u64::from_le_bytes(extra[0..8].try_into().expect("8 octets"));
            if program == self.title_id && extra[8..24] == uid && extra[32] == kind {
                found.push(dir);
            }
        }
        found.sort();
        Ok(found.pop())
    }

    fn committed(dir: &Path) -> PathBuf {
        dir.join("0")
    }
}

impl SaveLayout for RyujinxLayout {
    fn export(&self) -> Result<Option<Snapshot>, LayoutError> {
        let account = self.find(TYPE_ACCOUNT)?.map(|d| Self::committed(&d));
        let device = self.find(TYPE_DEVICE)?.map(|d| Self::committed(&d));
        export_parts(&[("account", account), ("device", device)], &self.exclude)
    }

    fn import(&self, archive: &[u8]) -> Result<(), LayoutError> {
        import_parts(
            archive,
            &|name| {
                let kind = if name == "account" { TYPE_ACCOUNT } else { TYPE_DEVICE };
                let dir = self.find(kind)?.ok_or_else(|| {
                    LayoutError::NeedsFirstLaunch(format!("{:016x}, sauvegarde {name}", self.title_id))
                })?;
                // Ryujinx garde deux copies identiques au repos : « 0 » (validée) et « 1 ».
                Ok(vec![dir.join("0"), dir.join("1")])
            },
            &self.exclude,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(p: &Path, data: &[u8]) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, data).unwrap();
    }

    /// Crée un index Ryujinx factice, au format mesuré.
    pub fn ryu_index(root: &Path, idx: &str, title: u64, uid: [u8; 16], kind: u8) -> PathBuf {
        let dir = root.join(idx);
        let mut extra = vec![0u8; 512];
        extra[0..8].copy_from_slice(&title.to_le_bytes());
        extra[8..24].copy_from_slice(&uid);
        extra[32] = kind;
        write(&dir.join("ExtraData0"), &extra);
        write(&dir.join("ExtraData1"), &extra);
        fs::create_dir_all(dir.join("0")).unwrap();
        fs::create_dir_all(dir.join("1")).unwrap();
        dir
    }

    #[test]
    fn conversion_identifiant_utilisateur_conforme_a_la_mesure() {
        // Mesuré : user_id « 00000000000000010000000000000000 » ↔ octets 01 00 … 00.
        let b = ryujinx_uid_bytes("00000000000000010000000000000000").unwrap();
        assert_eq!(b[0], 1);
        assert!(b[1..].iter().all(|&x| x == 0));
        assert!(ryujinx_uid_bytes("court").is_none());
    }

    #[test]
    fn eden_vers_ryujinx_et_retour() {
        const MK8: u64 = 0x0100152000022000;
        let t = tempfile::tempdir().unwrap();
        // Côté Eden : compte + appareil.
        let eden = DirLayout {
            account: t.path().join("eden/nand/user/save/0000000000000000/AAAA/0100152000022000"),
            device: Some(t.path().join("eden/nand/user/save/0000000000000000/00000000000000000000000000000000/0100152000022000")),
            exclude: vec![],
        };
        write(&eden.account.join("progress.dat"), b"coupe-150cc");
        write(&eden.device.as_ref().unwrap().join("system.dat"), b"reglages-console");
        let neutral = eden.export().unwrap().unwrap();

        // Côté Ryujinx : les index existent déjà (jeu lancé une fois avec ce profil).
        let root = t.path().join("ryu/bis/user/save");
        let uid = ryujinx_uid_bytes("00000000000000010000000000000000").unwrap();
        let acc = ryu_index(&root, "0000000000000002", MK8, uid, TYPE_ACCOUNT);
        let dev = ryu_index(&root, "0000000000000003", MK8, [0; 16], TYPE_DEVICE);
        ryu_index(&root, "0000000000000004", 0x0100dca0064a6000, uid, TYPE_ACCOUNT); // autre jeu
        let ryu = RyujinxLayout { save_root: root, title_id: MK8, user_id: "00000000000000010000000000000000".into(), exclude: vec![] };
        ryu.import(&neutral.archive).unwrap();

        for c in ["0", "1"] {
            assert_eq!(fs::read(acc.join(c).join("progress.dat")).unwrap(), b"coupe-150cc");
            assert_eq!(fs::read(dev.join(c).join("system.dat")).unwrap(), b"reglages-console");
        }
        // Même forme neutre dans les deux sens : même empreinte.
        assert_eq!(ryu.export().unwrap().unwrap().hash, neutral.hash);

        // Et retour vers un Eden vierge.
        let eden2 = DirLayout { account: t.path().join("eden2/a"), device: Some(t.path().join("eden2/d")), exclude: vec![] };
        eden2.import(&ryu.export().unwrap().unwrap().archive).unwrap();
        assert_eq!(fs::read(eden2.account.join("progress.dat")).unwrap(), b"coupe-150cc");
        assert_eq!(eden2.export().unwrap().unwrap().hash, neutral.hash);
    }

    #[test]
    fn ryujinx_sans_index_demande_un_premier_lancement() {
        let t = tempfile::tempdir().unwrap();
        let eden = DirLayout { account: t.path().join("a"), device: None, exclude: vec![] };
        write(&eden.account.join("s"), b"1");
        let neutral = eden.export().unwrap().unwrap();
        let ryu = RyujinxLayout {
            save_root: t.path().join("vide"),
            title_id: 1,
            user_id: "00000000000000010000000000000000".into(),
            exclude: vec![],
        };
        assert!(ryu.export().unwrap().is_none());
        assert!(matches!(ryu.import(&neutral.archive), Err(LayoutError::NeedsFirstLaunch(_))));
    }

    #[test]
    fn les_profils_ryujinx_restent_separes() {
        const T: u64 = 0x0100ff500e34a000;
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("save");
        let guilhem = "00000000000000010000000000000000";
        let claire = "00000000000000020000000000000000";
        let g = ryu_index(&root, "0000000000000001", T, ryujinx_uid_bytes(guilhem).unwrap(), TYPE_ACCOUNT);
        let c = ryu_index(&root, "0000000000000002", T, ryujinx_uid_bytes(claire).unwrap(), TYPE_ACCOUNT);
        write(&g.join("0/s"), b"guilhem");
        write(&c.join("0/s"), b"claire");
        let lg = RyujinxLayout { save_root: root.clone(), title_id: T, user_id: guilhem.into(), exclude: vec![] };
        let lc = RyujinxLayout { save_root: root, title_id: T, user_id: claire.into(), exclude: vec![] };
        assert_ne!(lg.export().unwrap().unwrap().hash, lc.export().unwrap().unwrap().hash);
        lc.import(&lg.export().unwrap().unwrap().archive).unwrap();
        assert_eq!(fs::read(g.join("0/s")).unwrap(), b"guilhem", "Guilhem intact");
    }

    #[test]
    fn sav_melonds_entre_deux_noms_de_rom() {
        let t = tempfile::tempdir().unwrap();
        let mk = |dir: &str, rom: &str| NamedFileLayout {
            inner: DirLayout {
                account: t.path().join(dir),
                device: None,
                exclude: vec![format!("!{}", ludo_core::snapshot::escape_wildcards(&format!("{rom}.sav")))],
            },
            local: format!("{rom}.sav"),
            neutral: "sauvegarde.sav".into(),
        };
        // Thor : nom No-Intro ; autre appareil : nom court. Un autre jeu partage le dossier.
        let thor = mk("thor/saves", "Pokemon - Version Diamant (France) (Rev 5)");
        write(&t.path().join("thor/saves/Pokemon - Version Diamant (France) (Rev 5).sav"), b"badges-8");
        write(&t.path().join("thor/saves/Mario Kart DS.sav"), b"mkds");
        let neutral = thor.export().unwrap().unwrap();
        let deck = mk("deck/saves", "Pokemon Diamant");
        write(&t.path().join("deck/saves/Zelda.sav"), b"zelda");
        deck.import(&neutral.archive).unwrap();
        assert_eq!(fs::read(t.path().join("deck/saves/Pokemon Diamant.sav")).unwrap(), b"badges-8");
        assert_eq!(fs::read(t.path().join("deck/saves/Zelda.sav")).unwrap(), b"zelda", "autre jeu intact");
        assert_eq!(deck.export().unwrap().unwrap().hash, neutral.hash, "même forme neutre sur les deux appareils");
        assert!(mk("vide", "X").export().unwrap().is_none());
    }
}
