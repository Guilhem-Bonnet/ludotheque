//! Simulation de plusieurs appareils qui jouent et se synchronisent dans n'importe quel ordre,
//! contre un serveur en mémoire qui applique le protocole (compare-and-swap, versions écartées).
//!
//! La synchronisation est modélisée en **deux temps** (décision, puis application) pour que
//! d'autres appareils puissent passer entre les deux : les courses du CAS sont exercées.
//! Les contenus sont tirés en partie d'un petit réservoir, pour exercer « même contenu des
//! deux côtés » et les retours à un contenu antérieur.
//!
//! Invariants vérifiés (proptest) :
//! 1. `decide` ne propose jamais de télécharger par-dessus une partie locale non envoyée ;
//! 2. `decide` ne déclare un conflit que si les deux côtés ont changé depuis la base ;
//! 3. aucune partie locale n'est jamais remplacée sans être déjà sur le serveur
//!    (tête ou version écartée archivée) ;
//! 4. une fois que plus personne ne joue, tous les appareils convergent sur la tête.

use ludo_core::sync::{decide, LocalState, SyncAction};
use ludo_proto::SaveRevision;
use proptest::prelude::*;
use std::collections::HashSet;

#[derive(Default)]
struct Server {
    history: Vec<SaveRevision>,
}

impl Server {
    fn head(&self) -> Option<&SaveRevision> {
        self.history.iter().rev().find(|r| !r.discarded)
    }
    fn add(&mut self, parent: u64, hash: &str, discarded: bool) -> u64 {
        let rev = self.history.len() as u64 + 1;
        self.history.push(SaveRevision {
            rev,
            parent,
            hash: hash.into(),
            size: 1,
            device: "x".into(),
            emulator: "x".into(),
            playtime_s: 0,
            created_at: 0,
            discarded,
        });
        rev
    }
    /// CAS : `Ok(rev)` accepté (ou inchangé), `Err(())` si la tête a bougé.
    fn push(&mut self, base_rev: u64, hash: &str) -> Result<u64, ()> {
        let head = self.head().map(|h| (h.rev, h.hash.clone()));
        if let Some((rev, h)) = &head {
            if h == hash {
                return Ok(*rev);
            }
        }
        if head.map_or(0, |(r, _)| r) != base_rev {
            return Err(());
        }
        Ok(self.add(base_rev, hash, false))
    }
    fn hash_of(&self, rev: u64) -> String {
        self.history[(rev - 1) as usize].hash.clone()
    }
    fn contains(&self, hash: &str) -> bool {
        self.history.iter().any(|r| r.hash == hash)
    }
}

#[derive(Clone)]
struct Device {
    base_rev: u64,
    base_hash: Option<String>,
    current: Option<String>,
    pending: Option<SyncAction>,
}

impl Device {
    fn state(&self) -> LocalState {
        LocalState { base_rev: self.base_rev, base_hash: self.base_hash.clone(), current_hash: self.current.clone() }
    }
    fn unsent(&self) -> bool {
        self.current.is_some() && self.current != self.base_hash
    }
}

#[derive(Debug, Clone)]
enum Event {
    /// Joue : contenu tiré du réservoir (Some(k)) ou unique (None).
    Play(usize, Option<u8>),
    Decide(usize),
    Apply(usize, bool),
    Sync(usize, bool),
}

fn event() -> impl Strategy<Value = Event> {
    prop_oneof![
        (0usize..3, proptest::option::of(0u8..3)).prop_map(|(d, k)| Event::Play(d, k)),
        (0usize..3).prop_map(Event::Decide),
        (0usize..3, any::<bool>()).prop_map(|(d, b)| Event::Apply(d, b)),
        (0usize..3, any::<bool>()).prop_map(|(d, b)| Event::Sync(d, b)),
    ]
}

/// Décision, avec vérification des invariants 1 et 2 sur la sortie de `decide`.
fn decide_checked(dev: &Device, server: &Server) -> SyncAction {
    let action = decide(&dev.state(), server.head());
    let server_rev = server.head().map_or(0, |h| h.rev);
    match &action {
        SyncAction::Download { .. } => assert!(!dev.unsent(), "téléchargement proposé par-dessus une partie non envoyée"),
        SyncAction::Conflict { .. } => {
            assert!(dev.unsent(), "conflit sans modification locale");
            assert_ne!(server_rev, dev.base_rev, "conflit sans changement serveur");
        }
        _ => {}
    }
    action
}

/// Remplace le contenu local ; invariant 3 : la version remplacée est déjà sur le serveur.
fn replace_local(dev: &mut Device, server: &Server, rev: u64) {
    if let Some(cur) = &dev.current {
        assert!(!dev.unsent() || server.contains(cur), "partie locale remplacée sans être sur le serveur : {cur}");
    }
    let h = server.hash_of(rev);
    dev.base_rev = rev;
    dev.base_hash = Some(h.clone());
    dev.current = Some(h);
}

/// Applique une action décidée plus tôt ; le serveur a pu changer entre-temps.
/// `keep_local` = choix du joueur en cas de conflit.
fn apply(dev: &mut Device, server: &mut Server, action: SyncAction, keep_local: bool) {
    match action {
        SyncAction::UpToDate | SyncAction::LocalMissing => {}
        SyncAction::Rebase { rev } => {
            // Ne vaut que si le contenu local est toujours celui de cette révision.
            if dev.current.as_deref() == Some(server.hash_of(rev).as_str()) {
                dev.base_rev = rev;
                dev.base_hash = dev.current.clone();
            }
        }
        SyncAction::Download { rev } => {
            // Le joueur a pu rejouer depuis la décision : le démon revérifie avant d'écrire.
            if !dev.unsent() {
                replace_local(dev, server, rev);
            }
        }
        SyncAction::Upload { base_rev } | SyncAction::ServerBehind { server_rev: base_rev } => {
            if let Some(cur) = dev.current.clone() {
                if let Ok(rev) = server.push(base_rev, &cur) {
                    dev.base_rev = rev;
                    dev.base_hash = Some(cur);
                }
            }
        }
        SyncAction::Conflict { server_rev, .. } => {
            let Some(cur) = dev.current.clone() else { return };
            if keep_local {
                if let Ok(rev) = server.push(server_rev, &cur) {
                    dev.base_rev = rev;
                    dev.base_hash = Some(cur);
                }
            } else {
                // « Utiliser l'autre sauvegarde » : la version locale est d'abord archivée.
                server.add(dev.base_rev, &cur, true);
                replace_local(dev, server, server_rev);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(3000))]

    #[test]
    fn appareils_concurrents(n in 2usize..4, events in proptest::collection::vec(event(), 1..80)) {
        let mut server = Server::default();
        let mut devices = vec![Device { base_rev: 0, base_hash: None, current: None, pending: None }; n];
        let mut counter = 0u32;

        for ev in events {
            match ev {
                Event::Play(d, k) if d < n => {
                    counter += 1;
                    devices[d].current = Some(match k {
                        Some(k) => format!("commun-{k}"),
                        None => format!("partie-{counter}"),
                    });
                    devices[d].pending = None; // une partie invalide la décision en attente
                }
                Event::Decide(d) if d < n => {
                    let a = decide_checked(&devices[d], &server);
                    devices[d].pending = Some(a);
                }
                Event::Apply(d, keep) if d < n => {
                    if let Some(a) = devices[d].pending.take() {
                        apply(&mut devices[d], &mut server, a, keep);
                    }
                }
                Event::Sync(d, keep) if d < n => {
                    let a = decide_checked(&devices[d], &server);
                    apply(&mut devices[d], &mut server, a, keep);
                }
                _ => {}
            }
        }

        let pending: Vec<String> = devices.iter().filter(|d| d.unsent()).filter_map(|d| d.current.clone()).collect();

        // Plus personne ne joue : jusqu'à 4 tours de synchronisation atomique (« garder le serveur »).
        for _ in 0..4 {
            for dev in devices.iter_mut() {
                let a = decide_checked(dev, &server);
                apply(dev, &mut server, a, false);
            }
        }

        // Invariant 4 : convergence.
        let head = server.head().map(|h| h.hash.clone());
        for d in &devices {
            prop_assert_eq!(&d.current, &head);
        }
        // Invariant 3 (fin de partie) : toute partie en attente a fini sur le serveur.
        let on_server: HashSet<_> = server.history.iter().map(|r| r.hash.clone()).collect();
        for h in &pending {
            prop_assert!(on_server.contains(h), "partie perdue : {}", h);
        }
    }
}

#[test]
fn la_simulation_exerce_les_cas_difficiles() {
    // Garde-fou contre une simulation qui deviendrait vide : scénario déterministe qui passe
    // par un conflit, une version écartée et un même contenu des deux côtés.
    let mut server = Server::default();
    let mut a = Device { base_rev: 0, base_hash: None, current: Some("x".into()), pending: None };
    let mut b = Device { base_rev: 0, base_hash: None, current: None, pending: None };
    let act = decide_checked(&a, &server);
    apply(&mut a, &mut server, act, false);
    let act = decide_checked(&b, &server);
    apply(&mut b, &mut server, act, false);
    a.current = Some("a2".into());
    b.current = Some("b2".into());
    let act = decide_checked(&a, &server);
    apply(&mut a, &mut server, act, false);
    let act = decide_checked(&b, &server);
    assert!(matches!(act, SyncAction::Conflict { .. }));
    apply(&mut b, &mut server, act, false);
    assert!(server.history.iter().any(|r| r.discarded && r.hash == "b2"), "version écartée archivée");
    b.current = Some("a2".into()); // même contenu que la tête par un autre chemin
    assert!(matches!(decide_checked(&b, &server), SyncAction::UpToDate | SyncAction::Rebase { .. }));
}
