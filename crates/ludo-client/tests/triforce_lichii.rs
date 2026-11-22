//! Ocarina of Time (Switch 2) : une partie passe du portage natif Triforce à l'émulateur Lichii, et
//! retour, par la forme neutre de la Ludothèque (mêmes fichiers `account.sav`, `option.sav`,
//! `slot_NN/…` des deux côtés). Tout se passe dans des dossiers temporaires, jamais dans les vraies
//! sauvegardes.
//!
//! Variante sur une copie réelle (lancée à la main, ignorée par défaut) :
//! `LUDO_PREUVE_TRIFORCE=<copie du dossier save/<utilisateur> de Triforce> cargo test --test triforce_lichii -- --ignored`

use ludo_client::config::{self, Config};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const OOT: &str = "0400486024DEC000";
const TRIFORCE_USER: &str = "5a3c9e1f0b7d24680000000000000001";
const LICHII_USER: &str = "64BB29852878F9D9D9EBF99A67E3E6BD";

/// Configuration d'un PC où Triforce et Lichii ont leurs données dans `root` (fiches du dépôt).
fn cfg(root: &Path) -> Config {
    toml::from_str(&format!(
        r#"
        serveur = "x"
        appareil = "pc"
        [emulateurs.triforce]
        bin = "triforce-pc"
        data = "{root}/hookshot"
        [emulateurs.lichii]
        bin = "lichii-pc"
        data = "{root}/lichii"
        [profils.guilhem.natifs.triforce]
        uuid = "{TRIFORCE_USER}"
        [profils.guilhem.natifs.lichii]
        uuid = "{LICHII_USER}"
        "#,
        root = root.display()
    ))
    .unwrap()
}

/// Tous les fichiers d'un dossier : chemin relatif → contenu.
fn files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    for e in walk(dir) {
        out.insert(e.strip_prefix(dir).unwrap().to_string_lossy().into_owned(), fs::read(&e).unwrap());
    }
    out
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(dir) else { return out };
    for e in rd {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

fn copy_dir(from: &Path, to: &Path) {
    for f in walk(from) {
        let dest = to.join(f.strip_prefix(from).unwrap());
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::copy(&f, &dest).unwrap();
    }
}

/// Triforce → Lichii → Triforce vierge : contenu identique octet pour octet, même empreinte neutre.
fn aller_retour(root: &Path, triforce_save: &Path) {
    let c = cfg(root);
    let tri = config::prepare(&c, "triforce", "guilhem", OOT).unwrap();
    assert_eq!(tri.dirs[0], root.join("hookshot/save").join(TRIFORCE_USER), "dossier Triforce de la fiche");
    copy_dir(triforce_save, &tri.dirs[0]);
    let before = files(&tri.dirs[0]);
    assert!(before.contains_key("account.sav") && before.keys().any(|k| k.starts_with("slot_")));
    let neutral = tri.layout.export().unwrap().expect("sauvegarde Triforce");

    let lichii = config::prepare(&c, "lichii", "guilhem", OOT).unwrap();
    let lichii_dir = root.join("lichii/nand/user/save/0000000000000000").join(LICHII_USER).join(OOT);
    assert_eq!(lichii.dirs[0], lichii_dir, "dossier Lichii de la fiche (titre en majuscules)");
    lichii.layout.import(&neutral.archive).unwrap();
    assert_eq!(files(&lichii_dir), before, "Lichii reçoit exactement les fichiers de Triforce");
    assert_eq!(lichii.layout.export().unwrap().unwrap().hash, neutral.hash, "même forme neutre des deux côtés");

    // Retour vers un Triforce vierge (la partie continuée dans Lichii revient au portage).
    fs::rename(&tri.dirs[0], root.join("triforce-ancien")).unwrap();
    tri.layout.import(&lichii.layout.export().unwrap().unwrap().archive).unwrap();
    assert_eq!(files(&tri.dirs[0]), before, "retour identique dans Triforce");
}

#[test]
fn ocarina_of_time_triforce_vers_lichii_et_retour() {
    let t = tempfile::tempdir().unwrap();
    // Arborescence relevée le 21/11 sur le PC (~/.local/share/hookshot/save/<utilisateur>/).
    let save = t.path().join("modele");
    fs::create_dir_all(&save).unwrap();
    fs::write(save.join("account.sav"), b"compte").unwrap();
    fs::write(save.join("option.sav"), b"options").unwrap();
    for n in 0..6 {
        let slot = save.join(format!("slot_{n:02}"));
        fs::create_dir_all(&slot).unwrap();
        fs::write(slot.join("progress.sav"), format!("progression {n}")).unwrap();
        fs::write(slot.join("caption.sav"), format!("vignette {n}")).unwrap();
    }
    aller_retour(&t.path().join("pc"), &save);
}

#[test]
#[ignore = "preuve manuelle sur une COPIE des vraies sauvegardes (LUDO_PREUVE_TRIFORCE)"]
fn ocarina_of_time_copie_reelle() {
    let src = PathBuf::from(std::env::var("LUDO_PREUVE_TRIFORCE").expect("LUDO_PREUVE_TRIFORCE = copie du dossier de sauvegarde Triforce"));
    let home = std::env::var("HOME").unwrap_or_default();
    assert!(!src.starts_with(Path::new(&home).join(".local/share/hookshot")), "jamais le vrai dossier : passer une copie");
    let t = tempfile::tempdir().unwrap();
    aller_retour(t.path(), &src);
    println!("preuve OK : {} fichiers transférés Triforce → Lichii → Triforce", files(&src).len());
}
