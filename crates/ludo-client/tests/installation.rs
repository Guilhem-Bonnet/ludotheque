//! Installation de jeux : vrai `ludo-server` pour le cas nominal et la reprise, faux serveurs
//! (routeur de test) pour les cas hostiles (200 au lieu de 206, tar piégé, taille incohérente).

use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use ludo_client::api::Client;
use ludo_client::install::{uninstall, InstallError, Installed, Installer};
use ludo_proto::{CatalogGame, FileRole, GameFile};
use ludo_server::store::{Store, SystemClock};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

fn spawn(router: axum::Router) -> String {
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

fn put(root: &Path, rel: &str, content: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, content).unwrap();
}

struct Env {
    url: String,
    server_roms: tempfile::TempDir,
    device: tempfile::TempDir,
    _data: tempfile::TempDir,
}

fn env() -> Env {
    let server_roms = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let content: Vec<u8> = (0..=255u8).cycle().take(100_000).collect();
    put(server_roms.path(), "switch/Jeu [0100152000022000][v0].nsp", &content);
    put(server_roms.path(), "switch/MAJ-DLC/Jeu [0100152000022800][v1].nsp", &[7; 500]);
    put(server_roms.path(), "wiiu/Wind Waker/code/app.rpx", &[11; 70]);
    put(server_roms.path(), "wiiu/Wind Waker/content/a.bin", &[12; 30]);
    let store = Store::open(data.path(), Box::new(SystemClock)).unwrap().with_roms(server_roms.path().to_path_buf());
    store.rescan().unwrap();
    let url = spawn(ludo_server::router(Arc::new(store)));
    Env { url, server_roms, device: tempfile::tempdir().unwrap(), _data: data }
}

fn catalog(e: &Env) -> Vec<CatalogGame> {
    Client::new(&e.url).games().unwrap()
}

fn base_of(games: &[CatalogGame], console: &str) -> (String, GameFile) {
    let g = games.iter().find(|g| g.console == console).unwrap();
    (g.id.clone(), g.files.iter().find(|f| f.role == FileRole::Base).unwrap().clone())
}

#[test]
fn installation_complete_puis_deja_present() {
    let e = env();
    let (id, f) = base_of(&catalog(&e), "switch");
    let client = Client::new(&e.url);
    let inst = Installer { client: &client, roms: e.device.path() };
    let mut last = (0, 0);
    let out = inst.install(&id, &f, &mut |d, t| last = (d, t)).unwrap();
    assert_eq!((out, last), (Installed::Downloaded, (100_000, 100_000)));
    let dest = e.device.path().join(&f.path);
    assert_eq!(fs::read(&dest).unwrap(), fs::read(e.server_roms.path().join(&f.path)).unwrap());
    assert!(!dest.with_file_name(format!("{}.part", dest.file_name().unwrap().to_str().unwrap())).exists());
    // Deuxième appel : rien à faire.
    assert_eq!(inst.install(&id, &f, &mut |_, _| {}).unwrap(), Installed::AlreadyPresent);
}

#[test]
fn reprise_depuis_un_fichier_partiel() {
    let e = env();
    let (id, f) = base_of(&catalog(&e), "switch");
    let full = fs::read(e.server_roms.path().join(&f.path)).unwrap();
    let dest = e.device.path().join(&f.path);
    fs::create_dir_all(dest.parent().unwrap()).unwrap();
    let part = format!("{}.part", dest.display());
    fs::write(&part, &full[..40_000]).unwrap();
    let client = Client::new(&e.url);
    let mut first = None;
    Installer { client: &client, roms: e.device.path() }
        .install(&id, &f, &mut |d, _| {
            first.get_or_insert(d);
        })
        .unwrap();
    assert_eq!(fs::read(&dest).unwrap(), full);
    // La progression part de l'octet repris, pas de zéro (premier appel après 40 000 octets).
    assert!(first.unwrap() > 40_000);
}

/// Faux serveur : `Range` ignoré (toujours 200 et fichier entier), et trace des en-têtes reçus.
fn fake_full_server(body: Vec<u8>, ranges: Arc<Mutex<Vec<Option<String>>>>) -> String {
    spawn(axum::Router::new().route(
        "/api/v1/games/{g}/files/{f}",
        get(move |h: HeaderMap| {
            let body = body.clone();
            let ranges = ranges.clone();
            async move {
                ranges.lock().unwrap().push(h.get(header::RANGE).map(|v| v.to_str().unwrap().to_string()));
                (StatusCode::OK, body)
            }
        }),
    ))
}

fn file(path: &str, size: u64, format: &str) -> GameFile {
    GameFile { id: 1, role: FileRole::Base, path: path.into(), format: format.into(), size, mtime: 0 }
}

#[test]
fn serveur_qui_repond_200_repart_de_zero() {
    let body: Vec<u8> = (0..=255u8).cycle().take(5_000).collect();
    let ranges = Arc::new(Mutex::new(Vec::new()));
    let url = fake_full_server(body.clone(), ranges.clone());
    let dev = tempfile::tempdir().unwrap();
    // Partiel corrompu (mauvais octets) : s'il était conservé, le résultat serait faux.
    put(dev.path(), "ds/x.nds.part", &[0xAA; 2_000]);
    let client = Client::new(&url);
    Installer { client: &client, roms: dev.path() }.install("g", &file("ds/x.nds", 5_000, "nds"), &mut |_, _| {}).unwrap();
    assert_eq!(fs::read(dev.path().join("ds/x.nds")).unwrap(), body);
    assert_eq!(ranges.lock().unwrap().as_slice(), &[Some("bytes=2000-".to_string())]);
}

#[test]
fn taille_differente_du_catalogue_est_une_erreur_et_nettoie() {
    let ranges = Arc::new(Mutex::new(Vec::new()));
    let url = fake_full_server(vec![1; 100], ranges);
    let dev = tempfile::tempdir().unwrap();
    let client = Client::new(&url);
    let err = Installer { client: &client, roms: dev.path() }.install("g", &file("ds/x.nds", 150, "nds"), &mut |_, _| {}).unwrap_err();
    assert!(matches!(err, InstallError::SizeMismatch { expected: 150, got: 100 }), "{err}");
    assert!(err.to_string().contains("taille inattendue"));
    assert!(!dev.path().join("ds/x.nds").exists() && !dev.path().join("ds/x.nds.part").exists());
}

#[test]
fn chemins_du_catalogue_refuses() {
    let dev = tempfile::tempdir().unwrap();
    let client = Client::new("http://127.0.0.1:1");
    let inst = Installer { client: &client, roms: dev.path() };
    for bad in ["../x.nds", "/etc/passwd", "ds/../../x", ""] {
        let err = inst.install("g", &file(bad, 1, "nds"), &mut |_, _| {}).unwrap_err();
        assert!(matches!(err, InstallError::BadPath(_)), "{bad} : {err}");
    }
}

#[test]
fn espace_disque_insuffisant_avant_de_telecharger() {
    let dev = tempfile::tempdir().unwrap();
    // Serveur injoignable : si le contrôle d'espace ne passait pas d'abord, l'erreur serait « injoignable ».
    let client = Client::new("http://127.0.0.1:1");
    let err = Installer { client: &client, roms: dev.path() }.install("g", &file("ds/x.nds", u64::MAX / 2, "nds"), &mut |_, _| {}).unwrap_err();
    if ludo_client::install::free_space(dev.path()).is_some() {
        assert!(matches!(err, InstallError::NoSpace { .. }), "{err}");
        assert!(err.to_string().contains("espace disque insuffisant"));
    }
}

#[test]
fn dossier_installe_par_tar_depuis_le_vrai_serveur() {
    let e = env();
    let (id, f) = base_of(&catalog(&e), "wiiu");
    assert_eq!(f.format, "dir");
    let client = Client::new(&e.url);
    let inst = Installer { client: &client, roms: e.device.path() };
    let mut last = (0, 0);
    assert_eq!(inst.install(&id, &f, &mut |d, t| last = (d, t)).unwrap(), Installed::Downloaded);
    assert_eq!(last, (100, 100));
    let dest = e.device.path().join(&f.path);
    assert_eq!(fs::read(dest.join("code/app.rpx")).unwrap(), vec![11; 70]);
    assert_eq!(fs::read(dest.join("content/a.bin")).unwrap(), vec![12; 30]);
    assert!(!e.device.path().join("wiiu/Wind Waker.part").exists());
    assert_eq!(inst.install(&id, &f, &mut |_, _| {}).unwrap(), Installed::AlreadyPresent);
    // Réinstallation après altération : remplacée proprement, sans reste.
    fs::write(dest.join("code/app.rpx"), b"abc").unwrap();
    assert_eq!(inst.install(&id, &f, &mut |_, _| {}).unwrap(), Installed::Downloaded);
    assert_eq!(fs::read(dest.join("code/app.rpx")).unwrap(), vec![11; 70]);
    assert!(!e.device.path().join("wiiu/Wind Waker.old").exists());
}

fn tar_server(archive: Vec<u8>) -> String {
    spawn(axum::Router::new().route(
        "/api/v1/games/{g}/files/{f}",
        get(move || {
            let a = archive.clone();
            async move { ([(header::CONTENT_TYPE, "application/x-tar"), (header::HeaderName::from_static("x-ludo-archive"), "tar")], a).into_response() }
        }),
    ))
}

/// Entrée de tar au nom arbitraire (le constructeur de `tar` refuse « .. » : en-tête écrit à la main).
fn raw_entry(name: &str, kind: tar::EntryType, link: Option<&str>, data: &[u8]) -> Vec<u8> {
    let mut h = tar::Header::new_gnu();
    h.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
    if let Some(l) = link {
        h.as_old_mut().linkname[..l.len()].copy_from_slice(l.as_bytes());
    }
    h.set_entry_type(kind);
    h.set_size(data.len() as u64);
    h.set_mode(0o644);
    h.set_cksum();
    let mut out = h.as_bytes().to_vec();
    out.extend_from_slice(data);
    out.resize(out.len().div_ceil(512) * 512, 0);
    out
}

fn archive(entries: &[Vec<u8>]) -> Vec<u8> {
    let mut v: Vec<u8> = entries.concat();
    v.extend_from_slice(&[0u8; 1024]);
    v
}

#[test]
fn extraction_refuse_evasions_et_liens() {
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("parent", raw_entry("../evil.bin", tar::EntryType::Regular, None, b"x")),
        ("parent profond", raw_entry("a/../../evil.bin", tar::EntryType::Regular, None, b"x")),
        ("absolu", raw_entry("/tmp/evil.bin", tar::EntryType::Regular, None, b"x")),
        ("lien symbolique", raw_entry("lien", tar::EntryType::Symlink, Some("/etc/passwd"), b"")),
        ("lien dur", raw_entry("dur", tar::EntryType::Link, Some("autre"), b"")),
    ];
    for (name, entry) in cases {
        let url = tar_server(archive(&[raw_entry("ok.bin", tar::EntryType::Regular, None, b"ok"), entry]));
        let dev = tempfile::tempdir().unwrap();
        let client = Client::new(&url);
        let err = Installer { client: &client, roms: dev.path() }.install("g", &file("wiiu/Jeu", 3, "dir"), &mut |_, _| {}).unwrap_err();
        assert!(matches!(err, InstallError::Archive(_)), "{name} : {err}");
        // Rien de posé, ni dans le dossier cible ni à côté (même relatif au dossier partiel).
        assert!(!dev.path().join("wiiu/Jeu").exists() && !dev.path().join("wiiu/Jeu.part").exists(), "{name}");
        assert!(!dev.path().join("wiiu/evil.bin").exists() && !dev.path().join("evil.bin").exists(), "{name}");
    }
}

#[test]
fn tar_de_taille_differente_refuse() {
    let url = tar_server(archive(&[raw_entry("a.bin", tar::EntryType::Regular, None, b"12345")]));
    let dev = tempfile::tempdir().unwrap();
    let client = Client::new(&url);
    let err = Installer { client: &client, roms: dev.path() }.install("g", &file("wiiu/Jeu", 9, "dir"), &mut |_, _| {}).unwrap_err();
    assert!(matches!(err, InstallError::SizeMismatch { expected: 9, got: 5 }), "{err}");
    assert!(!dev.path().join("wiiu/Jeu").exists() && !dev.path().join("wiiu/Jeu.part").exists());
}

#[test]
fn desinstallation_ne_touche_que_les_roms_du_jeu() {
    let dev = tempfile::tempdir().unwrap();
    let r = dev.path();
    put(r, "switch/Jeu.nsp", b"rom");
    put(r, "switch/MAJ-DLC/Jeu-maj.nsp", b"maj");
    put(r, "switch/MAJ-DLC/Autre.nsp", b"autre jeu");
    put(r, "switch/Jeu2.nsp.part", b"partiel");
    put(r, "wiiu/Wind Waker/code/app.rpx", b"x");
    put(r, "saves/jeu/sauvegarde.dat", b"SAUVEGARDE");
    let files = [
        file("switch/Jeu2.nsp", 1, "nsp"),
        file("switch/Jeu.nsp", 3, "nsp"),
        file("switch/MAJ-DLC/Jeu-maj.nsp", 3, "nsp"),
        file("wiiu/Wind Waker", 1, "dir"),
        file("switch/absent.nsp", 1, "nsp"),
    ];
    assert_eq!(uninstall(r, &files).unwrap(), 4);
    assert!(!r.join("switch/Jeu.nsp").exists() && !r.join("switch/Jeu2.nsp.part").exists() && !r.join("wiiu/Wind Waker").exists());
    assert!(r.join("switch/MAJ-DLC/Autre.nsp").exists(), "le dossier MAJ-DLC non vide reste");
    assert!(r.join("wiiu").exists(), "le dossier de console reste");
    assert_eq!(fs::read(r.join("saves/jeu/sauvegarde.dat")).unwrap(), b"SAUVEGARDE");
    assert!(uninstall(r, &[file("../saves/jeu", 1, "dir")]).is_err());
    assert!(uninstall(r, &[]).is_ok());
}
