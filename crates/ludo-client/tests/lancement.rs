//! Lancement d'un jeu : vrai `ludo-server` en processus, faux émulateur (script shell) décrit par
//! une fiche de test. Chaque cas vérifie aussi l'état du verrou de partie sur le serveur.

use ludo_client::api::Client;
use ludo_client::config::{Config, GameConf};
use ludo_client::engine::{Resolution, SyncOutcome};
use ludo_client::launch::Settled;
use ludo_client::install::Installer;
use ludo_client::journal::{Entry, Journal};
use ludo_client::launch::{self, Env, LaunchError, LaunchOutcome, Observer, Step, StepState};
use ludo_proto::{CatalogGame, FileRole, LeaseOutcome, PushOutcome, PushParams, SaveKey};
use ludo_server::store::{Store, SystemClock};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const TITLE: &str = "0100152000022000";

fn serve(store: Arc<Store>, listener: std::net::TcpListener) {
    let router = ludo_server::router(store);
    listener.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(async move {
            axum::serve(tokio::net::TcpListener::from_std(listener).unwrap(), router).await.unwrap();
        });
    });
}

fn spawn(store: Arc<Store>) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    serve(store, listener);
    format!("http://{addr}")
}

struct Rig {
    url: String,
    dir: tempfile::TempDir,
    cfg: Config,
    journal: PathBuf,
    lock: Mutex<()>,
    game: String,
    store: Arc<Store>,
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
    let store = Store::open(&dir.path().join("server-data"), Box::new(SystemClock)).unwrap().with_roms(server_roms.clone());
    store.rescan().unwrap();
    let store = Arc::new(store);
    let url = spawn(store.clone());

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
        consoles: BTreeMap::new(),
        emulateurs: BTreeMap::new(),
        dossiers: BTreeMap::new(),
    };
    let journal = dir.path().join("journal.json");
    Rig { url, dir, cfg, journal, lock: Mutex::new(()), game, store }
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
    let out = launch::resolve(&r.env(), &r.key(), Resolution::KeepLocal, rev1).unwrap();
    assert_eq!(out, Settled::Done(SyncOutcome::Uploaded { rev: rev1 + 1 }));
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
    let out = launch::resolve(&r.env(), &r.key(), Resolution::KeepServer, rev1).unwrap();
    assert_eq!(out, Settled::Done(SyncOutcome::Downloaded { rev: rev1 }));
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

fn script(r: &Rig, name: &str, body: &str) -> String {
    let p = r.dir.path().join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    p.display().to_string()
}

/// Copie locale synchronisée avec la rév. 1 du serveur (même contenu), journal à jour.
fn synced_with_rev1(r: &Rig, playtime_s: u64) -> u64 {
    let rev1 = r.push_from_other("depart\n", 0, playtime_s);
    fs::create_dir_all(r.save_dir()).unwrap();
    fs::write(r.save_dir().join("save.dat"), "depart\n").unwrap();
    let layout = ludo_client::layout::DirLayout { account: r.save_dir(), device: None, exclude: vec![] };
    let hash = ludo_client::layout::SaveLayout::export(&layout).unwrap().unwrap().hash;
    Journal::open(&r.journal).unwrap().set(&r.key(), Entry { base_rev: rev1, base_hash: Some(hash), pending_upload: false, conflict: false }).unwrap();
    rev1
}

#[test]
fn hors_ligne_avant_en_ligne_apres_temps_de_jeu_tete_plus_duree() {
    let mut r = rig(None);
    let rev1 = synced_with_rev1(&r, 5000);
    let rom_dir = r.cfg.roms.as_ref().unwrap().join("switch");
    fs::create_dir_all(&rom_dir).unwrap();
    fs::write(rom_dir.join(format!("Jeu [{TITLE}][v0].nsp")), [0u8; 10]).unwrap();
    // Le serveur n'écoute pas au départ : il apparaît pendant la partie (le faux émulateur dure 1 s).
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    r.cfg.serveur = format!("http://{addr}");
    let store = r.store.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(600));
        serve(store, std::net::TcpListener::bind(addr).unwrap());
    });
    let mut o = obs(&r);
    o.server = r.cfg.serveur.clone();
    let out = launch::launch(&r.env(), &r.key(), &mut o).unwrap();
    let LaunchOutcome::Played { before, after, playtime_s, .. } = out else { panic!("{out:?}") };
    assert_eq!(before, SyncOutcome::Offline { pending_upload: false }, "avant : hors ligne");
    assert!(matches!(after, SyncOutcome::Uploaded { rev } if rev == rev1 + 1), "{after:?}");
    assert!(playtime_s >= 5001, "tête + durée, pas la durée seule : {playtime_s}");
    let head = Client::new(&r.cfg.serveur).head(&r.key()).unwrap().unwrap();
    assert_eq!(head.playtime_s, playtime_s);
}

#[test]
fn sortie_en_erreur_de_l_emulateur_signalee() {
    let r = rig(None);
    let bin = script(&r, "plante.sh", "exit 3");
    let mut r = r;
    let fiche = r.cfg.fiches.clone().unwrap().join("fake.toml");
    let text = fs::read_to_string(&fiche).unwrap();
    let old = text.lines().find(|l| l.starts_with("bin = ")).unwrap().to_string();
    fs::write(&fiche, text.replace(&old, &format!("bin = \"{bin}\""))).unwrap();
    r.cfg.serveur = r.url.clone();
    let out = launch::launch(&r.env(), &r.key(), &mut obs(&r)).unwrap();
    let LaunchOutcome::Played { exit_ok, .. } = out else { panic!("{out:?}") };
    assert!(!exit_ok);
    assert!(r.lease_free());
}

#[test]
fn lancement_refuse_si_ludo_run_tient_le_verrou_sur_le_meme_appareil() {
    let r = rig(None);
    // `ludo run` prend le verrou sous l'identifiant de l'appareil lui-même.
    assert!(matches!(r.client().acquire_lease(&r.key(), "pc", false).unwrap(), LeaseOutcome::Granted { .. }));
    let err = launch::launch(&r.env(), &r.key(), &mut obs(&r)).unwrap_err();
    assert_eq!(err, LaunchError::HeldElsewhere("pc".into()));
    assert!(!r.marker().exists());
    assert_eq!(launch::holder_device("pc-app"), "pc");
    assert_eq!(launch::holder_device("pc-ui"), "pc");
    // Et l'inverse : la partie de l'app est vue « ailleurs » par `ludo run`.
    r.client().release_lease(&r.key(), "pc").unwrap();
    assert!(matches!(r.client().acquire_lease(&r.key(), &launch::app_device("pc"), false).unwrap(), LeaseOutcome::Granted { .. }));
    assert!(matches!(r.client().acquire_lease(&r.key(), "pc", false).unwrap(), LeaseOutcome::HeldElsewhere { .. }));
}

#[test]
fn resolution_avec_revision_perimee_refusee() {
    let (r, rev1) = conflict_rig();
    // Un autre appareil a poussé entre l'affichage du conflit et le choix.
    let rev2 = r.push_from_other("encore plus recent\n", rev1, 9000);
    let out = launch::resolve(&r.env(), &r.key(), Resolution::KeepLocal, rev1).unwrap();
    let Settled::Conflict(info) = out else { panic!("{out:?}") };
    assert_eq!(info.server.rev, rev2, "le conflit réaffiché porte la version à jour");
    assert_eq!(fs::read_to_string(r.save_dir().join("save.dat")).unwrap(), "locale\n");
    assert_eq!(r.client().head(&r.key()).unwrap().unwrap().rev, rev2, "tête intacte");
    assert!(r.lease_free());
}

struct Panique;
impl Observer for Panique {
    fn step(&mut self, step: Step, state: StepState, _: Option<String>) {
        if step == Step::Sauvegarde && state == StepState::EnCours {
            panic!("panne simulée");
        }
    }
    fn install(&mut self, _: &CatalogGame) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn panique_pendant_la_partie_verrou_rendu_sans_blocage() {
    let r = rig(None);
    let rom_dir = r.cfg.roms.as_ref().unwrap().join("switch");
    fs::create_dir_all(&rom_dir).unwrap();
    fs::write(rom_dir.join(format!("Jeu [{TITLE}][v0].nsp")), [0u8; 10]).unwrap();
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| launch::launch(&r.env(), &r.key(), &mut Panique)));
    assert!(res.is_err());
    assert!(r.lease_free(), "verrou rendu malgré la panique");
}

#[test]
fn renvoi_de_la_sauvegarde_sans_relancer_le_jeu() {
    let r = rig(None);
    let rev1 = synced_with_rev1(&r, 5000);
    fs::write(r.save_dir().join("save.dat"), "depart\njouee\n").unwrap();
    // Temps de jeu mémorisé plus bas que la tête : jamais un temps inférieur à la tête.
    let out = launch::send(&r.env(), &r.key(), 100).unwrap();
    assert_eq!(out, Settled::Done(SyncOutcome::Uploaded { rev: rev1 + 1 }));
    assert!(!r.marker().exists(), "le jeu n'est pas relancé");
    assert_eq!(r.client().head(&r.key()).unwrap().unwrap().playtime_s, 5000);
    assert!(r.lease_free());
}

#[test]
fn rom_hors_ligne_choix_deterministe() {
    let t = tempfile::tempdir().unwrap();
    fs::create_dir_all(t.path().join("switch")).unwrap();
    for n in ["b [0100152000022000].xci", "z [0100152000022000].nsp", "a [0100152000022000].nsp"] {
        fs::write(t.path().join("switch").join(n), "x").unwrap();
    }
    assert!(launch::find_rom_offline(t.path(), "0100152000022000").unwrap().ends_with("switch/a [0100152000022000].nsp"));
}

#[test]
fn jeu_sans_section_jeux_lance_avec_l_emulateur_par_defaut_de_la_console() {
    let mut r = rig(None);
    r.cfg.jeux.clear();
    // Aucun réglage : défaut intégré (Ryujinx pour la Switch) ; ici le profil n'a pas d'identifiant
    // Ryujinx, donc erreur claire avant toute action, rien de lancé.
    let mut o = obs(&r);
    let err = launch::launch(&r.env(), &r.key(), &mut o).unwrap_err();
    assert!(err.to_string().contains("ryujinx_user_id"), "{err}");
    assert!(!r.marker().exists());
    assert!(r.lease_free());

    // Émulateur par défaut déclaré : title_id lu dans le catalogue du serveur, partie jouée.
    r.cfg.consoles.insert("switch".into(), "fake".into());
    let mut o = obs(&r);
    let out = launch::launch(&r.env(), &r.key(), &mut o).unwrap();
    assert!(matches!(out, LaunchOutcome::Played { exit_ok: true, .. }), "{out:?}");
    assert!(r.marker().exists(), "émulateur lancé");
    assert!(r.save_dir().join("save.dat").exists(), "sauvegarde rangée sous le title_id du catalogue");
}
