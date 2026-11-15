//! Lancement d'un jeu : vrai `ludo-server` en processus, faux émulateur (script shell) décrit par
//! une fiche de test. Chaque cas vérifie aussi l'état du verrou de partie sur le serveur.

use ludo_client::api::Client;
use ludo_client::config::{Config, GameConf};
use ludo_client::engine::{Resolution, SyncOutcome};
use ludo_client::install::Installer;
use ludo_client::journal::Journal;
use ludo_client::launch::{self, Env, LaunchError, LaunchOutcome, Observer, Step, StepState};
use ludo_proto::{CatalogGame, FileRole, LeaseOutcome, PushOutcome, PushParams, SaveKey};
use ludo_server::store::{Store, SystemClock};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const TITLE: &str = "0100152000022000";

fn spawn(url_data: &Path, roms: &Path) -> String {
    let store = Store::open(url_data, Box::new(SystemClock)).unwrap().with_roms(roms.to_path_buf());
    store.rescan().unwrap();
    let router = ludo_server::router(Arc::new(store));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(async move {
            axum::serve(tokio::net::TcpListener::from_std(listener).unwrap(), router).await.unwrap();
        });
    });
    format!("http://{addr}")
}

struct Rig {
    url: String,
    dir: tempfile::TempDir,
    cfg: Config,
    journal: PathBuf,
    lock: Mutex<()>,
    game: String,
}

impl Rig {
    fn save_dir(&self) -> PathBuf {
        self.dir.path().join("emu/saves/claire").join(TITLE)
    }
    fn marker(&self) -> PathBuf {
        self.dir.path().join("emulateur-lance")
    }
    fn key(&self) -> SaveKey {
        SaveKey { profile: "claire".into(), game: self.game.clone() }
    }
    fn env(&self) -> Env<'_> {
        Env { cfg: &self.cfg, journal_path: &self.journal, journal_lock: &self.lock }
    }
    fn client(&self) -> Client {
        Client::new(&self.url)
    }
    /// L'appareil `autre` peut-il prendre le verrou ? (donc : plus personne ne le tient)
    fn lease_free(&self) -> bool {
        let c = self.client();
        let granted = matches!(c.acquire_lease(&self.key(), "verif", false).unwrap(), LeaseOutcome::Granted { .. });
        c.release_lease(&self.key(), "verif").unwrap();
        granted
    }
    /// Un autre appareil envoie une sauvegarde (rév. suivante) ; renvoie l'archive.
    fn push_from_other(&self, content: &str, base_rev: u64, playtime_s: u64) -> u64 {
        let other = self.dir.path().join("autre-appareil");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("save.dat"), content).unwrap();
        let layout = ludo_client::layout::DirLayout { account: other, device: None, exclude: vec![] };
        let snap = ludo_client::layout::SaveLayout::export(&layout).unwrap().unwrap();
        let p = PushParams { base_rev, device: "steam-deck".into(), emulator: "fake".into(), playtime_s, discarded: false };
        match self.client().push(&self.key(), &p, &snap.archive).unwrap() {
            PushOutcome::Accepted { revision } => revision.rev,
            o => panic!("{o:?}"),
        }
    }
}

/// `bin` : commande de l'émulateur (None = faux émulateur qui écrit dans la sauvegarde).
fn rig(bin: Option<&str>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let server_roms = dir.path().join("server-roms");
    fs::create_dir_all(server_roms.join("switch")).unwrap();
    fs::write(server_roms.join(format!("switch/Jeu [{TITLE}][v0].nsp")), vec![5u8; 4000]).unwrap();
    let url = spawn(&dir.path().join("server-data"), &server_roms);

    let script = dir.path().join("faux-emulateur.sh");
    fs::write(&script, format!("#!/bin/sh\ntouch '{}'\nmkdir -p \"$2\"\necho jouee >> \"$2/save.dat\"\nsleep 1\n", dir.path().join("emulateur-lance").display())).unwrap();
    fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

    let fiches = dir.path().join("fiches");
    fs::create_dir_all(&fiches).unwrap();
    fs::write(
        fiches.join("fake.toml"),
        format!(
            "id = \"fake\"\nconsoles = [\"switch\"]\nformats = [\"nsp\"]\nlancement = \"{{bin}} {{rom}} {{data}}/saves/{{profil}}/{{title_id}}\"\nprofils = \"aucun\"\nsauvegardes = \"{{data}}/saves/{{profil}}/{{title_id}}\"\n\n[appareil.pc]\nbin = \"{}\"\ndata = \"{}\"\n",
            bin.map_or(script.display().to_string(), str::to_string),
            dir.path().join("emu").display()
        ),
    )
    .unwrap();

    let game = Client::new(&url).games().unwrap().into_iter().next().unwrap().id;
    let cfg = Config {
        serveur: url.clone(),
        appareil: "pc".into(),
        profil_defaut: Some("claire".into()),
        fiches: Some(fiches),
        roms: Some(dir.path().join("device-roms")),
        profils: BTreeMap::from([("claire".to_string(), Default::default())]),
        jeux: BTreeMap::from([(game.clone(), GameConf { emulateur: "fake".into(), title_id: TITLE.into(), profils: None })]),
    };
    let journal = dir.path().join("journal.json");
    Rig { url, dir, cfg, journal, lock: Mutex::new(()), game }
}

#[derive(Default)]
struct Obs {
    steps: Vec<(Step, StepState)>,
    installs: usize,
    device_roms: PathBuf,
    server: String,
}

impl Observer for Obs {
    fn step(&mut self, step: Step, state: StepState, _detail: Option<String>) {
        self.steps.push((step, state));
    }
    fn install(&mut self, game: &CatalogGame) -> Result<(), String> {
        self.installs += 1;
        let client = Client::new(&self.server);
        let f = game.files.iter().find(|f| f.role == FileRole::Base).unwrap();
        Installer::new(&client, &self.device_roms).install(&game.id, f, &mut |_, _| {}).map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn obs(r: &Rig) -> Obs {
    Obs { device_roms: r.cfg.roms.clone().unwrap(), server: r.url.clone(), ..Default::default() }
}

#[test]
fn lancement_normal_telechargement_jeu_puis_envoi_avec_temps_de_jeu() {
    let r = rig(None);
    let rev1 = r.push_from_other("depart\n", 0, 100);
    let mut o = obs(&r);
    let out = launch::launch(&r.env(), &r.key(), &mut o).unwrap();

    assert_eq!(o.installs, 1, "le jeu manquait : installation");
    let rom = r.cfg.roms.as_ref().unwrap().join(format!("switch/Jeu [{TITLE}][v0].nsp"));
    assert!(rom.exists());
    assert!(r.marker().exists(), "émulateur lancé");
    let LaunchOutcome::Played { before, after, playtime_s, exit_ok, .. } = out else { panic!("{out:?}") };
    assert_eq!(before, SyncOutcome::Downloaded { rev: rev1 });
    assert!(matches!(after, SyncOutcome::Uploaded { rev } if rev == rev1 + 1), "{after:?}");
    assert!(exit_ok && playtime_s >= 100, "temps de jeu cumulé : {playtime_s}");
    // La sauvegarde du serveur contient la partie, avec le temps de jeu.
    let head = r.client().head(&r.key()).unwrap().unwrap();
    assert_eq!((head.rev, head.device.as_str(), head.playtime_s), (rev1 + 1, "pc", playtime_s));
    assert_eq!(fs::read_to_string(r.save_dir().join("save.dat")).unwrap(), "depart\njouee\n");
    let ids: Vec<_> = o.steps.iter().filter(|(_, s)| *s == StepState::Fait).map(|(s, _)| s.id()).collect();
    assert_eq!(ids, ["profil", "telechargement", "sauvegarde", "lancement", "en_jeu", "envoi", "termine"]);
    assert!(r.lease_free(), "verrou rendu");

    // Deuxième lancement : jeu déjà là, rien à télécharger.
    let mut o2 = obs(&r);
    launch::launch(&r.env(), &r.key(), &mut o2).unwrap();
    assert_eq!(o2.installs, 0);
}

#[test]
fn conflit_avant_la_partie_aucun_lancement() {
    let r = rig(None);
    // Copie locale jamais synchronisée + une autre version sur le serveur.
    fs::create_dir_all(r.save_dir()).unwrap();
    fs::write(r.save_dir().join("save.dat"), "locale\n").unwrap();
    let rev1 = r.push_from_other("serveur\n", 0, 3600);
    let mut o = obs(&r);
    let out = launch::launch(&r.env(), &r.key(), &mut o).unwrap();
    let LaunchOutcome::Conflict(info) = out else { panic!("{out:?}") };
    assert_eq!((info.server.rev, info.played, info.base.is_none()), (rev1, false, true));
    assert!(!r.marker().exists(), "l'émulateur ne doit pas être lancé");
    assert_eq!(fs::read_to_string(r.save_dir().join("save.dat")).unwrap(), "locale\n", "sauvegarde locale intacte");
    assert_eq!(r.client().head(&r.key()).unwrap().unwrap().rev, rev1, "tête du serveur intacte");
    assert!(r.lease_free());
}

#[test]
fn verrou_tenu_ailleurs_erreur_sans_lancement() {
    let r = rig(None);
    let c = r.client();
    assert!(matches!(c.acquire_lease(&r.key(), "steam-deck", false).unwrap(), LeaseOutcome::Granted { .. }));
    let mut o = obs(&r);
    let err = launch::launch(&r.env(), &r.key(), &mut o).unwrap_err();
    assert_eq!(err, LaunchError::HeldElsewhere("steam-deck".into()));
    assert_eq!(err.to_string(), "En cours sur steam-deck");
    assert!(!r.marker().exists());
    assert_eq!(o.installs, 0);
    // Le verrou de l'autre appareil n'a pas été touché.
    assert!(matches!(c.acquire_lease(&r.key(), "pc", false).unwrap(), LeaseOutcome::HeldElsewhere { .. }));
}

#[test]
fn emulateur_introuvable_erreur_verrou_rendu_sauvegarde_intacte() {
    let r = rig(Some("/nonexistent/emulateur"));
    let mut o = obs(&r);
    let err = launch::launch(&r.env(), &r.key(), &mut o).unwrap_err();
    assert!(err.to_string().starts_with("Échec du lancement de l'émulateur"), "{err}");
    assert!(r.lease_free(), "verrou rendu");
    assert!(!r.save_dir().exists(), "sauvegarde locale non touchée");
    assert!(r.client().head(&r.key()).unwrap().is_none(), "rien envoyé au serveur");
}

#[test]
fn hors_ligne_on_joue_et_l_envoi_reste_en_attente() {
    let mut r = rig(None);
    // Jeu déjà installé, sauvegarde locale connue ; puis le serveur disparaît.
    let rom_dir = r.cfg.roms.as_ref().unwrap().join("switch");
    fs::create_dir_all(&rom_dir).unwrap();
    fs::write(rom_dir.join(format!("Jeu [{TITLE}][v0].nsp")), [0u8; 10]).unwrap();
    fs::create_dir_all(r.save_dir()).unwrap();
    fs::write(r.save_dir().join("save.dat"), "avant\n").unwrap();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    r.cfg.serveur = format!("http://{}", l.local_addr().unwrap());
    drop(l);
    let mut o = obs(&r);
    o.server = r.cfg.serveur.clone();
    let out = launch::launch(&r.env(), &r.key(), &mut o).unwrap();
    let LaunchOutcome::Played { before, after, .. } = out else { panic!("{out:?}") };
    assert_eq!(before, SyncOutcome::Offline { pending_upload: true });
    assert_eq!(after, SyncOutcome::Offline { pending_upload: true });
    assert_eq!(o.installs, 0);
    assert_eq!(fs::read_to_string(r.save_dir().join("save.dat")).unwrap(), "avant\njouee\n");
    let entry = Journal::open(&r.journal).unwrap().get(&r.key());
    assert!(entry.pending_upload);
}

/// Prépare un conflit : copie locale ET version serveur sans base commune.
fn conflict_rig() -> (Rig, u64) {
    let r = rig(None);
    fs::create_dir_all(r.save_dir()).unwrap();
    fs::write(r.save_dir().join("save.dat"), "locale\n").unwrap();
    let rev1 = r.push_from_other("serveur\n", 0, 7200);
    let o = launch::launch(&r.env(), &r.key(), &mut obs(&r)).unwrap();
    assert!(matches!(o, LaunchOutcome::Conflict(_)));
    (r, rev1)
}

#[test]
fn resolution_garder_la_locale() {
    let (r, rev1) = conflict_rig();
    let out = launch::resolve(&r.env(), &r.key(), Resolution::KeepLocal).unwrap();
    assert_eq!(out, SyncOutcome::Uploaded { rev: rev1 + 1 });
    assert_eq!(fs::read_to_string(r.save_dir().join("save.dat")).unwrap(), "locale\n");
    let head = r.client().head(&r.key()).unwrap().unwrap();
    assert_eq!((head.rev, head.device.as_str()), (rev1 + 1, "pc"));
    assert!(r.lease_free());
    // Le conflit est levé : le lancement suivant joue.
    let o = launch::launch(&r.env(), &r.key(), &mut obs(&r)).unwrap();
    assert!(matches!(o, LaunchOutcome::Played { .. }), "{o:?}");
}

#[test]
fn resolution_utiliser_le_serveur_archive_la_locale() {
    let (r, rev1) = conflict_rig();
    let out = launch::resolve(&r.env(), &r.key(), Resolution::KeepServer).unwrap();
    assert_eq!(out, SyncOutcome::Downloaded { rev: rev1 });
    assert_eq!(fs::read_to_string(r.save_dir().join("save.dat")).unwrap(), "serveur\n");
    let history = r.client().history(&r.key()).unwrap();
    let discarded: Vec<_> = history.iter().filter(|h| h.discarded).collect();
    assert_eq!(discarded.len(), 1, "la copie locale est archivée comme écartée : {history:?}");
    assert_eq!(discarded[0].device, "pc");
    // La tête reste la version du serveur.
    assert_eq!(history.iter().find(|h| !h.discarded).unwrap().rev, rev1);
    let blob = r.client().blob(&r.key(), discarded[0].rev).unwrap();
    let layout = ludo_client::layout::DirLayout { account: r.dir.path().join("verif"), device: None, exclude: vec![] };
    ludo_client::layout::SaveLayout::import(&layout, &blob).unwrap();
    assert_eq!(fs::read_to_string(r.dir.path().join("verif/save.dat")).unwrap(), "locale\n");
    assert!(r.lease_free());
}

#[test]
fn rom_retrouvee_sans_catalogue() {
    let t = tempfile::tempdir().unwrap();
    fs::create_dir_all(t.path().join("switch/MAJ-DLC")).unwrap();
    fs::write(t.path().join("switch/MAJ-DLC/Jeu [0100152000022800][v1].nsp"), "x").unwrap();
    fs::write(t.path().join("switch/Jeu [0100152000022000][v0].nsp"), "x").unwrap();
    let found = launch::find_rom_offline(t.path(), "0100152000022000").unwrap();
    assert!(found.ends_with("switch/Jeu [0100152000022000][v0].nsp"));
    assert!(launch::find_rom_offline(t.path(), "0100aaaaaaaaaaaa").is_none());
}
