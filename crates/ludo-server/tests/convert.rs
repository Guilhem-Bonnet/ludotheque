//! Conversion NSZ → NSP côté serveur, avec un faux convertisseur (script shell) : 202 pendant la
//! conversion puis fichier du cache (Range, If-Range), une seule conversion pour des demandes
//! simultanées, une à la fois, éviction LRU, échec sans fichier partiel, fichier servi tel quel
//! quand le client accepte le format stocké, 503 sans clés.

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use ludo_proto::ConvertProgress;
use ludo_server::convert::{ConvertConfig, Converter};
use ludo_server::store::{Store, SystemClock};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

/// Faux nsz : `-D --minimal-output --keys K -o OUT FICHIER`. Attend tant que `gate` existe (à côté
/// des clés), note chaque appel dans `calls.log`, échoue si le nom contient « bad ».
const FAKE: &str = r#"#!/bin/bash
while [ $# -gt 0 ]; do case "$1" in --keys) K="$2"; shift 2;; -o) O="$2"; shift 2;; -D|--minimal-output) shift;; *) F="$1"; shift;; esac; done
D=$(dirname "$K")
echo "$F" >> "$D/calls.log"
printf '| 0%% Decompress\r/ 1%% Decompress\r- 42%% Decompress\r'
case "$F" in *noisy*) head -c 3000000 /dev/zero | tr '\0' 'x' >&2; head -c 2000000 /dev/zero | tr '\0' 'y';; esac
case "$F" in *forky*) sleep 300 & echo $! > "$D/child.pid"; wait;; esac
while [ -e "$D/gate" ]; do sleep 0.05; done
b=$(basename "$F")
case "$b" in *bad*) echo partiel > "$O/${b%.*}.nsp"; echo "boom" >&2; exit 1;; esac
case "${b##*.}" in nsz) n=nsp;; xcz) n=xci;; esac
{ cat "$F"; printf 'DECOMP'; } > "$O/${b%.*}.$n"
"#;

struct Env {
    router: axum::Router,
    tmp: tempfile::TempDir,
    games: Vec<(String, u64, String)>, // (jeu, fichier, nom)
}

impl Env {
    fn cache(&self) -> PathBuf {
        self.tmp.path().join("cache")
    }
    fn gate(&self, on: bool) {
        let g = self.tmp.path().join("keys/gate");
        if on {
            std::fs::write(g, "").unwrap();
        } else {
            let _ = std::fs::remove_file(g);
        }
    }
    fn calls(&self) -> usize {
        std::fs::read_to_string(self.tmp.path().join("keys/calls.log"))
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }
    fn ready(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(self.cache().join("ready"))
            .unwrap()
            .filter_map(Result::ok)
            .flat_map(|d| {
                std::fs::read_dir(d.path())
                    .unwrap()
                    .filter_map(Result::ok)
                    .map(|f| f.file_name().to_string_lossy().into_owned())
            })
            .collect();
        v.sort();
        v
    }
    fn ready_dirs(&self) -> usize {
        std::fs::read_dir(self.cache().join("ready"))
            .unwrap()
            .count()
    }
    fn id(&self, name: &str) -> (String, u64) {
        let (g, f, _) = self
            .games
            .iter()
            .find(|(_, _, n)| n.contains(name))
            .unwrap_or_else(|| panic!("{name} absent du catalogue : {:?}", self.games));
        (g.clone(), *f)
    }
    async fn get(
        &self,
        name: &str,
        query: &str,
        headers: &[(&str, &str)],
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let (g, f) = self.id(name);
        let mut req = Request::get(format!("/api/v1/games/{g}/files/{f}{query}"));
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = self
            .router
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let (status, h) = (resp.status(), resp.headers().clone());
        (
            status,
            h,
            resp.into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
    }
    /// Redemande jusqu'à autre chose qu'un 202.
    async fn until_done(&self, name: &str, query: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
        for _ in 0..200 {
            let r = self.get(name, query, &[]).await;
            if r.0 != StatusCode::ACCEPTED {
                return r;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("conversion jamais terminée");
    }
}

fn write(p: &Path, content: &[u8]) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

/// `keys` : fournir un fichier de clés (faux) ou non. `max` : taille maximale du cache.
fn env(keys: bool, max: u64, files: &[&str]) -> Env {
    env_with(keys, max, files, |_| ())
}

fn env_with(keys: bool, max: u64, files: &[&str], tweak: impl FnOnce(&mut ConvertConfig)) -> Env {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    for f in files {
        write(
            &roms.join("switch").join(f),
            format!("contenu de {f}").repeat(4).as_bytes(),
        );
    }
    write(&tmp.path().join("keys/prod.keys"), b"fausses cles de test");
    let script = tmp.path().join("faux-nsz.sh");
    std::fs::write(&script, FAKE).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let store = Store::open(&tmp.path().join("data"), Box::new(SystemClock))
        .unwrap()
        .with_roms(roms);
    store.rescan().unwrap();
    let games = store
        .games()
        .unwrap()
        .into_iter()
        .flat_map(|g| {
            g.files
                .into_iter()
                .map(move |f| (g.id.clone(), f.id, f.path))
        })
        .collect();
    let mut cfg = ConvertConfig::new(tmp.path().join("cache"));
    cfg.max_bytes = max;
    cfg.command = script.to_string_lossy().into_owned();
    cfg.keys = keys.then(|| tmp.path().join("keys/prod.keys"));
    cfg.timeout = Duration::from_secs(20);
    cfg.failure_backoff = Duration::from_secs(2);
    tweak(&mut cfg);
    let router = ludo_server::router_with(Arc::new(store), Some(Converter::new(cfg).unwrap()));
    Env { router, tmp, games }
}

const ALL: &str = "?formats=nsp,xci";

#[tokio::test]
async fn deux_cents_deux_puis_fichier_converti_avec_reprise() {
    let e = env(true, 1 << 30, &["Jeu [0100152000022000][v0].nsz"]);
    e.gate(true);
    let (st, h, body) = e.get("Jeu", ALL, &[]).await;
    assert_eq!(st, StatusCode::ACCEPTED);
    assert_eq!(h[header::RETRY_AFTER], "5");
    let p: ConvertProgress = serde_json::from_slice(&body).unwrap();
    assert!(p.state == "queued" || p.state == "converting");
    assert!(p.source_bytes > 0, "taille de la source annoncée");
    // Toujours en cours : encore 202, et une seule conversion lancée.
    assert_eq!(e.get("Jeu", ALL, &[]).await.0, StatusCode::ACCEPTED);
    e.gate(false);
    let (st, h, body) = e.until_done("Jeu", ALL).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(h["x-ludo-format"], "nsp");
    assert!(body.ends_with(b"DECOMP"));
    let total = body.len();
    assert_eq!(h["x-ludo-total-bytes"].to_str().unwrap(), total.to_string());
    assert_eq!(e.calls(), 1);
    assert_eq!(e.ready(), ["file.nsp"]);
    // Reprise : Range sur le fichier du cache, validateur stable.
    let etag = h[header::ETAG].to_str().unwrap().to_string();
    let (st, h2, part) = e.get("Jeu", ALL, &[("range", "bytes=5-")]).await;
    assert_eq!(st, StatusCode::PARTIAL_CONTENT);
    assert_eq!(part, body[5..]);
    assert_eq!(
        h2[header::CONTENT_RANGE].to_str().unwrap(),
        format!("bytes 5-{}/{total}", total - 1)
    );
    assert_eq!(h2[header::ETAG].to_str().unwrap(), etag);
    // If-Range identique : reprise ; différent : fichier entier.
    assert_eq!(
        e.get("Jeu", ALL, &[("range", "bytes=5-"), ("if-range", &etag)])
            .await
            .0,
        StatusCode::PARTIAL_CONTENT
    );
    assert_eq!(
        e.get(
            "Jeu",
            ALL,
            &[("range", "bytes=5-"), ("if-range", "\"autre\"")]
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(e.calls(), 1, "le cache sert, aucune nouvelle conversion");
}

#[tokio::test]
async fn servi_tel_quel_si_le_client_accepte_le_format_stocke() {
    let e = env(true, 1 << 30, &["Jeu [0100152000022000][v0].nsz"]);
    for q in [
        "",
        "?formats=nsz",
        "?formats=nsz,nsp",
        "?formats=zcci",
        "?formats=",
    ] {
        let (st, h, body) = e.get("Jeu", q, &[]).await;
        assert_eq!(st, StatusCode::OK, "{q}");
        assert!(h.get("x-ludo-format").is_none());
        assert!(!body.ends_with(b"DECOMP"), "{q}");
    }
    assert_eq!(e.calls(), 0);
}

#[tokio::test]
async fn sans_cles_conversion_indisponible() {
    let e = env(false, 1 << 30, &["Jeu [0100152000022000][v0].nsz"]);
    let (st, _, body) = e.get("Jeu", ALL, &[]).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        String::from_utf8(body).unwrap(),
        "Conversion indisponible : clés manquantes"
    );
    // Clés configurées mais fichier absent (secret optionnel non créé) : même réponse.
    let e2 = env(true, 1 << 30, &["Jeu [0100152000022000][v0].nsz"]);
    std::fs::remove_file(e2.tmp.path().join("keys/prod.keys")).unwrap();
    assert_eq!(
        e2.get("Jeu", ALL, &[]).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    // Sans conversion demandée, le service reste normal.
    assert_eq!(e.get("Jeu", "?formats=nsz", &[]).await.0, StatusCode::OK);
}

#[tokio::test]
async fn demandes_simultanees_une_seule_conversion() {
    let e = Arc::new(env(true, 1 << 30, &["Jeu [0100152000022000][v0].nsz"]));
    e.gate(true);
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let e = e.clone();
        tasks.push(tokio::spawn(
            async move { e.until_done("Jeu", ALL).await.0 },
        ));
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    e.gate(false);
    for t in tasks {
        assert_eq!(t.await.unwrap(), StatusCode::OK);
    }
    assert_eq!(e.calls(), 1);
}

#[tokio::test]
async fn une_conversion_a_la_fois() {
    let e = env(
        true,
        1 << 30,
        &[
            "A [0100152000022000][v0].nsz",
            "B [0100152000023000][v0].nsz",
        ],
    );
    e.gate(true);
    assert_eq!(e.get("A", ALL, &[]).await.0, StatusCode::ACCEPTED);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (st, _, body) = e.get("B", ALL, &[]).await;
    assert_eq!(st, StatusCode::ACCEPTED);
    assert_eq!(
        serde_json::from_slice::<ConvertProgress>(&body)
            .unwrap()
            .state,
        "queued"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(e.calls(), 1, "B attend la fin de A");
    e.gate(false);
    assert_eq!(e.until_done("A", ALL).await.0, StatusCode::OK);
    assert_eq!(e.until_done("B", ALL).await.0, StatusCode::OK);
    assert_eq!(e.calls(), 2);
}

#[tokio::test]
async fn echec_sans_fichier_partiel_puis_nouvel_essai() {
    let e = env(true, 1 << 30, &["Jeu bad [0100152000022000][v0].nsz"]);
    let (st, _, body) = e.until_done("bad", ALL).await;
    assert_eq!(st, StatusCode::INTERNAL_SERVER_ERROR);
    let msg = String::from_utf8(body).unwrap();
    assert!(msg.starts_with("Conversion échouée"), "{msg}");
    assert!(!msg.contains("boom"), "pas de détail interne côté client");
    assert_eq!(e.ready_dirs(), 0, "rien dans le cache");
    assert_eq!(
        std::fs::read_dir(e.cache().join("work")).unwrap().count(),
        0,
        "dossier de travail nettoyé"
    );
    // Une fois le délai d'attente écoulé, la demande suivante relance une conversion (qui échoue encore).
    tokio::time::sleep(Duration::from_millis(2100)).await;
    assert_eq!(
        e.until_done("bad", ALL).await.0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(e.calls(), 2);
}

#[tokio::test]
async fn eviction_lru_par_dernier_acces() {
    // Chaque conversion pèse ~ 3 × 4 × nom + 6 octets ; plafond réglé pour en garder trois.
    let names = [
        "A [0100152000022000][v0].nsz",
        "B [0100152000023000][v0].nsz",
        "C [0100152000024000][v0].nsz",
        "D [0100152000025000][v0].nsz",
    ];
    let probe = env(true, 1 << 30, &names[..1]);
    let one = probe.until_done("A", ALL).await.2.len() as u64;
    let src = one - 6;
    let e = env(true, one * 3 + src, &names);
    for n in ["A", "B", "C"] {
        assert_eq!(e.until_done(n, ALL).await.0, StatusCode::OK);
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert_eq!(e.ready_dirs(), 3);
    // A est servi de nouveau : B devient le moins récemment utilisé.
    assert_eq!(e.get("A", ALL, &[]).await.0, StatusCode::OK);
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(e.until_done("D", ALL).await.0, StatusCode::OK);
    assert_eq!(e.ready_dirs(), 3);
    // B a disparu : le redemander relance une conversion ; A, C et D sont servis sans appel.
    let before = e.calls();
    for n in ["A", "C", "D"] {
        assert_eq!(e.get(n, ALL, &[]).await.0, StatusCode::OK, "{n}");
    }
    assert_eq!(e.calls(), before);
    assert_eq!(e.get("B", ALL, &[]).await.0, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn pourcentage_lu_sur_la_sortie_a_retour_chariot() {
    let e = env(true, 1 << 30, &["Jeu [0100152000022000][v0].nsz"]);
    e.gate(true);
    let mut vu = None;
    for _ in 0..100 {
        let (st, _, body) = e.get("Jeu", ALL, &[]).await;
        assert_eq!(st, StatusCode::ACCEPTED);
        vu = serde_json::from_slice::<ConvertProgress>(&body)
            .unwrap()
            .percent;
        if vu.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        vu,
        Some(42),
        "pourcentage publié au fil de l'eau, avant la fin"
    );
    e.gate(false);
    assert_eq!(e.until_done("Jeu", ALL).await.0, StatusCode::OK);
}

#[tokio::test]
async fn sorties_volumineuses_sans_blocage() {
    // 3 Mo sur stderr et 2 Mo sur stdout sans fin de ligne : un tube non vidé bloquerait le convertisseur.
    let e = env(true, 1 << 30, &["Jeu noisy [0100152000022000][v0].nsz"]);
    let (st, _, body) = e.until_done("noisy", ALL).await;
    assert_eq!(st, StatusCode::OK);
    assert!(body.ends_with(b"DECOMP"));
}

fn vivant(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[tokio::test]
async fn delai_depasse_tue_le_groupe_de_processus() {
    let e = env_with(
        true,
        1 << 30,
        &["Jeu forky [0100152000022000][v0].nsz"],
        |c| c.timeout = Duration::from_millis(800),
    );
    let (st, _, _) = e.until_done("forky", ALL).await;
    assert_eq!(st, StatusCode::INTERNAL_SERVER_ERROR);
    let pid = std::fs::read_to_string(e.tmp.path().join("keys/child.pid"))
        .unwrap()
        .trim()
        .to_string();
    for _ in 0..40 {
        if !vivant(&pid) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        !vivant(&pid),
        "le fils lancé par le convertisseur doit être tué avec lui"
    );
    assert_eq!(e.ready_dirs(), 0);
}

#[tokio::test]
async fn echec_garde_en_attente_puis_nouvel_essai() {
    let e = env_with(
        true,
        1 << 30,
        &["Jeu bad [0100152000022000][v0].nsz"],
        |c| c.failure_backoff = Duration::from_millis(1500),
    );
    assert_eq!(
        e.until_done("bad", ALL).await.0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    // Pendant le délai : toujours l'échec, sans relancer le convertisseur.
    for _ in 0..3 {
        assert_eq!(
            e.get("bad", ALL, &[]).await.0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
    assert_eq!(e.calls(), 1, "aucune relance pendant le délai d'attente");
    tokio::time::sleep(Duration::from_millis(1600)).await;
    assert_eq!(
        e.until_done("bad", ALL).await.0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(e.calls(), 2, "relancé une fois le délai écoulé");
}

#[tokio::test]
async fn eviction_pendant_un_envoi_sans_coupure() {
    let names = [
        "A [0100152000022000][v0].nsz",
        "B [0100152000023000][v0].nsz",
        "C [0100152000024000][v0].nsz",
    ];
    let probe = env(true, 1 << 30, &names[..1]);
    let one = probe.until_done("A", ALL).await.2.len() as u64;
    let src = one - 6;
    // Place pour deux conversions : la troisième force une éviction.
    let e = env(true, one * 2 + src, &names);
    for n in ["A", "B"] {
        assert_eq!(e.until_done(n, ALL).await.0, StatusCode::OK);
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    // Envoi de A commencé (corps non lu) : A est le plus ancien mais en cours d'utilisation.
    let (g, f) = e.id("A");
    let resp = e
        .router
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/games/{g}/files/{f}{ALL}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(e.until_done("C", ALL).await.0, StatusCode::OK);
    assert_eq!(
        e.ready_dirs(),
        2,
        "une conversion évincée : la plus ancienne non utilisée (B)"
    );
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.len() as u64, one, "envoi complet malgré l'éviction");
    assert!(body.ends_with(b"DECOMP"));
    // Envoi fini : A n'est plus protégé, une nouvelle conversion peut l'évincer.
    assert_eq!(e.until_done("B", ALL).await.0, StatusCode::OK);
    assert_eq!(e.ready_dirs(), 2);
}
