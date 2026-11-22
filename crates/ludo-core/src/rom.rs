//! Identifiant d'un jeu lu dans l'en-tête de sa ROM, pour les consoles dont les noms de fichiers ne
//! portent pas d'identifiant (3DS, Wii U, GameCube, DS).
//!
//! L'identifiant sert seulement à retrouver le dossier de sauvegarde **sur l'appareil** : la clé
//! de sauvegarde côté serveur reste l'identifiant du jeu dans le catalogue. Chaque appareil peut
//! donc lire l'en-tête de sa propre copie de la ROM, sans rien changer au serveur.
//!
//! Formats relevés le 21/11/2026 sur les ROM de la Thor (`dd | od`) :
//! - 3DS NCCH (`.cxi`) : « NCCH » à 0x100, identifiant de programme (u64 petit-boutiste) à 0x118 ;
//! - 3DS NCSD (`.3ds`, `.cci`) : « NCSD » à 0x100, identifiant de média (u64 petit-boutiste) à 0x108 ;
//! - GameCube RVZ/WIA : « RVZ\x01 » ou « WIA\x01 » à 0, en-tête du disque recopié à 0x58
//!   (identifiant ASCII de 6 caractères, ex. `GZ2P01`) ; ISO/GCM : identifiant à 0 ;
//! - DS (`.nds`) : code de jeu ASCII de 4 caractères à 0x0C (ex. `ADAF`) ;
//! - Wii U (dossier) : `<title_id>` de `meta/meta.xml`.
//!
//! Les identifiants ASCII (GameCube, DS) sont rangés dans un `title_id` de 16 chiffres
//! hexadécimaux : octets ASCII en gros-boutiste, complétés à gauche par des zéros
//! (`GZ2P01` → `0000475a32503031`). [`ascii_code`] fait la conversion inverse.

/// Octets d'en-tête à lire pour [`title_id_from_header`] (couvre tous les formats ci-dessus).
pub const HEADER_LEN: usize = 0x120;

fn u64_le(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// Code ASCII (lettres et chiffres) rangé dans un entier, comme le fait la Ludothèque.
fn ascii_id(code: &[u8]) -> Option<u64> {
    if code.is_empty() || code.len() > 8 || !code.iter().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(code.iter().fold(0u64, |acc, &c| (acc << 8) | u64::from(c)))
}

/// Code ASCII d'un `title_id` construit par [`ascii_id`] (`0000475a32503031` → `GZ2P01`) ;
/// `None` si l'identifiant n'est pas un code ASCII.
pub fn ascii_code(title_id: u64) -> Option<String> {
    let bytes: Vec<u8> = title_id.to_be_bytes().into_iter().skip_while(|&b| b == 0).collect();
    (!bytes.is_empty() && bytes.iter().all(|c| c.is_ascii_alphanumeric())).then(|| String::from_utf8(bytes).expect("ASCII"))
}

/// Identifiant du jeu d'après l'extension du fichier et ses premiers octets (au moins
/// [`HEADER_LEN`] pour les formats 3DS). `None` : format inconnu ou en-tête illisible (ROM
/// compressée `.zcci`, fichier tronqué…).
pub fn title_id_from_header(ext: &str, header: &[u8]) -> Option<u64> {
    match ext.to_ascii_lowercase().as_str() {
        "cxi" | "3ds" | "cci" => match header.get(0x100..0x104)? {
            b"NCCH" => u64_le(header, 0x118),
            b"NCSD" => u64_le(header, 0x108),
            _ => None,
        }
        .filter(|&t| t >> 32 == 0x0004_0000),
        "rvz" | "wia" => {
            let magic = header.get(0..4)?;
            (magic == b"RVZ\x01" || magic == b"WIA\x01").then_some(())?;
            ascii_id(header.get(0x58..0x5E)?)
        }
        "iso" | "gcm" => ascii_id(header.get(0..6)?),
        "nds" => ascii_id(header.get(0x0C..0x10)?),
        _ => None,
    }
}

/// Identifiant d'un jeu Wii U d'après le texte de son `meta/meta.xml`.
pub fn title_id_from_meta_xml(xml: &str) -> Option<u64> {
    let start = xml.find("<title_id")?;
    let after = &xml[start..];
    let value = &after[after.find('>')? + 1..];
    let end = value.find('<')?;
    let hex = value[..end].trim();
    (hex.len() == 16).then(|| u64::from_str_radix(hex, 16).ok()).flatten()
}

/// Région GameCube d'après la 4e lettre de l'identifiant (dossier `GC/<région>` de Dolphin).
pub fn gc_region(code: &str) -> Option<&'static str> {
    match code.as_bytes().get(3)? {
        b'E' => Some("USA"),
        b'J' => Some("JAP"),
        b'P' | b'D' | b'F' | b'S' | b'I' | b'H' | b'U' | b'X' | b'Y' | b'Z' => Some("EUR"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_at(at: usize, bytes: &[u8]) -> Vec<u8> {
        let mut h = vec![0u8; HEADER_LEN];
        h[at..at + bytes.len()].copy_from_slice(bytes);
        h
    }

    #[test]
    fn cxi_animal_crossing_mesure_sur_la_thor() {
        // Octets lus à 0x100 du .cxi d'Animal Crossing New Leaf Welcome amiibo (Europe).
        let mut h = header_at(0x100, b"NCCH");
        h[0x118..0x120].copy_from_slice(&[0x00, 0x8f, 0x19, 0x00, 0x00, 0x00, 0x04, 0x00]);
        assert_eq!(title_id_from_header("cxi", &h), Some(0x0004_0000_0019_8f00));
    }

    #[test]
    fn ncsd_majoras_mask_correspond_au_dossier_azahar() {
        // .3ds de Majora's Mask 3D : identifiant 00040000 00125600, le dossier de sauvegarde
        // relevé dans Azahar est `title/00040000/00125600`.
        let mut h = header_at(0x100, b"NCSD");
        h[0x108..0x110].copy_from_slice(&[0x00, 0x56, 0x12, 0x00, 0x00, 0x00, 0x04, 0x00]);
        assert_eq!(title_id_from_header("3DS", &h), Some(0x0004_0000_0012_5600));
        assert_eq!(title_id_from_header("cxi", &header_at(0x100, b"XXXX")), None);
        assert_eq!(title_id_from_header("cxi", &[0u8; 16]), None, "en-tête tronqué");
    }

    #[test]
    fn rvz_twilight_princess() {
        let mut h = header_at(0, b"RVZ\x01");
        h[0x58..0x5E].copy_from_slice(b"GZ2P01");
        let t = title_id_from_header("rvz", &h).unwrap();
        assert_eq!(format!("{t:016x}"), "0000475a32503031");
        assert_eq!(ascii_code(t).as_deref(), Some("GZ2P01"));
        assert_eq!(gc_region("GZ2P01"), Some("EUR"));
        assert_eq!(gc_region("GZ2E01"), Some("USA"));
        assert_eq!(title_id_from_header("rvz", &header_at(0, b"ZIP!")), None);
        assert_eq!(title_id_from_header("iso", b"GALE01\0\0"), ascii_id(b"GALE01"));
    }

    #[test]
    fn nds_pokemon_diamant() {
        let mut h = header_at(0, b"POKEMON D");
        h[0x0C..0x10].copy_from_slice(b"ADAF");
        let t = title_id_from_header("nds", &h).unwrap();
        assert_eq!(ascii_code(t).as_deref(), Some("ADAF"));
        assert_eq!(ascii_code(0x0100_1520_0002_2000), None, "identifiant Switch : pas un code ASCII");
    }

    #[test]
    fn meta_xml_wind_waker_hd() {
        let xml = r#"<menu><title_id type="hexBinary" length="8">0005000010143500</title_id></menu>"#;
        assert_eq!(title_id_from_meta_xml(xml), Some(0x0005_0000_1014_3500));
        assert_eq!(title_id_from_meta_xml("<menu/>"), None);
    }
}
