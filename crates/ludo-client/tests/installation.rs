//! Installation de jeux : vrai `ludo-server` pour le cas nominal et la reprise, faux serveurs
//! (routeur de test) pour les cas hostiles (200 au lieu de 206, tar piégé, taille incohérente).

use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use ludo_client::api::Client;
use ludo_client::install::{uninstall, InstallError, Installed, Installer};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
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
    let inst = Installer::new(&client, e.device.path());
    let mut last = (0, 0);
    let out = inst.install(&id, &f, &mut |d, t| last = (d, t)).unwrap();
    assert_eq!((out, last), (Installed::Downloaded, (100_000, 100_000)));
    let dest = e.device.path().join(&f.path);
    assert_eq!(fs::read(&dest).unwrap(), fs::read(e.server_roms.path().join(&f.path)).unwrap());
    assert!(!dest.with_file_name(format!("{}.part", dest.file_name().unwrap().to_str().unwrap())).exists());
    // Deuxième appel : rien à faire.
    assert_eq!(inst.install(&id, &f, &mut |_, _| {}).unwrap(), Installed::AlreadyPresent);
}

/// ETag que le serveur donne au fichier (taille et date en hexadécimal, entre guillemets).
fn server_etag(e: &Env, f: &GameFile) -> String {
    let m = fs::metadata(e.server_roms.path().join(&f.path)).unwrap();
    let mtime = m.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    format!("\"{:x}-{:x}\"", m.len(), mtime)
}

#[test]
fn fichier_change_entre_deux_reprises_repart_de_zero() {
    let e = env();
    let (id, f) = base_of(&catalog(&e), "switch");
    let full = fs::read(e.server_roms.path().join(&f.path)).unwrap();
    let dest = e.device.path().join(&f.path);
    fs::create_dir_all(dest.parent().unwrap()).unwrap();
    let part = format!("{}.part", dest.display());
    // Début d'une ancienne version (octets faux) avec l'ETag de cette ancienne version.
    fs::write(&part, vec![0xAA; 40_000]).unwrap();
    fs::write(format!("{part}.etag"), "\"ancienne-version\"").unwrap();
    let client = Client::new(&e.url);
    Installer::new(&client, e.device.path()).install(&id, &f, &mut |_, _| {}).unwrap();
    assert_eq!(fs::read(&dest).unwrap(), full);
    assert!(!std::path::Path::new(&format!("{part}.etag")).exists());
}

#[test]
fn partiel_sans_validateur_n_est_pas_repris() {
    let e = env();
    let (id, f) = base_of(&catalog(&e), "switch");
    let full = fs::read(e.server_roms.path().join(&f.path)).unwrap();
    let dest = e.device.path().join(&f.path);
    fs::create_dir_all(dest.parent().unwrap()).unwrap();
    fs::write(format!("{}.part", dest.display()), vec![0xAA; 40_000]).unwrap();
    let client = Client::new(&e.url);
    let mut first = None;
    Installer::new(&client, e.device.path())
        .install(&id, &f, &mut |d, _| {
            first.get_or_insert(d);
        })
        .unwrap();
    assert_eq!(fs::read(&dest).unwrap(), full);
    assert!(first.unwrap() <= 100_000);
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
    fs::write(format!("{part}.etag"), server_etag(&e, &f)).unwrap();
    let client = Client::new(&e.url);
    let mut first = None;
    Installer::new(&client, e.device.path())
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
    put(dev.path(), "ds/x.nds.part.etag", b"\"v1\"");
    let client = Client::new(&url);
    Installer::new(&client, dev.path()).install("g", &file("ds/x.nds", 5_000, "nds"), &mut |_, _| {}).unwrap();
    assert_eq!(fs::read(dev.path().join("ds/x.nds")).unwrap(), body);
    assert_eq!(ranges.lock().unwrap().as_slice(), &[Some("bytes=2000-".to_string())]);
}

#[test]
fn flux_trop_long_est_une_erreur_et_nettoie() {
    let ranges = Arc::new(Mutex::new(Vec::new()));
    let url = fake_full_server(vec![1; 100], ranges);
    let dev = tempfile::tempdir().unwrap();
    let client = Client::new(&url);
    let err = Installer::new(&client, dev.path()).install("g", &file("ds/x.nds", 60, "nds"), &mut |_, _| {}).unwrap_err();
    assert!(matches!(err, InstallError::SizeMismatch { expected: 60, .. }), "{err}");
    assert!(err.to_string().contains("taille inattendue"));
    assert!(!dev.path().join("ds/x.nds").exists() && !dev.path().join("ds/x.nds.part").exists());
}

#[test]
fn flux_court_garde_le_partiel() {
    let ranges = Arc::new(Mutex::new(Vec::new()));
    let url = fake_full_server(vec![1; 100], ranges);
    let dev = tempfile::tempdir().unwrap();
    let client = Client::new(&url);
    let err = Installer::new(&client, dev.path()).install("g", &file("ds/x.nds", 150, "nds"), &mut |_, _| {}).unwrap_err();
    assert!(matches!(err, InstallError::Interrupted(_)), "{err}");
    assert!(!dev.path().join("ds/x.nds").exists());
    assert_eq!(fs::metadata(dev.path().join("ds/x.nds.part")).unwrap().len(), 100);
}

/// Faux serveur brut : envoie `head` puis `body`, ne dit plus rien et garde la connexion ouverte.
fn stall_server(head: &'static str, body: Vec<u8>) -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for conn in l.incoming() {
            let Ok(mut c) = conn else { break };
            let body = body.clone();
            std::thread::spawn(move || {
                let mut req = [0u8; 4096];
                let _ = c.read(&mut req);
                let _ = c.write_all(head.as_bytes());
                let _ = c.write_all(&body);
                let _ = c.flush();
                std::thread::sleep(Duration::from_secs(60));
            });
        }
    });
    format!("http://{addr}")
}

const STALL_HEAD: &str = "HTTP/1.1 200 OK\r\ncontent-length: 1000\r\netag: \"v1\"\r\n\r\n";

#[test]
fn delai_d_inactivite_interrompt_et_garde_le_partiel() {
    let url = stall_server(STALL_HEAD, vec![7; 10]);
    let dev = tempfile::tempdir().unwrap();
    let client = Client::new(&url);
    let t0 = Instant::now();
    let err = Installer::new(&client, dev.path()).with_idle(Duration::from_millis(400)).install("g", &file("ds/x.nds", 1000, "nds"), &mut |_, _| {}).unwrap_err();
    assert!(matches!(err, InstallError::Interrupted(_)), "{err}");
    assert!(t0.elapsed() < Duration::from_secs(10), "trop long : {:?}", t0.elapsed());
    assert_eq!(fs::read(dev.path().join("ds/x.nds.part")).unwrap(), vec![7; 10]);
    assert!(dev.path().join("ds/x.nds.part.etag").exists());
}

#[test]
fn annulation_garde_le_partiel_d_un_fichier() {
    let url = stall_server(STALL_HEAD, vec![7; 10]);
    let dev = tempfile::tempdir().unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let c2 = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        c2.store(true, Ordering::SeqCst);
    });
    let client = Client::new(&url);
    let t0 = Instant::now();
    let err = Installer::new(&client, dev.path()).with_cancel(cancel).install("g", &file("ds/x.nds", 1000, "nds"), &mut |_, _| {}).unwrap_err();
    assert!(matches!(err, InstallError::Cancelled), "{err}");
    assert_eq!(err.to_string(), "Installation annulée");
    assert!(t0.elapsed() < Duration::from_secs(10));
    assert!(dev.path().join("ds/x.nds.part").exists());
}

#[test]
fn annulation_supprime_le_dossier_partiel_d_un_tar() {
    // Début d'un tar valide (un fichier de 5 octets), puis silence.
    let url = stall_server(
        "HTTP/1.1 200 OK\r\ncontent-length: 100000\r\nx-ludo-archive: tar\r\n\r\n",
        raw_entry("a.bin", tar::EntryType::Regular, None, b"12345"),
    );
    let dev = tempfile::tempdir().unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let c2 = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        c2.store(true, Ordering::SeqCst);
    });
    let client = Client::new(&url);
    let err = Installer::new(&client, dev.path()).with_cancel(cancel).install("g", &file("wiiu/Jeu", 100, "dir"), &mut |_, _| {}).unwrap_err();
    assert!(matches!(err, InstallError::Cancelled), "{err}");
    assert!(!dev.path().join("wiiu/Jeu").exists() && !dev.path().join("wiiu/Jeu.part").exists());
}

#[test]
fn chemins_du_catalogue_refuses() {
    let dev = tempfile::tempdir().unwrap();
    let client = Client::new("http://127.0.0.1:1");
    let inst = Installer::new(&client, dev.path());
    for bad in ["../x.nds", "/etc/passwd", "ds/../../x", "", "switch", "switch/"] {
        let err = inst.install("g", &file(bad, 1, "nds"), &mut |_, _| {}).unwrap_err();
        assert!(matches!(err, InstallError::BadPath(_)), "{bad} : {err}");
    }
}

#[test]
fn espace_disque_insuffisant_avant_de_telecharger() {
    let dev = tempfile::tempdir().unwrap();
    // Serveur injoignable : si le contrôle d'espace ne passait pas d'abord, l'erreur serait « injoignable ».
    let client = Client::new("http://127.0.0.1:1");
    let err = Installer::new(&client, dev.path()).install("g", &file("ds/x.nds", u64::MAX / 2, "nds"), &mut |_, _| {}).unwrap_err();
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
    let inst = Installer::new(&client, e.device.path());
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
    tar_server_total(archive, None)
}

fn tar_server_total(archive: Vec<u8>, total: Option<u64>) -> String {
    spawn(axum::Router::new().route(
        "/api/v1/games/{g}/files/{f}",
        get(move || {
            let a = archive.clone();
            async move {
                let mut r = ([(header::CONTENT_TYPE, "application/x-tar"), (header::HeaderName::from_static("x-ludo-archive"), "tar")], a).into_response();
                if let Some(t) = total {
                    r.headers_mut().insert("x-ludo-total-bytes", t.to_string().parse().unwrap());
                }
                r
            }
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
        let err = Installer::new(&client, dev.path()).install("g", &file("wiiu/Jeu", 3, "dir"), &mut |_, _| {}).unwrap_err();
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
    let err = Installer::new(&client, dev.path()).install("g", &file("wiiu/Jeu", 9, "dir"), &mut |_, _| {}).unwrap_err();
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

#[test]
fn extraction_s_arrete_au_depassement_et_suit_le_total_annonce() {
    let a = archive(&[raw_entry("a.bin", tar::EntryType::Regular, None, b"12345")]);
    let dev = tempfile::tempdir().unwrap();
    // Catalogue : 3 octets, sans total annoncé : dépassement refusé.
    let client = Client::new(&tar_server(a.clone()));
    let err = Installer::new(&client, dev.path()).install("g", &file("wiiu/Jeu", 3, "dir"), &mut |_, _| {}).unwrap_err();
    assert!(matches!(err, InstallError::SizeMismatch { expected: 3, got: 5 }), "{err}");
    assert!(!dev.path().join("wiiu/Jeu").exists() && !dev.path().join("wiiu/Jeu.part").exists());
    // Même archive, total annoncé de 5 octets par le serveur : la taille du catalogue périmée est ignorée.
    let client = Client::new(&tar_server_total(a, Some(5)));
    Installer::new(&client, dev.path()).install("g", &file("wiiu/Jeu", 3, "dir"), &mut |_, _| {}).unwrap();
    assert_eq!(fs::read(dev.path().join("wiiu/Jeu/a.bin")).unwrap(), b"12345");
}

#[cfg(unix)]
#[test]
fn lien_symbolique_intermediaire_refuse_pour_installer_et_desinstaller() {
    use std::os::unix::fs::symlink;
    let body: Vec<u8> = vec![9; 50];
    let url = fake_full_server(body, Arc::new(Mutex::new(Vec::new())));
    let client = Client::new(&url);
    let dev = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("x.nds"), b"hors").unwrap();
    // Dossier de console remplacé par un lien vers l'extérieur.
    symlink(outside.path(), dev.path().join("ds")).unwrap();
    let err = Installer::new(&client, dev.path()).install("g", &file("ds/x.nds", 50, "nds"), &mut |_, _| {}).unwrap_err();
    assert!(matches!(err, InstallError::SymLink(_)), "{err}");
    assert!(err.to_string().contains("lien symbolique"));
    let err = uninstall(dev.path(), &[file("ds/x.nds", 4, "nds")]).unwrap_err();
    assert!(matches!(err, InstallError::SymLink(_)), "{err}");
    assert_eq!(fs::read(outside.path().join("x.nds")).unwrap(), b"hors");
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1, "rien n'a été écrit à l'extérieur");

    // Le .part lui-même est un lien : refusé, la cible n'est pas touchée.
    let dev2 = tempfile::tempdir().unwrap();
    fs::create_dir_all(dev2.path().join("gc")).unwrap();
    symlink(outside.path().join("x.nds"), dev2.path().join("gc/y.rvz.part")).unwrap();
    let err = Installer::new(&client, dev2.path()).install("g", &file("gc/y.rvz", 50, "rvz"), &mut |_, _| {}).unwrap_err();
    assert!(matches!(err, InstallError::SymLink(_)), "{err}");
    assert_eq!(fs::read(outside.path().join("x.nds")).unwrap(), b"hors");

    // Dossier intermédiaire (sous la console) en lien : refusé aussi pour la désinstallation.
    let dev3 = tempfile::tempdir().unwrap();
    fs::create_dir_all(dev3.path().join("switch")).unwrap();
    symlink(outside.path(), dev3.path().join("switch/MAJ-DLC")).unwrap();
    assert!(matches!(uninstall(dev3.path(), &[file("switch/MAJ-DLC/x.nds", 4, "nds")]), Err(InstallError::SymLink(_))));
    assert_eq!(fs::read(outside.path().join("x.nds")).unwrap(), b"hors");
}

#[test]
fn desinstaller_refuse_un_chemin_a_un_composant() {
    let dev = tempfile::tempdir().unwrap();
    put(dev.path(), "switch/Jeu.nsp", b"rom");
    assert!(matches!(uninstall(dev.path(), &[file("switch", 1, "dir")]), Err(InstallError::BadPath(_))));
    assert!(dev.path().join("switch/Jeu.nsp").exists());
}

// ---- Conversion côté serveur (NSZ → NSP) --------------------------------------------------

/// Faux convertisseur : attend tant que `gate` (à côté des clés) existe, puis copie le fichier en
/// ajoutant « DECOMP » sous l'extension convertie.
const FAKE_NSZ: &str = r#"#!/bin/bash
while [ $# -gt 0 ]; do case "$1" in --keys) K="$2"; shift 2;; -o) O="$2"; shift 2;; -D|--minimal-output) shift;; *) F="$1"; shift;; esac; done
D=$(dirname "$K")
echo "50% conversion"
while [ -e "$D/gate" ]; do sleep 0.05; done
b=$(basename "$F")
{ cat "$F"; printf 'DECOMP'; } > "$O/${b%.*}.nsp"
"#;

struct ConvEnv {
    url: String,
    device: tempfile::TempDir,
    gate: std::path::PathBuf,
    _keep: tempfile::TempDir,
}

fn conv_env() -> ConvEnv {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    put(tmp.path(), "roms/switch/Jeu [0100152000022000][v0].nsz", &[9u8; 20_000]);
    put(tmp.path(), "keys/prod.keys", b"fausses cles de test");
    let script = tmp.path().join("faux-nsz.sh");
    fs::write(&script, FAKE_NSZ).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let store = Store::open(&tmp.path().join("data"), Box::new(SystemClock)).unwrap().with_roms(tmp.path().join("roms"));
    store.rescan().unwrap();
    let mut cfg = ludo_server::convert::ConvertConfig::new(tmp.path().join("cache"));
    cfg.command = script.to_string_lossy().into_owned();
    cfg.keys = Some(tmp.path().join("keys/prod.keys"));
    let url = spawn_with_converter(store, cfg);
    let gate = tmp.path().join("keys/gate");
    ConvEnv { url, device: tempfile::tempdir().unwrap(), gate, _keep: tmp }
}

fn spawn_with_converter(store: Store, cfg: ludo_server::convert::ConvertConfig) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(async move {
            let conv = ludo_server::convert::Converter::new(cfg).unwrap();
            let router = ludo_server::router_with(Arc::new(store), Some(conv));
            axum::serve(tokio::net::TcpListener::from_std(listener).unwrap(), router).await.unwrap();
        });
    });
    format!("http://{addr}")
}

#[test]
fn conversion_attendue_puis_installee_en_nsp() {
    let e = conv_env();
    fs::write(&e.gate, "").unwrap();
    let client = Client::new(&e.url);
    let (_, f) = base_of(&client.games().unwrap(), "switch");
    assert_eq!(f.format, "nsz");
    let id = client.games().unwrap()[0].id.clone();
    // La conversion est retenue 600 ms, puis libérée.
    let gate = e.gate.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(600));
        let _ = fs::remove_file(gate);
    });
    let waits = Mutex::new(Vec::new());
    let installer = Installer::new(&client, e.device.path())
        .with_formats(vec!["nsp".into(), "xci".into()])
        .with_retry_cap(Duration::from_millis(100))
        .with_prepare(|p| waits.lock().unwrap().push(p.state.clone()));
    let mut last = (0, 0);
    let got = installer.install(&id, &f, &mut |d, t| last = (d, t)).unwrap();
    assert_eq!(got, Installed::Downloaded);
    assert!(!waits.lock().unwrap().is_empty(), "au moins une réponse 202 vue");
    // Le fichier est posé sous son nom converti, avec la taille du fichier converti (pas celle du catalogue).
    let nsp = e.device.path().join("switch/Jeu [0100152000022000][v0].nsp");
    let data = fs::read(&nsp).unwrap();
    assert_eq!(data.len(), 20_006);
    assert!(data.ends_with(b"DECOMP"));
    assert_ne!(f.size, data.len() as u64);
    assert_eq!(last, (20_006, 20_006));
    assert!(!e.device.path().join("switch/Jeu [0100152000022000][v0].nsz").exists());
    // Deuxième installation : déjà présent, sans nouvelle attente.
    assert_eq!(installer.install(&id, &f, &mut |_, _| {}).unwrap(), Installed::AlreadyPresent);
    // Désinstallation : retire le fichier converti.
    assert_eq!(uninstall(e.device.path(), &[f]).unwrap(), 1);
    assert!(!nsp.exists());
}

#[test]
fn format_accepte_tel_quel_sans_conversion() {
    let e = conv_env();
    let client = Client::new(&e.url);
    let (id, f) = base_of(&client.games().unwrap(), "switch");
    let installer = Installer::new(&client, e.device.path()).with_formats(vec!["nsz".into(), "nsp".into()]);
    assert_eq!(installer.install(&id, &f, &mut |_, _| {}).unwrap(), Installed::Downloaded);
    assert_eq!(fs::metadata(e.device.path().join("switch/Jeu [0100152000022000][v0].nsz")).unwrap().len(), f.size);
    assert!(!e.device.path().join("switch/Jeu [0100152000022000][v0].nsp").exists());
}

#[test]
fn annulation_pendant_la_preparation() {
    let e = conv_env();
    fs::write(&e.gate, "").unwrap();
    let client = Client::new(&e.url);
    let (id, f) = base_of(&client.games().unwrap(), "switch");
    let cancel = Arc::new(AtomicBool::new(false));
    let c2 = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        c2.store(true, Ordering::SeqCst);
    });
    let installer = Installer::new(&client, e.device.path()).with_formats(vec!["nsp".into()]).with_retry_cap(Duration::from_millis(100)).with_cancel(cancel);
    let t = Instant::now();
    assert!(matches!(installer.install(&id, &f, &mut |_, _| {}), Err(InstallError::Cancelled)));
    assert!(t.elapsed() < Duration::from_secs(5));
    assert!(!e.device.path().join("switch/Jeu [0100152000022000][v0].nsp").exists());
}

#[test]
fn preparation_sans_progres_abandonnee() {
    let e = conv_env();
    fs::write(&e.gate, "").unwrap();
    let client = Client::new(&e.url);
    let (id, f) = base_of(&client.games().unwrap(), "switch");
    let installer = Installer::new(&client, e.device.path()).with_formats(vec!["nsp".into()]).with_retry_cap(Duration::from_millis(100)).with_prepare_stall(Duration::from_millis(700));
    assert!(matches!(installer.install(&id, &f, &mut |_, _| {}), Err(InstallError::Interrupted(_))));
}

#[test]
fn chemin_converti() {
    assert_eq!(ludo_client::install::converted_rel("switch/Jeu [01][v0].nsz").as_deref(), Some("switch/Jeu [01][v0].nsp"));
    assert_eq!(ludo_client::install::converted_rel("switch/Jeu.xcz").as_deref(), Some("switch/Jeu.xci"));
    assert_eq!(ludo_client::install::converted_rel("switch/Jeu.nsp"), None);
}
