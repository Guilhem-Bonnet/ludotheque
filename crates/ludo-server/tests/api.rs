//! Tests de l'API HTTP et du stockage : envoi avec compare-and-swap, conflit, version écartée,
//! historique, contenu, archives refusées, verrous, concurrence, blobs corrompus, profils.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use ludo_proto::{LeaseOutcome, Profile, PushOutcome, PushParams, SaveKey, SaveRevision};
use ludo_server::store::{Clock, Store};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

struct TestClock(Arc<AtomicU64>);
impl Clock for TestClock {
    fn now(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// Archive neutre réelle contenant un fichier `save.bin`.
fn arch(content: &str) -> Vec<u8> {
    let t = tempfile::tempdir().unwrap();
    std::fs::write(t.path().join("save.bin"), content).unwrap();
    ludo_core::snapshot::snapshot(t.path(), &[]).unwrap().unwrap().archive
}

struct App {
    router: axum::Router,
    time: Arc<AtomicU64>,
    _dir: tempfile::TempDir,
}

fn app() -> App {
    let dir = tempfile::tempdir().unwrap();
    let time = Arc::new(AtomicU64::new(1_000));
    let store = Store::open(dir.path(), Box::new(TestClock(time.clone()))).unwrap();
    App { router: ludo_server::router(Arc::new(store)), time, _dir: dir }
}

async fn call(app: &App, req: Request<Body>) -> (StatusCode, Vec<u8>) {
    let resp = app.router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, body)
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

async fn push_raw(app: &App, base: u64, device: &str, data: Vec<u8>, discarded: bool) -> (StatusCode, Vec<u8>) {
    let uri = format!(
        "/api/v1/saves/guilhem/switch-mk8?base_rev={base}&device={device}&emulator=ryujinx&playtime_s=60&discarded={discarded}"
    );
    call(app, Request::put(uri).body(Body::from(data)).unwrap()).await
}

async fn push(app: &App, base: u64, device: &str, content: &str) -> (StatusCode, PushOutcome) {
    let (s, b) = push_raw(app, base, device, arch(content), false).await;
    (s, serde_json::from_slice(&b).unwrap_or_else(|_| panic!("{s} {}", String::from_utf8_lossy(&b))))
}

async fn lease(app: &App, profile: &str, device: &str, force: bool) -> (StatusCode, LeaseOutcome) {
    let body = format!(r#"{{"device":"{device}","force":{force}}}"#);
    let req = Request::post(format!("/api/v1/leases/{profile}/switch-mk8"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let (s, b) = call(app, req).await;
    (s, serde_json::from_slice(&b).unwrap())
}

#[tokio::test]
async fn sante() {
    let a = app();
    let (s, b) = call(&a, get("/api/v1/health")).await;
    assert_eq!((s, b.as_slice()), (StatusCode::OK, &b"ok"[..]));
}

#[tokio::test]
async fn cycle_complet_envoi_conflit_version_ecartee_historique() {
    let a = app();
    let (s, _) = call(&a, get("/api/v1/saves/guilhem/switch-mk8")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Première sauvegarde (base 0).
    let (s, o) = push(&a, 0, "pc", "partie-1").await;
    assert_eq!(s, StatusCode::CREATED);
    assert!(matches!(o, PushOutcome::Accepted { ref revision } if revision.rev == 1 && revision.parent == 0));

    // Même contenu renvoyé : aucune révision créée.
    let (s, o) = push(&a, 1, "pc", "partie-1").await;
    assert_eq!(s, StatusCode::OK);
    assert!(matches!(o, PushOutcome::Unchanged { ref revision } if revision.rev == 1));

    // Le Deck joue sur la base 1 : révision 2.
    let (s, _) = push(&a, 1, "steam-deck", "partie-2").await;
    assert_eq!(s, StatusCode::CREATED);

    // Le PC, resté sur la base 1, envoie autre chose : conflit, la tête ne bouge pas.
    let (s, o) = push(&a, 1, "pc", "partie-3").await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert!(matches!(o, PushOutcome::Conflict { ref head } if head.rev == 2 && head.device == "steam-deck"));

    // Le joueur choisit la version du Deck : celle du PC est archivée comme écartée.
    let (s, b) = push_raw(&a, 1, "pc", arch("partie-3"), true).await;
    assert_eq!(s, StatusCode::CREATED);
    let o: PushOutcome = serde_json::from_slice(&b).unwrap();
    assert!(matches!(o, PushOutcome::Accepted { ref revision } if revision.rev == 3 && revision.discarded));

    // La tête reste celle du Deck.
    let (_, b) = call(&a, get("/api/v1/saves/guilhem/switch-mk8")).await;
    let head: SaveRevision = serde_json::from_slice(&b).unwrap();
    assert_eq!((head.rev, head.device.as_str()), (2, "steam-deck"));

    // Le CAS se fait toujours contre la tête (2), pas contre la version écartée (3).
    let (s, o) = push(&a, 2, "steam-deck", "partie-4").await;
    assert_eq!(s, StatusCode::CREATED);
    assert!(matches!(o, PushOutcome::Accepted { ref revision } if revision.rev == 4 && revision.parent == 2));

    // Historique complet, version écartée comprise, et contenu de chaque révision.
    let (_, b) = call(&a, get("/api/v1/saves/guilhem/switch-mk8/history")).await;
    let hist: Vec<SaveRevision> = serde_json::from_slice(&b).unwrap();
    assert_eq!(hist.iter().map(|r| (r.rev, r.discarded)).collect::<Vec<_>>(), vec![(4, false), (3, true), (2, false), (1, false)]);
    assert_eq!(hist[0].created_at, 1_000);
    for (rev, content) in [(1, "partie-1"), (2, "partie-2"), (3, "partie-3"), (4, "partie-4")] {
        let (s, b) = call(&a, get(&format!("/api/v1/saves/guilhem/switch-mk8/{rev}"))).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(b, arch(content));
    }

    // Les sauvegardes de Claire sont séparées.
    let (s, _) = call(&a, get("/api/v1/saves/claire/switch-mk8")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn archives_refusees() {
    let a = app();
    // Corps vide, octets quelconques, tar sans fichier.
    for data in [Vec::new(), b"pas une archive".to_vec(), tar::Builder::new(Vec::new()).into_inner().unwrap()] {
        let (s, _) = push_raw(&a, 0, "pc", data, false).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
    // Lien symbolique dans l'archive.
    let mut b = tar::Builder::new(Vec::new());
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(tar::EntryType::Symlink);
    h.set_size(0);
    h.set_link_name("/etc").unwrap();
    h.set_path("lnk").unwrap();
    h.set_cksum();
    b.append(&h, &b""[..]).unwrap();
    let (s, _) = push_raw(&a, 0, "pc", b.into_inner().unwrap(), false).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Rien n'a été enregistré.
    let (s, _) = call(&a, get("/api/v1/saves/guilhem/switch-mk8")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn identifiants_invalides_refuses() {
    let a = app();
    for uri in [
        "/api/v1/saves/guil%20hem/jeu",
        "/api/v1/saves/guilhem/%2e%2e",
        "/api/v1/saves/guilhem/.cache",
        "/api/v1/saves/guilhem/a%2Fb",
        "/api/v1/saves/history/jeu",
    ] {
        let (s, _) = call(&a, get(uri)).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{uri}");
    }
}

#[tokio::test]
async fn verrous_de_partie() {
    let a = app();
    let (s, _) = lease(&a, "guilhem", "pc", false).await;
    assert_eq!(s, StatusCode::OK);
    let (s, o) = lease(&a, "guilhem", "steam-deck", false).await;
    assert_eq!(s, StatusCode::LOCKED);
    assert!(matches!(o, LeaseOutcome::HeldElsewhere { ref lease } if lease.device == "pc"));
    // Deux profils ne se gênent jamais.
    let (s, _) = lease(&a, "claire", "steam-deck", false).await;
    assert_eq!(s, StatusCode::OK);
    // « Jouer ici » force la prise.
    let (s, _) = lease(&a, "guilhem", "steam-deck", true).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = call(&a, Request::delete("/api/v1/leases/guilhem/switch-mk8?device=steam-deck").body(Body::empty()).unwrap()).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = lease(&a, "guilhem", "pc", false).await;
    assert_eq!(s, StatusCode::OK);
    // Sans battement pendant plus de 2 minutes, le verrou expire.
    a.time.fetch_add(ludo_proto::LEASE_TTL_S + 1, Ordering::SeqCst);
    let (s, _) = lease(&a, "guilhem", "steam-deck", false).await;
    assert_eq!(s, StatusCode::OK);
}

fn params(base: u64, device: &str) -> PushParams {
    PushParams { base_rev: base, device: device.into(), emulator: "eden".into(), playtime_s: 1, discarded: false }
}

#[test]
fn persistance_apres_redemarrage() {
    let dir = tempfile::tempdir().unwrap();
    let time = Arc::new(AtomicU64::new(5));
    let key = SaveKey { profile: "claire".into(), game: "n3ds-mm3d".into() };
    {
        let s = Store::open(dir.path(), Box::new(TestClock(time.clone()))).unwrap();
        s.push(&key, &params(0, "steam-deck"), &arch("zelda")).unwrap();
    }
    let s = Store::open(dir.path(), Box::new(TestClock(time))).unwrap();
    assert_eq!(s.head(&key).unwrap().unwrap().rev, 1);
    assert_eq!(s.blob(&key, 1).unwrap().unwrap(), arch("zelda"));
}

#[test]
fn envois_concurrents_un_seul_gagnant_par_base() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path(), Box::new(TestClock(Arc::new(AtomicU64::new(1))))).unwrap());
    let key = SaveKey { profile: "guilhem".into(), game: "switch-botw".into() };
    store.push(&key, &params(0, "pc"), &arch("base")).unwrap();

    // 8 appareils envoient en même temps sur la base 1, dont 4 avec le même contenu.
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let (store, key) = (store.clone(), key.clone());
            std::thread::spawn(move || {
                let content = if i < 4 { "meme".to_string() } else { format!("partie-{i}") };
                store.push(&key, &params(1, &format!("d{i}")), &arch(&content)).unwrap()
            })
        })
        .collect();
    let outcomes: Vec<PushOutcome> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let accepted = outcomes.iter().filter(|o| matches!(o, PushOutcome::Accepted { .. })).count();
    assert_eq!(accepted, 1, "un seul envoi accepté sur la base 1 : {outcomes:?}");
    assert_eq!(store.head(&key).unwrap().unwrap().rev, 2);
    assert_eq!(store.history(&key).unwrap().len(), 2);
}

#[test]
fn blob_tronque_detecte_puis_repare_au_renvoi() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path(), Box::new(TestClock(Arc::new(AtomicU64::new(1))))).unwrap();
    let key = SaveKey { profile: "guilhem".into(), game: "switch-lm3".into() };
    let data = arch("luigi");
    store.push(&key, &params(0, "pc"), &data).unwrap();
    let hash = store.head(&key).unwrap().unwrap().hash;

    // Simule une coupure de courant : le blob est tronqué à 0 octet.
    let blob = dir.path().join("blobs").join(&hash[..2]).join(&hash);
    std::fs::write(&blob, b"").unwrap();
    assert!(store.blob(&key, 1).is_err(), "un blob corrompu n'est jamais servi");

    // Le même contenu renvoyé (autre jeu, même octets) répare le blob au lieu de le réutiliser.
    let other = SaveKey { profile: "guilhem".into(), game: "switch-lm3-copie".into() };
    store.push(&other, &params(0, "pc"), &data).unwrap();
    assert_eq!(store.blob(&key, 1).unwrap().unwrap(), data);
}

fn put_profile_req(id: &str, body: serde_json::Value) -> Request<Body> {
    Request::put(format!("/api/v1/profiles/{id}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn profile_json(id: &str, name: &str, kind: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id, "name": name, "avatarKind": kind,
        "accent": "#38b6ff", "accentSoft": "#123049", "accentInk": "#9ad8ff",
        "prefs": { "sons": true, "vibrations": false },
        // Champs d'affichage de l'interface : ignorés par le serveur.
        "status": "En cours sur Steam Deck", "busyElsewhere": true
    })
}

#[tokio::test]
async fn profils_creation_mise_a_jour_et_ordre() {
    let a = app();
    let (s, b) = call(&a, get("/api/v1/profiles")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(serde_json::from_slice::<Vec<Profile>>(&b).unwrap(), vec![]);

    // Création : le nom est enregistré sans les espaces autour.
    let (s, b) = call(&a, put_profile_req("guilhem", profile_json("guilhem", "  Guilhem ", "renard"))).await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&b));
    let p: Profile = serde_json::from_slice(&b).unwrap();
    assert_eq!((p.name.as_str(), p.avatar_kind.as_deref(), p.prefs.vibrations), ("Guilhem", Some("renard"), false));

    call(&a, put_profile_req("claire", profile_json("claire", "Claire", "champi"))).await;

    // Mise à jour de Guilhem : il garde sa place (ordre de création), les champs changent.
    let (s, _) = call(&a, put_profile_req("guilhem", profile_json("guilhem", "Gui", "poisson"))).await;
    assert_eq!(s, StatusCode::OK);

    let (_, b) = call(&a, get("/api/v1/profiles")).await;
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    // Forme JSON attendue par l'interface (camelCase), sans les champs d'affichage.
    assert_eq!(
        v[0],
        serde_json::json!({
            "id": "guilhem", "name": "Gui", "avatarKind": "poisson",
            "accent": "#38b6ff", "accentSoft": "#123049", "accentInk": "#9ad8ff",
            "prefs": { "sons": true, "vibrations": false }
        })
    );
    assert_eq!(v[1]["id"], "claire");
    assert_eq!(v.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn profils_invalides_refuses() {
    let a = app();
    let mut sans_prefs = profile_json("claire", "Claire", "chat");
    sans_prefs.as_object_mut().unwrap().remove("prefs");
    sans_prefs.as_object_mut().unwrap().remove("avatarKind");
    // Sans préférences ni personnage : accepté, préférences par défaut (tout activé).
    let (s, b) = call(&a, put_profile_req("claire", sans_prefs)).await;
    assert_eq!(s, StatusCode::OK);
    let p: Profile = serde_json::from_slice(&b).unwrap();
    assert_eq!((p.avatar_kind, p.prefs.sons, p.prefs.vibrations), (None, true, true));

    let mut mauvaise_couleur = profile_json("guilhem", "Guilhem", "renard");
    mauvaise_couleur["accent"] = "rouge".into();
    for (id, body) in [
        ("guilhem", profile_json("guilhem", "   ", "renard")),
        ("guilhem", profile_json("guilhem", "", "renard")),
        ("guilhem", profile_json("guilhem", "Dix-sept lettres!", "renard")),
        ("guilhem", profile_json("guilhem", "Guilhem", "dragon")),
        ("guilhem", profile_json("claire", "Claire", "renard")),
        ("guilhem", mauvaise_couleur),
        ("..", profile_json("..", "Point", "renard")),
    ] {
        let (s, b) = call(&a, put_profile_req(id, body.clone())).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{body} → {}", String::from_utf8_lossy(&b));
    }
    // Seize caractères (accents compris) : la limite est en caractères, pas en octets.
    let (s, _) = call(&a, put_profile_req("guilhem", profile_json("guilhem", "Éléonore-Hélène!", "etoile"))).await;
    assert_eq!(s, StatusCode::OK);

    let (_, b) = call(&a, get("/api/v1/profiles")).await;
    let ids: Vec<String> = serde_json::from_slice::<Vec<Profile>>(&b).unwrap().into_iter().map(|p| p.id).collect();
    assert_eq!(ids, vec!["claire", "guilhem"]);
}

#[test]
fn profils_persistent_apres_redemarrage() {
    let dir = tempfile::tempdir().unwrap();
    let p: Profile = serde_json::from_value(profile_json("guilhem", "Guilhem", "renard")).unwrap();
    {
        let s = Store::open(dir.path(), Box::new(TestClock(Arc::new(AtomicU64::new(1))))).unwrap();
        s.put_profile("guilhem", &p).unwrap();
    }
    let s = Store::open(dir.path(), Box::new(TestClock(Arc::new(AtomicU64::new(2))))).unwrap();
    assert_eq!(s.profiles().unwrap(), vec![p]);
}
