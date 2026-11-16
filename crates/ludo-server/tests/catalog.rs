//! Tests du catalogue : balayage (fichiers factices + catalogue.toml), refus des évasions de chemin,
//! téléchargement avec reprise (Range), résumé des sauvegardes d'un profil.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use ludo_proto::{CatalogGame, FileRole, PushParams, RescanReport, SaveKey, SaveSummary};
use ludo_server::store::{Clock, Store};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

struct TestClock(Arc<AtomicU64>);
impl Clock for TestClock {
    fn now(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct App {
    router: axum::Router,
    store: Arc<Store>,
    roms: tempfile::TempDir,
    time: Arc<AtomicU64>,
    _data: tempfile::TempDir,
}

fn put(root: &Path, rel: &str, content: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

const CATALOGUE: &str = r##"
[[game]]
id = "switch-lm3"
console = "switch"
title = "Luigi's Mansion 3"
short = "Luigi's\nMansion 3"
title_id = "0100dca0064a6000"
entry = "switch/Luigis Mansion 3.nsp"
art = { motif = "manoir", from = "#22e08f", to = "#0a5a78" }

[[game]]
id = "gc-fantome"
console = "gc"
title = "Jeu absent"
entry = "gc/absent.rvz"
"##;

/// Racine de ROM factice : un jeu décrit, des jeux non décrits, mises à jour, DLC, dossiers à ignorer.
fn app() -> App {
    let roms = tempfile::tempdir().unwrap();
    let r = roms.path();
    put(r, "catalogue.toml", CATALOGUE.as_bytes());
    put(r, "switch/Luigis Mansion 3.nsp", &[1; 100]);
    put(r, "switch/Mario Kart 8 Deluxe [0100152000022000][v0].xci", &[2; 200]);
    put(r, "switch/MAJ-DLC/Luigis Mansion 3 [0100DCA0064A6800][v1].nsp", &[3; 30]);
    put(r, "switch/MAJ-DLC/Luigis Mansion 3 [0100DCA0064A7001][v2].nsp", &[4; 40]);
    put(r, "switch/MAJ-DLC/Mario Kart 8 Deluxe [0100152000022800][v3].nsz", &[5; 50]);
    put(r, "switch/MAJ-DLC/Orphelin.nsp", &[6; 5]);
    put(r, "switch/Zip jeux/Ignore.nsp", &[7; 5]);
    put(r, "switch/notes.txt", b"pas une rom");
    put(r, "ds/Pokémon Diamant (France).nds", &[8; 64]);
    put(r, "3ds/Majora_s Mask.cxi", &[9; 10]);
    put(r, "gc/Twilight Princess.rvz", &[10; 20]);
    put(r, "wiiu/Wind Waker HD/code/app.rpx", &[11; 70]);
    put(r, "wiiu/Wind Waker HD/content/a.bin", &[12; 30]);
    put(r, "wiiu/Zip-vieux/x.bin", &[13; 3]);
    put(r, "switch2/Pokopia.nup", &[14; 300]);
    let data = tempfile::tempdir().unwrap();
    let time = Arc::new(AtomicU64::new(1_000));
    let store = Store::open(data.path(), Box::new(TestClock(time.clone()))).unwrap().with_roms(r.to_path_buf());
    let store = Arc::new(store);
    App { router: ludo_server::router(store.clone()), store, roms, time, _data: data }
}

async fn call(app: &App, req: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let resp = app.router.clone().oneshot(req).await.unwrap();
    let (status, headers) = (resp.status(), resp.headers().clone());
    (status, headers, resp.into_body().collect().await.unwrap().to_bytes().to_vec())
}

async fn catalog(app: &App) -> Vec<CatalogGame> {
    let (s, _, b) = call(app, Request::get("/api/v1/games").body(Body::empty()).unwrap()).await;
    assert_eq!(s, StatusCode::OK);
    serde_json::from_slice(&b).unwrap()
}

fn find<'a>(games: &'a [CatalogGame], id: &str) -> &'a CatalogGame {
    games.iter().find(|g| g.id == id).unwrap_or_else(|| panic!("jeu {id} absent : {:?}", games.iter().map(|g| &g.id).collect::<Vec<_>>()))
}

#[tokio::test]
async fn balayage_catalogue_et_jeux_non_decrits() {
    let a = app();
    let (s, _, b) = call(&a, Request::post("/api/v1/catalog/rescan").body(Body::empty()).unwrap()).await;
    assert_eq!(s, StatusCode::OK);
    let report: RescanReport = serde_json::from_slice(&b).unwrap();
    let games = catalog(&a).await;
    assert_eq!(report.games, games.len());
    assert_eq!(report.files, games.iter().map(|g| g.files.len()).sum::<usize>());

    // Jeu décrit : titre, titre court (retour à la ligne), jaquette du catalogue, mise à jour et DLC rattachés.
    let lm3 = find(&games, "switch-lm3");
    assert!(lm3.catalogued);
    assert_eq!((lm3.title.as_str(), lm3.short.as_str()), ("Luigi's Mansion 3", "Luigi's\nMansion 3"));
    assert_eq!(lm3.title_id.as_deref(), Some("0100dca0064a6000"));
    assert_eq!((lm3.art.motif.as_str(), lm3.art.from.as_str()), ("manoir", "#22e08f"));
    let roles: Vec<_> = lm3.files.iter().map(|f| (f.role, f.path.as_str(), f.size)).collect();
    assert_eq!(
        roles,
        vec![
            (FileRole::Base, "switch/Luigis Mansion 3.nsp", 100),
            (FileRole::Dlc, "switch/MAJ-DLC/Luigis Mansion 3 [0100DCA0064A7001][v2].nsp", 40),
            (FileRole::Maj, "switch/MAJ-DLC/Luigis Mansion 3 [0100DCA0064A6800][v1].nsp", 30),
        ]
    );

    // Jeu non décrit : identifiant par titre, titre tiré du nom, jaquette de repli, mise à jour .nsz rattachée.
    let mk8 = find(&games, "switch-0100152000022000");
    assert!(!mk8.catalogued);
    assert_eq!(mk8.title, "Mario Kart 8 Deluxe");
    assert!(ludo_proto::MOTIFS.contains(&mk8.art.motif.as_str()));
    assert_eq!(mk8.files.len(), 2);
    assert!(mk8.files.iter().any(|f| f.role == FileRole::Maj && f.format == "nsz"));

    // Autres consoles et formats.
    assert_eq!(find(&games, "ds-pokemon-diamant").title, "Pokémon Diamant");
    assert_eq!(find(&games, "3ds-majora-s-mask").files[0].format, "cxi");
    assert_eq!(find(&games, "gc-twilight-princess").files[0].format, "rvz");
    assert_eq!(find(&games, "switch2-pokopia").files[0].format, "nup");
    let wiiu = find(&games, "wiiu-wind-waker-hd");
    assert_eq!((wiiu.files[0].format.as_str(), wiiu.files[0].size), ("dir", 100));

    // Ignorés : dossiers Zip*, fichiers non ROM, entrée du catalogue absente du disque, MAJ-DLC orphelin.
    assert_eq!(games.len(), 7, "{:?}", games.iter().map(|g| &g.id).collect::<Vec<_>>());
    assert!(games.iter().all(|g| g.id != "gc-fantome"));
    assert!(games.iter().flat_map(|g| &g.files).all(|f| !f.path.contains("Zip") && !f.path.contains("Orphelin") && !f.path.ends_with(".txt")));
}

#[tokio::test]
async fn jaquette_de_repli_stable_et_ids_de_fichiers_stables() {
    let a = app();
    a.store.rescan().unwrap();
    let first = catalog(&a).await;
    // Nouveau fichier : les identifiants de fichiers existants et les jaquettes ne bougent pas.
    put(a.roms.path(), "ds/Autre jeu.nds", &[1; 4]);
    a.store.rescan().unwrap();
    let second = catalog(&a).await;
    assert_eq!(second.len(), first.len() + 1);
    for g in &first {
        assert_eq!(find(&second, &g.id).files, g.files);
        assert_eq!(find(&second, &g.id).art, g.art);
    }
    // Fichier supprimé : le jeu disparaît.
    std::fs::remove_file(a.roms.path().join("ds/Autre jeu.nds")).unwrap();
    a.store.rescan().unwrap();
    assert_eq!(catalog(&a).await.len(), first.len());
}

#[tokio::test]
async fn catalogue_toml_illisible_refuse_le_balayage() {
    let a = app();
    a.store.rescan().unwrap();
    put(a.roms.path(), "catalogue.toml", b"[[game]\nid = ");
    let (s, _, _) = call(&a, Request::post("/api/v1/catalog/rescan").body(Body::empty()).unwrap()).await;
    assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR);
    // L'ancien catalogue reste servi.
    assert_eq!(catalog(&a).await.len(), 7);
}

#[tokio::test]
async fn sans_dossier_de_roms_catalogue_vide_et_balayage_indisponible() {
    let data = tempfile::tempdir().unwrap();
    let time = Arc::new(AtomicU64::new(1));
    let store = Arc::new(Store::open(data.path(), Box::new(TestClock(time.clone()))).unwrap());
    let a = App { router: ludo_server::router(store.clone()), store, roms: tempfile::tempdir().unwrap(), time, _data: data };
    assert!(catalog(&a).await.is_empty());
    let (s, _, _) = call(&a, Request::post("/api/v1/catalog/rescan").body(Body::empty()).unwrap()).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
}

async fn lm3_base(a: &App) -> (String, u64) {
    a.store.rescan().unwrap();
    let games = catalog(a).await;
    let f = &find(&games, "switch-lm3").files[0];
    (format!("/api/v1/games/switch-lm3/files/{}", f.id), f.size)
}

#[tokio::test]
async fn telechargement_complet_et_par_intervalle() {
    let a = app();
    // Contenu reconnaissable pour vérifier les octets servis.
    let content: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
    put(a.roms.path(), "switch/Luigis Mansion 3.nsp", &content);
    let (uri, _) = lm3_base(&a).await;

    let (s, h, b) = call(&a, Request::get(&uri).body(Body::empty()).unwrap()).await;
    assert_eq!((s, b.as_slice()), (StatusCode::OK, content.as_slice()));
    assert_eq!(h[header::ACCEPT_RANGES], "bytes");
    assert_eq!(h[header::CONTENT_LENGTH], "1000");

    let range = |v: &str| Request::get(&uri).header(header::RANGE, v).body(Body::empty()).unwrap();
    let (s, h, b) = call(&a, range("bytes=100-199")).await;
    assert_eq!(s, StatusCode::PARTIAL_CONTENT);
    assert_eq!(h[header::CONTENT_RANGE], "bytes 100-199/1000");
    assert_eq!(h[header::CONTENT_LENGTH], "100");
    assert_eq!(b, content[100..200]);

    // Reprise : de l'offset à la fin ; derniers octets ; fin au-delà de la taille bornée.
    let (s, h, b) = call(&a, range("bytes=900-")).await;
    assert_eq!((s, h[header::CONTENT_RANGE].to_str().unwrap()), (StatusCode::PARTIAL_CONTENT, "bytes 900-999/1000"));
    assert_eq!(b, content[900..]);
    let (_, h, b) = call(&a, range("bytes=-10")).await;
    assert_eq!((h[header::CONTENT_RANGE].to_str().unwrap(), b), ("bytes 990-999/1000", content[990..].to_vec()));
    let (_, h, _) = call(&a, range("bytes=990-5000")).await;
    assert_eq!(h[header::CONTENT_RANGE], "bytes 990-999/1000");

    // Hors fichier : 416. If-Range périmé : fichier entier.
    let (s, h, _) = call(&a, range("bytes=1000-")).await;
    assert_eq!((s, h[header::CONTENT_RANGE].to_str().unwrap()), (StatusCode::RANGE_NOT_SATISFIABLE, "bytes */1000"));
    let req = Request::get(&uri).header(header::RANGE, "bytes=0-9").header(header::IF_RANGE, "\"autre\"").body(Body::empty()).unwrap();
    let (s, _, b) = call(&a, req).await;
    assert_eq!((s, b.len()), (StatusCode::OK, 1000));
    let (_, h0, _) = call(&a, Request::get(&uri).body(Body::empty()).unwrap()).await;
    let req = Request::get(&uri).header(header::RANGE, "bytes=0-9").header(header::IF_RANGE, h0[header::ETAG].clone()).body(Body::empty()).unwrap();
    assert_eq!(call(&a, req).await.0, StatusCode::PARTIAL_CONTENT);
}

#[tokio::test]
async fn telechargement_refuse_les_evasions_et_les_cas_invalides() {
    let a = app();
    let (uri, _) = lm3_base(&a).await;
    // Après le balayage, le fichier est remplacé par un lien vers un fichier hors de la racine.
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    let target = a.roms.path().join("switch/Luigis Mansion 3.nsp");
    std::fs::remove_file(&target).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.path().join("secret.txt"), &target).unwrap();
        let (s, _, b) = call(&a, Request::get(&uri).body(Body::empty()).unwrap()).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        assert!(!String::from_utf8_lossy(&b).contains("secret\n"));
    }
    // Fichier disparu : 404. Fichier inconnu, jeu inconnu : 404.
    std::fs::remove_file(&target).ok();
    assert_eq!(call(&a, Request::get(&uri).body(Body::empty()).unwrap()).await.0, StatusCode::NOT_FOUND);
    assert_eq!(call(&a, Request::get("/api/v1/games/switch-lm3/files/99999").body(Body::empty()).unwrap()).await.0, StatusCode::NOT_FOUND);
    assert_eq!(call(&a, Request::get("/api/v1/games/inconnu/files/1").body(Body::empty()).unwrap()).await.0, StatusCode::NOT_FOUND);
    let games = catalog(&a).await;
    // Un fichier d'un autre jeu n'est pas servi sous l'identifiant d'un jeu étranger.
    let lm3 = &find(&games, "switch-lm3").files[0];
    let uri = format!("/api/v1/games/ds-pokemon-diamant/files/{}", lm3.id);
    assert_eq!(call(&a, Request::get(uri).body(Body::empty()).unwrap()).await.0, StatusCode::NOT_FOUND);
}

#[test]
fn safe_join_refuse_les_chemins_sortants() {
    use ludo_server::store::safe_join;
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "switch/a.nsp", b"x");
    assert!(safe_join(root.path(), "switch/a.nsp").is_ok());
    for bad in ["../secret", "switch/../../secret", "/etc/passwd", ""] {
        assert!(
            matches!(safe_join(root.path(), bad), Err(ludo_server::store::StoreError::Forbidden(_))),
            "{bad} aurait dû être refusé"
        );
    }
}

#[test]
fn intervalles_http() {
    use ludo_server::{parse_range, RangeSpec::*};
    assert_eq!(parse_range(None, 10), Full);
    assert_eq!(parse_range(Some("bytes=2-4"), 10), Part(2, 4));
    assert_eq!(parse_range(Some("bytes=5-"), 10), Part(5, 9));
    assert_eq!(parse_range(Some("bytes=-3"), 10), Part(7, 9));
    assert_eq!(parse_range(Some("bytes=-30"), 10), Part(0, 9));
    assert_eq!(parse_range(Some("bytes=10-12"), 10), Unsatisfiable);
    assert_eq!(parse_range(Some("bytes=0-1,4-5"), 10), Full);
    assert_eq!(parse_range(Some("items=0-1"), 10), Full);
    assert_eq!(parse_range(Some("bytes=4-2"), 10), Full);
    assert_eq!(parse_range(Some("bytes=0-0"), 0), Unsatisfiable);
    // Corrections de revue : suffixe nul et débordement insatisfiables, préfixe sans casse, « +5 » refusé.
    assert_eq!(parse_range(Some("bytes=-0"), 10), Unsatisfiable);
    assert_eq!(parse_range(Some("bytes=99999999999999999999-"), 10), Unsatisfiable);
    assert_eq!(parse_range(Some("BYTES=1-2"), 10), Part(1, 2));
    assert_eq!(parse_range(Some("bytes=+5-"), 10), Full);
}

#[tokio::test]
async fn partage_non_monte_ou_vide_ne_vide_pas_le_catalogue() {
    let a = app();
    let first = a.store.rescan().unwrap();
    assert!(first.games > 0);
    // Les éléments écartés sont listés avec leur raison.
    assert!(first.ignored.iter().any(|m| m.starts_with("gc-fantome")), "{:?}", first.ignored);
    assert!(first.ignored.iter().any(|m| m.contains("Orphelin.nsp")), "{:?}", first.ignored);
    let before = catalog(&a).await.len();
    let root = a.roms.path().to_path_buf();
    let aside = root.with_extension("demonte");
    // Partage non monté : la racine n'existe plus.
    std::fs::rename(&root, &aside).unwrap();
    let (s, _, _) = call(&a, Request::post("/api/v1/catalog/rescan").body(Body::empty()).unwrap()).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(catalog(&a).await.len(), before);
    // Point de montage vide : aucun jeu trouvé, l'ancien catalogue est gardé.
    std::fs::create_dir(&root).unwrap();
    let (s, _, _) = call(&a, Request::post("/api/v1/catalog/rescan").body(Body::empty()).unwrap()).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(catalog(&a).await.len(), before);
    std::fs::remove_dir(&root).unwrap();
    std::fs::rename(&aside, &root).unwrap();
    assert_eq!(a.store.rescan().unwrap().games, first.games);
}

#[tokio::test]
async fn dlc_d_un_autre_jeu_pas_rattache_par_le_nom() {
    // « Mario » est le début de « Mario Kart » : avec deux identifiants de titre connus et différents,
    // le DLC ne doit pas être rattaché par le nom.
    let a = app();
    put(a.roms.path(), "switch/Mario [0100000000010000][v0].nsp", &[1; 10]);
    put(a.roms.path(), "switch/MAJ-DLC/Mario Kart 9 [0100AAAAAAAA1001][v0].nsp", &[2; 10]);
    let r = a.store.rescan().unwrap();
    assert!(r.ignored.iter().any(|m| m.contains("Mario Kart 9")), "{:?}", r.ignored);
}

#[tokio::test]
async fn resume_des_sauvegardes_d_un_profil() {
    let a = app();
    let arch = |c: &str| {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("save.bin"), c).unwrap();
        ludo_core::snapshot::snapshot(t.path(), &[]).unwrap().unwrap().archive
    };
    let push = |profile: &str, game: &str, base: u64, device: &str, playtime_s: u64, discarded: bool, c: &str| {
        let key = SaveKey { profile: profile.into(), game: game.into() };
        let p = PushParams { base_rev: base, device: device.into(), emulator: "ryujinx".into(), playtime_s, discarded };
        a.store.push(&key, &p, &arch(c)).unwrap();
    };
    a.time.store(1_000, Ordering::SeqCst);
    push("claire", "switch-lm3", 0, "steam-deck", 100, false, "a");
    a.time.store(2_000, Ordering::SeqCst);
    push("claire", "switch-lm3", 1, "pc", 200, false, "b");
    push("claire", "switch-lm3", 2, "thor", 999, true, "c"); // version écartée : jamais tête
    a.time.store(1_500, Ordering::SeqCst);
    push("claire", "ds-pokemon-diamant", 0, "thor", 50, false, "d");
    push("guilhem", "switch-lm3", 0, "pc", 7, false, "e");

    let (s, _, b) = call(&a, Request::get("/api/v1/saves/claire").body(Body::empty()).unwrap()).await;
    assert_eq!(s, StatusCode::OK);
    let list: Vec<SaveSummary> = serde_json::from_slice(&b).unwrap();
    // Plus récent d'abord ; la tête de lm3 est la révision 2 (la 3 est écartée) ; rien de guilhem.
    assert_eq!(
        list,
        vec![
            SaveSummary { game: "switch-lm3".into(), rev: 2, playtime_s: 200, device: "pc".into(), emulator: "ryujinx".into(), created_at: 2_000 },
            SaveSummary { game: "ds-pokemon-diamant".into(), rev: 1, playtime_s: 50, device: "thor".into(), emulator: "ryujinx".into(), created_at: 1_500 },
        ]
    );
    let (s, _, b) = call(&a, Request::get("/api/v1/saves/personne").body(Body::empty()).unwrap()).await;
    assert_eq!((s, b.as_slice()), (StatusCode::OK, b"[]".as_slice()));
}

/// Entrées (nom, contenu) d'un tar ; les dossiers ont un contenu vide.
fn tar_entries(data: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut ar = tar::Archive::new(data);
    ar.entries()
        .unwrap()
        .map(|e| {
            let mut e = e.unwrap();
            let name = e.path().unwrap().to_string_lossy().into_owned();
            let mut v = Vec::new();
            std::io::Read::read_to_end(&mut e, &mut v).unwrap();
            (name, v)
        })
        .collect()
}

#[tokio::test]
async fn dossier_servi_en_tar_a_la_volee() {
    let a = app();
    a.store.rescan().unwrap();
    // Un lien symbolique vers l'extérieur ne doit jamais entrer dans l'archive.
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path().join("secret.txt"), a.roms.path().join("wiiu/Wind Waker HD/lien")).unwrap();
    let games = catalog(&a).await;
    let wiiu = &find(&games, "wiiu-wind-waker-hd").files[0];
    let uri = format!("/api/v1/games/wiiu-wind-waker-hd/files/{}", wiiu.id);
    // Le Range est ignoré : réponse 200 complète.
    let req = Request::get(&uri).header(header::RANGE, "bytes=10-20").body(Body::empty()).unwrap();
    let (s, h, b) = call(&a, req).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h[header::CONTENT_TYPE], "application/x-tar");
    assert_eq!(h["x-ludo-archive"], "tar");
    assert_eq!(h["x-ludo-total-bytes"], "100");
    assert!(h.get(header::CONTENT_RANGE).is_none());
    let entries = tar_entries(&b);
    let names: Vec<_> = entries.iter().map(|(n, _)| n.trim_end_matches('/')).collect();
    assert_eq!(names, vec!["code", "code/app.rpx", "content", "content/a.bin"]);
    assert_eq!(entries[1].1, vec![11; 70]);
    assert_eq!(entries[3].1, vec![12; 30]);
    assert!(!String::from_utf8_lossy(&b).contains("secret"));
}

#[test]
fn liste_tar_sans_liens_ni_evasion() {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "d/a.bin", &[1; 5]);
    put(root.path(), "d/sous/b.bin", &[2; 7]);
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("x"), "x").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.path(), root.path().join("d/dossier-lien")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("x"), root.path().join("d/fichier-lien")).unwrap();
    }
    let (entries, total) = ludo_server::list_tar_entries(&root.path().join("d")).unwrap();
    let names: Vec<_> = entries.iter().map(|e| e.name.to_str().unwrap()).collect();
    assert_eq!(names, vec!["a.bin", "sous", "sous/b.bin"]);
    assert_eq!(total, 12);
}

#[cfg(unix)]
#[tokio::test]
async fn taille_du_catalogue_egale_au_total_du_tar_meme_avec_noms_non_utf8() {
    use std::os::unix::ffi::OsStrExt;
    let a = app();
    let dir = a.roms.path().join("wiiu/Wind Waker HD/content");
    std::fs::write(dir.join(std::ffi::OsStr::from_bytes(b"\xff\xfe.bin")), [5u8; 9]).unwrap();
    std::os::unix::fs::symlink("a.bin", dir.join("lien")).unwrap();
    a.store.rescan().unwrap();
    let games = catalog(&a).await;
    let wiiu = &find(&games, "wiiu-wind-waker-hd").files[0];
    assert_eq!(wiiu.size, 109);
    let uri = format!("/api/v1/games/wiiu-wind-waker-hd/files/{}", wiiu.id);
    let (_, h, b) = call(&a, Request::get(uri).body(Body::empty()).unwrap()).await;
    assert_eq!(h["x-ludo-total-bytes"], "109");
    let total: usize = tar_entries(&b).iter().map(|(_, c)| c.len()).sum();
    assert_eq!(total, 109);
}

// --- Images des jeux ---

/// PNG minimal valide (signature + remplissage) : le serveur ne vérifie que les octets magiques.
fn png() -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
    v.extend_from_slice(&[7; 64]);
    v
}

fn put_cover(id: &str, ctype: &str, body: Vec<u8>) -> Request<Body> {
    Request::put(format!("/api/v1/games/{id}/cover")).header(header::CONTENT_TYPE, ctype).body(Body::from(body)).unwrap()
}

#[tokio::test]
async fn image_envoi_lecture_etag() {
    let a = app();
    a.store.rescan().unwrap();
    assert!(catalog(&a).await.iter().all(|g| g.cover.is_none()), "pas d'image au départ");
    let (s, _, _) = call(&a, Request::get("/api/v1/games/switch-lm3/cover").body(Body::empty()).unwrap()).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (s, _, _) = call(&a, put_cover("switch-lm3", "image/png", png())).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let games = catalog(&a).await;
    let url = find(&games, "switch-lm3").cover.clone().expect("cover présent");
    assert!(url.starts_with("/api/v1/games/switch-lm3/cover?v="), "{url}");
    assert!(find(&games, "ds-pokemon-diamant").cover.is_none());

    let (s, h, b) = call(&a, Request::get(&url).body(Body::empty()).unwrap()).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h[header::CONTENT_TYPE], "image/png");
    assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert!(h[header::CACHE_CONTROL].to_str().unwrap().contains("max-age"));
    assert_eq!(b, png());
    let etag = h[header::ETAG].to_str().unwrap().to_string();
    let (s, _, b) = call(&a, Request::get(&url).header(header::IF_NONE_MATCH, &etag).body(Body::empty()).unwrap()).await;
    assert_eq!((s, b.len()), (StatusCode::NOT_MODIFIED, 0));
    let (s, _, _) = call(&a, Request::get(&url).header(header::IF_NONE_MATCH, "\"autre\"").body(Body::empty()).unwrap()).await;
    assert_eq!(s, StatusCode::OK);

    // Remplacement par un JPEG : l'ancien PNG disparaît, le jeton de version change.
    let mut jpg = vec![0xFF, 0xD8, 0xFF, 0xE0];
    jpg.extend_from_slice(&[1; 99]);
    let (s, _, _) = call(&a, put_cover("switch-lm3", "image/jpeg", jpg.clone())).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let url2 = find(&catalog(&a).await, "switch-lm3").cover.clone().unwrap();
    assert_ne!(url, url2);
    let (_, h, b) = call(&a, Request::get(&url2).body(Body::empty()).unwrap()).await;
    assert_eq!((h[header::CONTENT_TYPE].to_str().unwrap(), b), ("image/jpeg", jpg));
}

#[tokio::test]
async fn image_refus_magic_taille_jeu_inconnu() {
    let a = app();
    a.store.rescan().unwrap();
    // L'en-tête du client ne compte pas : seuls les octets font foi.
    let (s, _, _) = call(&a, put_cover("switch-lm3", "image/png", b"<svg onload=alert(1)>".to_vec())).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _, _) = call(&a, put_cover("switch-lm3", "image/png", Vec::new())).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Trop lourd : refusé (413 par la limite de la route).
    let mut big = png();
    big.resize(5 * 1024 * 1024 + 1, 0);
    let (s, _, _) = call(&a, put_cover("switch-lm3", "image/png", big)).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    // Jeu absent du catalogue, identifiant malsain.
    let (s, _, _) = call(&a, put_cover("switch-fantome", "image/png", png())).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = call(&a, put_cover(".caché", "image/png", png())).await;
    assert!(s == StatusCode::BAD_REQUEST || s == StatusCode::NOT_FOUND, "{s}");
    assert!(catalog(&a).await.iter().all(|g| g.cover.is_none()));
}

#[tokio::test]
async fn image_survit_au_rebalayage() {
    let a = app();
    a.store.rescan().unwrap();
    call(&a, put_cover("ds-pokemon-diamant", "image/png", png())).await;
    a.store.rescan().unwrap();
    assert!(find(&catalog(&a).await, "ds-pokemon-diamant").cover.is_some());
}

#[tokio::test]
async fn image_lecture_refuse_traversee() {
    let a = app();
    a.store.rescan().unwrap();
    call(&a, put_cover("switch-lm3", "image/png", png())).await;
    // `..%2Fcovers%2Fswitch-lm3` résoudrait vers une image existante si l'id n'était pas validé.
    for uri in ["/api/v1/games/..%2Fcovers%2Fswitch-lm3/cover", "/api/v1/games/%2E%2E%2Fcovers%2Fswitch-lm3/cover"] {
        let (s, _, b) = call(&a, Request::get(uri).body(Body::empty()).unwrap()).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{uri}");
        assert_ne!(b, png(), "{uri}");
    }
}
