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

/// Extensions lues par [`title_id_from_header`] ; tout autre nom est traité comme un dossier de jeu
/// Wii U (`meta/meta.xml`).
pub const HEADER_EXTS: &[&str] = &["cxi", "3ds", "cci", "rvz", "wia", "iso", "gcm", "nds"];

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
        // Jeux (00040000) et démos (00040002) ; le reste (système, mises à jour) n'a pas de sauvegarde ici.
        .filter(|&t| matches!(t >> 32, 0x0004_0000 | 0x0004_0002)),
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

/// Formats Switch à table de fichiers PFS0 en clair (`.nsp`, et `.nsz` qui garde la même table).
/// Les `.xci`/`.xcz` n'en ont pas : leur identifiant de titre est dans une partie chiffrée (relevé le
/// 23/11 sur la Thor : « HEAD » à 0x100 puis l'identifiant de paquet, pas celui du titre), seul leur
/// nom `[<titre>]` le donne.
pub const PFS0_EXTS: &[&str] = &["nsp", "nsz"];

/// Octets à lire pour [`title_id_from_pfs0`] : en-tête, entrées et table des noms d'un paquet de
/// quelques fichiers (relevé : 6 à 7 fichiers, table finie avant 0x1A0).
pub const PFS0_HEADER_LEN: usize = 0x400;

fn u32_le(b: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// Identifiant de titre d'un `.nsp`/`.nsz` d'après sa table de fichiers PFS0 : le ticket s'appelle
/// `<identifiant de droits>.tik` (32 chiffres hexadécimaux), dont les 16 premiers sont l'identifiant
/// du titre contenu : celui du jeu de base pour un paquet de base, celui de la mise à jour
/// (`…800`) ou du contenu additionnel pour les autres. Relevé le 23/11 sur la Thor :
/// « Pokémon FireRed Version (French Ver.) [01004B3023412000][v0].nsp » contient
/// `01004b30234120000000000000000012.tik`. `None` : pas de PFS0, pas de ticket (paquet sans ticket),
/// ou table coupée par la lecture.
pub fn title_id_from_pfs0(header: &[u8]) -> Option<u64> {
    if header.get(0..4)? != b"PFS0" {
        return None;
    }
    let count = u32_le(header, 4)?;
    let table = 0x10usize.checked_add(count.checked_mul(0x18)?)?;
    let names = header.get(table..table.checked_add(u32_le(header, 8)?)?.min(header.len()))?;
    (0..count).find_map(|i| {
        let name_at = u32_le(header, 0x10 + i * 0x18 + 0x10)?;
        let rest = names.get(name_at..)?;
        // Nom terminé par un zéro DANS la lecture : un nom coupé n'est jamais une preuve.
        let name = std::str::from_utf8(&rest[..rest.iter().position(|&b| b == 0)?]).ok()?;
        let rights = name.strip_suffix(".tik")?;
        (rights.len() == 32 && rights.chars().all(|c| c.is_ascii_hexdigit())).then(|| u64::from_str_radix(&rights[..16], 16).ok())?
    })
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

    /// Paquet PFS0 minimal avec ces noms de fichiers (tailles et décalages des données à zéro).
    fn pfs0(names: &[&str]) -> Vec<u8> {
        let mut table = Vec::new();
        let mut entries = Vec::new();
        for n in names {
            entries.extend_from_slice(&[0u8; 16]);
            entries.extend_from_slice(&(table.len() as u32).to_le_bytes());
            entries.extend_from_slice(&[0u8; 4]);
            table.extend_from_slice(n.as_bytes());
            table.push(0);
        }
        let mut h = b"PFS0".to_vec();
        h.extend_from_slice(&(names.len() as u32).to_le_bytes());
        h.extend_from_slice(&(table.len() as u32).to_le_bytes());
        h.extend_from_slice(&[0u8; 4]);
        h.extend(entries);
        h.extend(table);
        h
    }

    #[test]
    fn nsp_de_la_thor_identifiant_du_ticket() {
        // Tables relevées le 23/11 sur la Thor (`dd | strings`).
        let rouge_feu = pfs0(&[
            "01004b30234120000000000000000012.tik",
            "01004b30234120000000000000000012.cert",
            "4e62aaa758be8b3eb66588e28c0d756c.cnmt.nca",
            "eca366f88d4dfc4746b6b474c93b1d57.nca",
            "728718a889070f53aef3ef51ffb70da5.nca",
            "fcf9c056c609eea18bc0b76fb752f313.nca",
        ]);
        assert_eq!(title_id_from_pfs0(&rouge_feu), Some(0x0100_4b30_2341_2000));
        // Ticket après le certificat (Super Mario Party, Skyward Sword HD).
        let smp = pfs0(&["010036b0034e40000000000000000005.cert", "010036b0034e40000000000000000005.tik", "9942e4e65076669f2fcb98f140e2d7e5.cnmt.xml"]);
        assert_eq!(title_id_from_pfs0(&smp), Some(0x0100_36b0_034e_4000));
        // Mise à jour : l'identifiant lu est celui de la mise à jour (…800), jamais celui du jeu de base.
        let maj = pfs0(&["0100f2c0115b68000000000000000010.tik", "a.nca"]);
        assert_eq!(title_id_from_pfs0(&maj), Some(0x0100_f2c0_115b_6800));
        assert!(rouge_feu.len() <= PFS0_HEADER_LEN);
    }

    #[test]
    fn pfs0_sans_ticket_coupe_ou_autre_format() {
        assert_eq!(title_id_from_pfs0(&pfs0(&["4e62aaa758be8b3eb66588e28c0d756c.cnmt.nca", "eca366f8.nca"])), None, "paquet sans ticket");
        let full = pfs0(&["a.nca", "01004b30234120000000000000000012.tik"]);
        assert_eq!(title_id_from_pfs0(&full[..full.len() - 3]), None, "nom du ticket coupé par la lecture");
        assert_eq!(title_id_from_pfs0(b"HEAD\0\0\0\0"), None, "xci");
        assert_eq!(title_id_from_pfs0(&pfs0(&["0100zz30234120000000000000000012.tik"])), None, "pas hexadécimal");
        let mut absurd = pfs0(&["x.tik"]);
        absurd[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(title_id_from_pfs0(&absurd), None, "nombre d'entrées absurde");
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
