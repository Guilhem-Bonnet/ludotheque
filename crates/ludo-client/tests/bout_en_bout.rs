//! Tests de bout en bout : un vrai `ludo-server` sur un port local, plusieurs appareils
//! simulés (Eden d'un côté, Ryujinx de l'autre), et le binaire `ludo` avec un faux émulateur.

use ludo_client::api::Client;
use ludo_client::engine::{Engine, Resolution, SyncOutcome};
use ludo_client::journal::Journal;
use ludo_client::layout::{ryujinx_uid_bytes, DirLayout, RyujinxLayout, SaveLayout};
use ludo_proto::{LeaseOutcome, SaveKey};
use ludo_server::store::{Store, SystemClock};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MK8: u64 = 0x0100152000022000;
const GUILHEM_RYU: &str = "00000000000000010000000000000000";

/// Démarre un serveur sur un port libre ; renvoie son URL. Le serveur vit jusqu'à la fin du test.
fn server(data: &Path) -> String {
    let store = Arc::new(Store::open(data, Box::new(SystemClock)).unwrap());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let l = tokio::net::TcpListener::from_std(listener).unwrap();
            axum::serve(l, ludo_server::router(store)).await.unwrap();
        });
    });
    format!("http://{addr}")
}

/// Un port sur lequel personne n'écoute.
fn dead_url() -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    format!("http://{addr}")
}

fn write(p: &Path, data: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, data).unwrap();
}

fn ryu_index(root: &Path, idx: &str, title: u64, uid: [u8; 16], kind: u8) -> PathBuf {
    let dir = root.join(idx);
    let mut extra = vec![0u8; 512];
    extra[0..8].copy_from_slice(&title.to_le_bytes());
    extra[8..24].copy_from_slice(&uid);
    extra[32] = kind;
    for f in ["ExtraData0", "ExtraData1"] {
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(f), &extra).unwrap();
    }
    fs::create_dir_all(dir.join("0")).unwrap();
    fs::create_dir_all(dir.join("1")).unwrap();
    dir
}

struct Device {
    name: &'static str,
    emulator: &'static str,
    journal: Journal,
    layout: Box<dyn SaveLayout>,
}

impl Device {
    fn sync(&mut self, client: &Client, key: &SaveKey) -> SyncOutcome {
        let mut e = Engine { client, journal: &mut self.journal, device: self.name.into(), emulator: self.emulator.into(), playtime_s: 1 };
        e.sync(key, self.layout.as_ref()).unwrap()
    }
    fn resolve(&mut self, client: &Client, key: &SaveKey, r: Resolution) -> SyncOutcome {
        let mut e = Engine { client, journal: &mut self.journal, device: self.name.into(), emulator: self.emulator.into(), playtime_s: 1 };
        e.resolve(key, self.layout.as_ref(), r).unwrap()
    }
}

#[test]
fn deux_emulateurs_un_seul_fil_de_sauvegarde() {
    let t = tempfile::tempdir().unwrap();
    let client = Client::new(&server(&t.path().join("srv")));
    let key = SaveKey { profile: "guilhem".into(), game: "switch-mk8".into() };

    // Thor (Eden) et PC (Ryujinx, jeu déjà lancé une fois avec ce profil).
    let eden_acc = t.path().join("thor/nand/user/save/0000000000000000/A2D2/0100152000022000");
    let mut thor = Device {
        name: "thor",
        emulator: "eden",
        journal: Journal::open(&t.path().join("thor/journal.json")).unwrap(),
        layout: Box::new(DirLayout { account: eden_acc.clone(), device: None, exclude: vec![] }),
    };
    let ryu_root = t.path().join("pc/bis/user/save");
    let acc = ryu_index(&ryu_root, "0000000000000002", MK8, ryujinx_uid_bytes(GUILHEM_RYU).unwrap(), 1);
    let mut pc = Device {
        name: "pc",
        emulator: "ryujinx",
        journal: Journal::open(&t.path().join("pc/journal.json")).unwrap(),
        layout: Box::new(RyujinxLayout { save_root: ryu_root, title_id: MK8, user_id: GUILHEM_RYU.into(), exclude: vec![] }),
    };

    // 1. Partie sur la Thor, envoyée.
    write(&eden_acc.join("progress.dat"), "coupe-50cc");
    assert_eq!(thor.sync(&client, &key), SyncOutcome::Uploaded { rev: 1 });
    // 2. Le PC récupère la partie dans Ryujinx.
    assert_eq!(pc.sync(&client, &key), SyncOutcome::Downloaded { rev: 1 });
    assert_eq!(fs::read_to_string(acc.join("0/progress.dat")).unwrap(), "coupe-50cc");
    assert_eq!(pc.sync(&client, &key), SyncOutcome::UpToDate);
    // 3. Partie sur le PC, la Thor la récupère dans Eden.
    write(&acc.join("0/progress.dat"), "coupe-100cc");
    assert_eq!(pc.sync(&client, &key), SyncOutcome::Uploaded { rev: 2 });
    assert_eq!(thor.sync(&client, &key), SyncOutcome::Downloaded { rev: 2 });
    assert_eq!(fs::read_to_string(eden_acc.join("progress.dat")).unwrap(), "coupe-100cc");

    // 4. Les deux jouent sans synchroniser : la Thor envoie, le PC tombe sur un conflit.
    write(&eden_acc.join("progress.dat"), "thor-150cc");
    write(&acc.join("0/progress.dat"), "pc-miroir");
    assert_eq!(thor.sync(&client, &key), SyncOutcome::Uploaded { rev: 3 });
    assert!(matches!(pc.sync(&client, &key), SyncOutcome::Conflict { ref server } if server.device == "thor"));
    // Choix « utiliser la sauvegarde du serveur » : celle du PC part dans l'historique.
    assert!(matches!(pc.resolve(&client, &key, Resolution::KeepServer), SyncOutcome::Downloaded { rev: 3 }));
    assert_eq!(fs::read_to_string(acc.join("0/progress.dat")).unwrap(), "thor-150cc");
    let hist = client.history(&key).unwrap();
    assert!(hist.iter().any(|r| r.discarded && r.device == "pc"), "version écartée gardée : {hist:?}");
    assert_eq!(client.head(&key).unwrap().unwrap().rev, 3);

    // 5. Les profils restent séparés : Claire ne voit rien de Guilhem.
    let claire = SaveKey { profile: "claire".into(), game: "switch-mk8".into() };
    assert!(client.head(&claire).unwrap().is_none());
}

#[test]
fn hors_ligne_puis_envoi_au_retour() {
    let t = tempfile::tempdir().unwrap();
    let key = SaveKey { profile: "claire".into(), game: "n3ds-mm3d".into() };
    let acc = t.path().join("deck/saves");
    let mut deck = Device {
        name: "steam-deck",
        emulator: "azahar",
        journal: Journal::open(&t.path().join("deck/journal.json")).unwrap(),
        layout: Box::new(DirLayout { account: acc.clone(), device: None, exclude: vec![] }),
    };
    write(&acc.join("save.bin"), "temple");

    // Serveur introuvable : « Hors ligne – envoi en attente », rien n'est perdu.
    let offline = Client::new(&dead_url());
    assert_eq!(deck.sync(&offline, &key), SyncOutcome::Offline { pending_upload: true });
    assert!(deck.journal.get(&key).pending_upload);

    // Retour à la maison : l'envoi part.
    let client = Client::new(&server(&t.path().join("srv")));
    assert_eq!(deck.sync(&client, &key), SyncOutcome::Uploaded { rev: 1 });
    assert!(!deck.journal.get(&key).pending_upload);
}

#[test]
fn verrou_partie_en_cours_ailleurs() {
    let t = tempfile::tempdir().unwrap();
    let client = Client::new(&server(&t.path().join("srv")));
    let key = SaveKey { profile: "guilhem".into(), game: "switch-botw".into() };
    assert!(matches!(client.acquire_lease(&key, "pc", false).unwrap(), LeaseOutcome::Granted { .. }));
    assert!(matches!(client.acquire_lease(&key, "thor", false).unwrap(), LeaseOutcome::HeldElsewhere { ref lease } if lease.device == "pc"));
    client.release_lease(&key, "pc").unwrap();
    assert!(matches!(client.acquire_lease(&key, "thor", false).unwrap(), LeaseOutcome::Granted { .. }));
}

/// Le binaire `ludo run` complet : verrou, synchro avant, lancement d'un faux émulateur,
/// synchro après, libération du verrou.
#[cfg(unix)]
#[test]
fn ludo_run_avec_un_faux_emulateur() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    let url = server(&t.path().join("srv"));
    let home = t.path().join("home");
    let data = t.path().join("emu");
    let tid = "0100abf008968000";

    // Faux émulateur : ajoute une ligne à la sauvegarde, comme une partie jouée.
    let script = t.path().join("faux-emu.sh");
    fs::write(&script, format!("#!/bin/sh\necho \"partie $1\" >> '{}/saves/{tid}/progress'\n", data.display())).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    fs::create_dir_all(data.join("saves").join(tid)).unwrap();

    let fiches = t.path().join("fiches");
    write(
        &fiches.join("faux.toml"),
        &format!(
            "id = \"faux\"\nconsoles = [\"test\"]\nformats = [\"bin\"]\nlancement = \"{{bin}} {{rom}}\"\nprofils = \"aucun\"\n\
             sauvegardes = \"{{data}}/saves/{{title_id}}\"\n[appareil.test]\nbin = \"{}\"\ndata = \"{}\"\n",
            script.display(),
            data.display()
        ),
    );
    let cfg = t.path().join("config.toml");
    write(
        &cfg,
        &format!(
            "serveur = \"{url}\"\nappareil = \"test\"\nprofil_defaut = \"guilhem\"\nfiches = \"{}\"\n[profils.guilhem]\n",
            fiches.display()
        ),
    );

    let ludo = env!("CARGO_BIN_EXE_ludo");
    let out = std::process::Command::new(ludo)
        .env("HOME", &home)
        .args(["--config", cfg.to_str().unwrap(), "run", "--jeu", "switch-sm", "--emulateur", "faux", "--title-id", tid, "--rom", "rom.bin"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}\n{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("À jour – sauvegarde envoyée"), "{stdout}");

    // La partie est sur le serveur, et le verrou est libéré.
    let client = Client::new(&url);
    let key = SaveKey { profile: "guilhem".into(), game: "switch-sm".into() };
    assert_eq!(client.head(&key).unwrap().unwrap().rev, 1);
    assert!(matches!(client.acquire_lease(&key, "autre", false).unwrap(), LeaseOutcome::Granted { .. }));
    client.release_lease(&key, "autre").unwrap();

    // `ludo statut` reflète l'état.
    let out = std::process::Command::new(ludo)
        .env("HOME", &home)
        .args(["--config", cfg.to_str().unwrap(), "statut"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("guilhem/switch-sm\tÀ jour\trévision 1"));
}
