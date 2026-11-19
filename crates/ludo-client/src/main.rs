//! `ludo` : synchronise les sauvegardes et lance les jeux de la ludothèque.
//!
//! Codes de sortie : 0 succès, 1 erreur, 2 conflit à trancher (`--resoudre`),
//! 3 partie en cours sur un autre appareil (`--jouer-ici`), 4 dossier de sauvegarde disparu.

use clap::{Parser, Subcommand, ValueEnum};
use ludo_client::launch;
use ludo_client::api::Client;
use ludo_client::config::{self, Config};
use ludo_client::engine::{Engine, Resolution, SyncOutcome};
use ludo_client::journal::Journal;
use ludo_proto::{LeaseOutcome, SaveKey};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

#[derive(Parser)]
#[command(name = "ludo", version, about = "Ludothèque : sauvegardes synchronisées et lancement des jeux")]
struct Cli {
    /// Fichier de configuration de l'appareil.
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum Choix {
    /// Garder la sauvegarde de cet appareil.
    Local,
    /// Utiliser la sauvegarde du serveur.
    Serveur,
}

#[derive(clap::Args)]
struct Jeu {
    /// Profil (absent = profil par défaut de l'appareil).
    #[arg(long)]
    profil: Option<String>,
    /// Identifiant du jeu dans la ludothèque.
    #[arg(long)]
    jeu: String,
    /// Émulateur (fiche) : ryujinx, eden…
    #[arg(long)]
    emulateur: String,
    /// Identifiant du titre (16 caractères hexadécimaux).
    #[arg(long)]
    title_id: String,
    /// Choix en cas de conflit de sauvegarde.
    #[arg(long, value_enum)]
    resoudre: Option<Choix>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Synchronise la sauvegarde d'un jeu.
    Sync(Jeu),
    /// Synchronise, lance le jeu, puis renvoie la sauvegarde à la sortie.
    Run {
        #[command(flatten)]
        jeu: Jeu,
        /// Chemin de la ROM.
        #[arg(long)]
        rom: PathBuf,
        /// Jouer ici même si une partie est en cours sur un autre appareil.
        #[arg(long)]
        jouer_ici: bool,
        /// Partie jouée en flux depuis cet appareil client (Moonlight) : le verrou porte son nom
        /// (`pc-via-thor`). Par défaut : `SUNSHINE_CLIENT_NAME`, fourni par Sunshine.
        #[arg(long, env = "SUNSHINE_CLIENT_NAME")]
        via: Option<String>,
        /// Lanceur qui remplace `{bin}` de la fiche (ex. `~/.local/bin/ryu-stream` en flux : manette
        /// Sunshine, génération d'images). Les arguments de la fiche (profil, ROM) lui sont passés.
        #[arg(long)]
        lanceur: Option<String>,
    },
    /// État des sauvegardes connues de cet appareil.
    Statut,
    /// Émulateurs connus et ce qui en est trouvé sur cet appareil (lecture seule, sans cache).
    Emulateurs,
    /// Synchronise, depuis ce PC, les sauvegardes d'un appareil Android branché en adb (USB ou Wi-Fi),
    /// décrit par une configuration dédiée (`--config ~/.config/ludo/thor.toml`, section `[adb]`).
    AdbSync {
        /// Profil (absent = profil par défaut de la configuration de l'appareil).
        #[arg(long)]
        profil: Option<String>,
        /// Un seul jeu (identifiant ludothèque).
        #[arg(long)]
        jeu: Option<String>,
        /// Choix en cas de conflit de sauvegarde (sinon le conflit est signalé, rien n'est écrit).
        #[arg(long, value_enum)]
        resoudre: Option<Choix>,
    },
    /// Démon : synchronise l'appareil adb à chaque événement — branchement (USB ou Wi-Fi, flux
    /// `adb track-devices`), fermeture d'un émulateur (`am_proc_died` du journal Android),
    /// écriture d'une sauvegarde (`inotifyd` sur l'appareil, anti-rebond). Aucune scrutation.
    AdbWatch,
    /// Démon : surveille (inotify) les dossiers de sauvegarde des émulateurs de CET appareil et
    /// synchronise un jeu 20 s après sa dernière écriture — y compris pour un émulateur lancé
    /// hors de la Ludothèque. Une partie en cours via la Ludothèque (verrou) est laissée à celle-ci.
    Watch,
    /// Jeux du catalogue et leurs fichiers trouvés sur cet appareil (dossier de la Ludothèque ou
    /// collection existante). Lecture seule.
    Jeux,
}

struct Ctx {
    cfg: Config,
    client: Client,
    journal: Journal,
}

fn label(o: &SyncOutcome) -> String {
    match o {
        SyncOutcome::UpToDate => "À jour".into(),
        SyncOutcome::Downloaded { .. } => "À jour – sauvegarde récupérée".into(),
        SyncOutcome::Uploaded { .. } => "À jour – sauvegarde envoyée".into(),
        SyncOutcome::Offline { pending_upload: true } => "Hors ligne – envoi en attente".into(),
        SyncOutcome::Offline { pending_upload: false } => "Hors ligne – sauvegarde locale utilisée".into(),
        SyncOutcome::Conflict { server } => format!("Conflit de sauvegarde – serveur : {} (révision {})", server.device, server.rev),
        SyncOutcome::LocalMissing => "Dossier de sauvegarde introuvable – rien n'a été modifié".into(),
    }
}

fn sync_once(ctx: &mut Ctx, jeu: &Jeu, key: &SaveKey, playtime_s: u64) -> Result<SyncOutcome, String> {
    let profile = key.profile.clone();
    let prepared = config::prepare(&ctx.cfg, &jeu.emulateur, &profile, &jeu.title_id).map_err(|e| e.to_string())?;
    let mut engine = Engine {
        client: &ctx.client,
        journal: &mut ctx.journal,
        device: ctx.cfg.appareil.clone(),
        emulator: jeu.emulateur.clone(),
        playtime_s,
    };
    let mut outcome = engine.sync(key, prepared.layout.as_ref()).map_err(|e| e.to_string())?;
    if let (SyncOutcome::Conflict { .. }, Some(choice)) = (&outcome, jeu.resoudre) {
        let r = match choice {
            Choix::Local => Resolution::KeepLocal,
            Choix::Serveur => Resolution::KeepServer,
        };
        outcome = engine.resolve(key, prepared.layout.as_ref(), r).map_err(|e| e.to_string())?;
    }
    Ok(outcome)
}

fn key_for(ctx: &Ctx, jeu: &Jeu) -> Result<SaveKey, String> {
    let profile = jeu
        .profil
        .clone()
        .or_else(|| ctx.cfg.profil_defaut.clone())
        .ok_or("Qui joue ? – préciser --profil (appareil partagé)")?;
    Ok(SaveKey { profile, game: jeu.jeu.clone() })
}

fn exit_for(o: &SyncOutcome) -> Option<ExitCode> {
    match o {
        SyncOutcome::Conflict { .. } => Some(ExitCode::from(2)),
        SyncOutcome::LocalMissing => Some(ExitCode::from(4)),
        _ => None,
    }
}

fn run(ctx: &mut Ctx, jeu: &Jeu, rom: &std::path::Path, jouer_ici: bool, via: Option<&str>, lanceur: Option<&str>) -> Result<ExitCode, String> {
    let key = key_for(ctx, jeu)?;
    // En flux (Moonlight), le verrou dit d'où l'on joue : « pc-via-thor ».
    let device = via.map_or_else(|| ctx.cfg.appareil.clone(), |c| launch::stream_device(&ctx.cfg.appareil, c));

    // Verrou de partie (souple). Hors ligne : on joue quand même.
    let online = match ctx.client.acquire_lease(&key, &device, jouer_ici) {
        Ok(LeaseOutcome::HeldElsewhere { lease }) => {
            println!("En cours sur {}", launch::holder_device(&lease.device));
            return Ok(ExitCode::from(3));
        }
        Ok(LeaseOutcome::Granted { .. }) => true,
        Err(_) => false,
    };

    // Cumul du serveur avant la partie (None = serveur injoignable, à ne pas confondre avec 0) :
    // un envoi fait avant la partie ne doit jamais remettre le cumul à zéro.
    let base_playtime = launch::head_playtime(&ctx.client, &key);
    let before = sync_once(ctx, jeu, &key, base_playtime.unwrap_or(0))?;
    println!("{}", label(&before));
    if let Some(code) = exit_for(&before) {
        if online {
            let _ = ctx.client.release_lease(&key, &device);
        }
        return Ok(code);
    }

    let mut prepared = config::prepare(&ctx.cfg, &jeu.emulateur, &key.profile, &jeu.title_id).map_err(|e| e.to_string())?;
    if let Some(l) = lanceur {
        prepared.vars.insert("bin", l.to_string());
    }
    let argv = config::launch_command(&prepared, &launch::boot_file(rom)).map_err(|e| e.to_string())?;
    let (bin, args) = argv.split_first().ok_or("commande de lancement vide")?;
    let started = Instant::now();
    // Le verrou expire au bout de 120 s : renouvelé pendant toute la partie (comme l'app).
    let stop = launch::Stop::new();
    let client = &ctx.client;
    let status = std::thread::scope(|s| {
        let _stop = launch::StopOnDrop(&stop);
        if online {
            s.spawn(|| launch::heartbeat(client, &key, &device, &stop));
        }
        // SIGTERM (fin du flux Sunshine) : émulateur arrêté proprement, puis sauvegarde envoyée.
        launch::run_emulator(bin, args, launch::EMULATOR_GRACE)
    })
    .map_err(|e| format!("Échec du lancement : {e}"))?;
    let (status, stopped) = status;
    if stopped {
        println!("Arrêt demandé : émulateur fermé, envoi de la sauvegarde");
    }
    let elapsed = started.elapsed().as_secs();
    if !status.success() {
        println!("Émulateur terminé avec une erreur ({status}) – sauvegarde quand même synchronisée");
    }

    // Cumul relu juste avant l'envoi : une partie commencée hors ligne ne remet pas le cumul à la durée seule.
    let total = launch::playtime_after(base_playtime, launch::head_playtime(&ctx.client, &key), elapsed);
    let after = sync_once(ctx, jeu, &key, total)?;
    println!("{}", label(&after));
    if online {
        let _ = ctx.client.release_lease(&key, &device);
    }
    Ok(exit_for(&after).unwrap_or(ExitCode::SUCCESS))
}

/// Synchronisation d'un appareil Android par adb, jeu par jeu (voir `ludo_client::adb`).
fn adb_sync(ctx: &Ctx, profil: Option<String>, only: Option<String>, resoudre: Option<Choix>) -> Result<ExitCode, String> {
    use ludo_client::adb::{self, Adb};
    let conf = ctx.cfg.adb.as_ref().ok_or("section [adb] absente de la configuration")?;
    let Some(dev) = Adb::find(&conf.series) else {
        println!("{} non connecté (adb) : rien à faire", ctx.cfg.appareil);
        return Ok(ExitCode::SUCCESS);
    };
    let profile = profil.or_else(|| ctx.cfg.profil_defaut.clone()).ok_or("préciser --profil")?;
    let games = ctx.client.games().map_err(|e| e.to_string())?;
    // ROM présentes sur l'appareil (par nom, formats équivalents) : jeux à suivre même sans sauvegarde.
    let rom_names: Vec<String> = conf
        .roms
        .as_deref()
        .map(|r| ["switch", "switch2", "n3ds", "3ds", "nds", "ds"].iter().flat_map(|c| dev.list(&format!("{r}/{c}"))).collect())
        .unwrap_or_default();
    let rom_titles: std::collections::BTreeSet<(String, String)> = rom_names.iter().filter_map(|n| ludo_client::local::title_key(n)).collect();
    // Présence d'un fichier du catalogue parmi les ROM de l'appareil : par nom, sinon par identité Switch.
    let rom_has = |path: &str, names: &std::collections::BTreeSet<(String, String)>| {
        names.contains(&ludo_client::local::match_key(path)) || ludo_client::local::title_key(path).is_some_and(|t| rom_titles.contains(&t))
    };
    let roms: std::collections::BTreeSet<(String, String)> = conf
        .roms
        .as_deref()
        .map(|r| ["switch", "switch2", "n3ds", "3ds", "nds", "ds"].iter().flat_map(|c| dev.list(&format!("{r}/{c}"))).map(|n| ludo_client::local::match_key(&n)).collect())
        .unwrap_or_default();
    // Inventaire de l'appareil (envoyé au serveur à la fin) : ROM présentes, jeux jouables en flux
    // depuis ce PC (applis Sunshine), sauvegardes trouvées sur l'appareil.
    let streamable = sunshine_roms();
    let mut inventory: std::collections::BTreeMap<String, ludo_proto::DeviceGame> = games
        .iter()
        .map(|g| {
            let installed = g.files.iter().any(|f| f.role == ludo_proto::FileRole::Base && rom_has(&f.path, &roms));
            let stream_app = streamable.app_for(g).cloned();
            (g.id.clone(), ludo_proto::DeviceGame { game: g.id.clone(), installed, stream: stream_app.is_some(), save: false, stream_app })
        })
        .collect();
    let journal_path = config::journal_path().with_file_name(format!("journal-{}.json", ctx.cfg.appareil));
    let mut journal = Journal::open(&journal_path).map_err(|e| e.to_string())?;
    let stamp = chrono_stamp();
    let mut code = ExitCode::SUCCESS;
    for g in games.iter().filter(|g| only.as_deref().is_none_or(|j| j == g.id)) {
        let Some(gc) = ctx.cfg.game_conf(&g.id, &profile, Some(g)) else { continue };
        let Some(package) = adb::package_of(&ctx.cfg, &gc.emulateur) else { continue };
        let mirror = tempfile::tempdir().map_err(|e| e.to_string())?;
        let (prepared, pairs) = match adb::prepare_mirrored(&ctx.cfg, &gc, &profile, mirror.path()) {
            Ok(p) => p,
            Err(e) => {
                println!("{}\t{e}", g.id);
                continue;
            }
        };
        let present_remote: Vec<bool> = pairs.iter().map(|(_, r)| dev.is_dir(r)).collect();
        if let Some(e) = inventory.get_mut(&g.id) {
            e.save = present_remote.iter().any(|p| *p);
        }
        let has_rom = g.files.iter().any(|f| f.role == ludo_proto::FileRole::Base && rom_has(&f.path, &roms));
        let key = SaveKey { profile: profile.clone(), game: g.id.clone() };
        let on_server = ctx.client.head(&key).ok().flatten().is_some();
        // À suivre : sauvegarde déjà sur l'appareil, ROM présente, ou jeu demandé explicitement.
        let wanted = present_remote.iter().any(|p| *p) || has_rom || (on_server && only.is_some());
        if !wanted {
            continue;
        }
        if dev.running(&package) {
            println!("{}\tÉmulateur en cours sur l'appareil : ignoré", g.id);
            continue;
        }
        let device = ctx.cfg.appareil.clone();
        match ctx.client.acquire_lease(&key, &device, false) {
            Ok(LeaseOutcome::HeldElsewhere { lease }) => {
                println!("{}\tEn cours sur {}", g.id, launch::holder_device(&lease.device));
                continue;
            }
            Ok(LeaseOutcome::Granted { .. }) => {}
            Err(e) => return Err(format!("serveur injoignable : {e}")),
        }
        // Entrée du journal avant la synchronisation : rétablie si la sauvegarde ne peut pas être
        // reposée sur l'appareil (sinon l'ancienne version restée là passerait pour une modification).
        let journal_before = journal.get(&key);
        let result = (|| -> Result<SyncOutcome, String> {
            for ((local, remote), present) in pairs.iter().zip(&present_remote) {
                if *present {
                    dev.pull_dir(remote, local).map_err(|e| e.to_string())?;
                }
            }
            let playtime_s = launch::head_playtime(&ctx.client, &key).unwrap_or(0);
            let mut engine = Engine { client: &ctx.client, journal: &mut journal, device: device.clone(), emulator: gc.emulateur.clone(), playtime_s };
            let mut out = engine.sync(&key, prepared.layout.as_ref()).map_err(|e| e.to_string())?;
            if let (SyncOutcome::Conflict { .. }, Some(choice)) = (&out, resoudre) {
                let r = match choice {
                    Choix::Local => Resolution::KeepLocal,
                    Choix::Serveur => Resolution::KeepServer,
                };
                out = engine.resolve(&key, prepared.layout.as_ref(), r).map_err(|e| e.to_string())?;
            }
            // Sauvegarde du serveur installée dans le miroir : la reposer sur l'appareil.
            if matches!(out, SyncOutcome::Downloaded { .. }) {
                for (local, remote) in &pairs {
                    if local.is_dir() {
                        if let Err(e) = dev.push_replace(local, remote, &stamp) {
                            let _ = engine.journal.set(&key, journal_before.clone());
                            return Err(format!("{e} (journal rétabli, rien n'a été remplacé sur l'appareil)"));
                        }
                    }
                }
            }
            Ok(out)
        })();
        let _ = ctx.client.release_lease(&key, &device);
        match result {
            Ok(o) => {
                println!("{}\t{}", g.id, label(&o));
                if exit_for(&o).is_some() {
                    code = ExitCode::from(2);
                }
            }
            Err(e) => {
                println!("{}\tÉchec : {e}", g.id);
                code = ExitCode::from(1);
            }
        }
    }
    // Inventaire complet (même pour une synchronisation limitée à un jeu, il décrit tout l'appareil).
    if only.is_none() {
        let list: Vec<ludo_proto::DeviceGame> = inventory.into_values().filter(|g| g.installed || g.stream || g.save).collect();
        match ctx.client.put_inventory(&ctx.cfg.appareil, &list) {
            Ok(inv) => println!("Inventaire envoyé : {} jeux sur {}", inv.games.len(), ctx.cfg.appareil),
            Err(e) => println!("Inventaire non envoyé : {e}"),
        }
    }
    Ok(code)
}

/// Applis Sunshine de ce PC (`~/.config/sunshine/apps.json`), voir [`sunshine_apps`].
fn sunshine_roms() -> SunshineApps {
    let path = config::home().join(".config/sunshine/apps.json");
    std::fs::read_to_string(path).map(|t| sunshine_apps(&t)).unwrap_or_default()
}

/// Jeux jouables en flux depuis ce PC et nom de l'appli Sunshine qui les lance (Moonlight `AppName`).
#[derive(Debug, Default, PartialEq)]
struct SunshineApps {
    /// Clé de rapprochement (nom, format équivalent) d'un chemin cité dans une commande → appli.
    by_rom: std::collections::BTreeMap<(String, String), String>,
    /// Identifiant Ludothèque (`ludo run … --jeu <id>`) → appli.
    by_game: std::collections::BTreeMap<String, String>,
}

impl SunshineApps {
    /// Appli qui lance ce jeu : par identifiant d'abord, sinon par un fichier du catalogue.
    fn app_for(&self, game: &ludo_proto::CatalogGame) -> Option<&String> {
        self.by_game.get(&game.id).or_else(|| game.files.iter().find_map(|f| self.by_rom.get(&ludo_client::local::match_key(&f.path))))
    }
}

/// Lit `apps.json` : chemins cités entre guillemets dans chaque commande (et leurs dossiers parents :
/// un jeu en dossier, Switch 2 extrait, est souvent cité par un fichier intérieur). Plusieurs applis
/// pour le même jeu : celle qui passe par `ludo run` l'emporte (sauvegarde synchronisée).
fn sunshine_apps(text: &str) -> SunshineApps {
    let mut out = SunshineApps::default();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return out };
    for app in v.get("apps").and_then(|a| a.as_array()).into_iter().flatten() {
        let cmd = app.get("cmd").and_then(|c| c.as_str()).unwrap_or("");
        let Some(name) = app.get("name").and_then(|n| n.as_str()).filter(|n| !n.is_empty()) else { continue };
        let ludo = cmd.contains("ludo run");
        // Première appli trouvée pour un jeu, sauf si une appli `ludo run` arrive ensuite.
        fn keep<K: Ord>(map: &mut std::collections::BTreeMap<K, String>, key: K, name: &str, ludo: bool) {
            match map.entry(key) {
                std::collections::btree_map::Entry::Vacant(e) => {
                    e.insert(name.to_string());
                }
                std::collections::btree_map::Entry::Occupied(mut e) if ludo => {
                    e.insert(name.to_string());
                }
                std::collections::btree_map::Entry::Occupied(_) => {}
            }
        }
        if ludo {
            let mut words = cmd.split_whitespace();
            while let Some(w) = words.next() {
                if w == "--jeu" {
                    if let Some(id) = words.next() {
                        let id = id.trim_matches(|c| c == '"' || c == '\'');
                        keep(&mut out.by_game, id.to_string(), name, ludo);
                    }
                }
            }
        }
        for quote in ['"', '\'']
        {
            let mut parts = cmd.split(quote);
            parts.next();
            while let Some(inside) = parts.next() {
                if inside.starts_with('/') {
                    for anc in std::path::Path::new(inside).ancestors().take(6) {
                        if let Some(n) = anc.file_name() {
                            let key = ludo_client::local::match_key(&n.to_string_lossy());
                            keep(&mut out.by_rom, key, name, ludo);
                        }
                    }
                }
                parts.next();
            }
        }
    }
    out
}

/// Événements qui déclenchent une synchronisation de l'appareil adb.
enum WatchEv {
    /// Liste des séries présentes (état « device »), poussée par `adb track-devices`.
    Devices(Vec<String>),
    /// `adb track-devices` s'est arrêté (serveur adb relancé) : à relancer.
    TrackerEnded,
    /// Processus terminé sur l'appareil (paquet Android).
    ProcDied(String),
    /// Écriture dans un dossier de sauvegarde (chemin du dossier surveillé).
    SaveWritten(String),
    /// Nouveau dossier de jeu créé : la surveillance doit l'inclure.
    NewSaveDir,
}

/// Lit le flux de `adb track-devices` : trames « longueur sur 4 hex + lignes série\tétat ».
fn spawn_tracker(tx: std::sync::mpsc::Sender<WatchEv>) -> std::io::Result<std::process::Child> {
    use std::io::Read;
    let mut child = std::process::Command::new("adb").arg("track-devices").stdout(std::process::Stdio::piped()).spawn()?;
    let mut out = child.stdout.take().ok_or_else(|| std::io::Error::other("sortie de adb indisponible"))?;
    std::thread::spawn(move || {
        loop {
            let mut len = [0u8; 4];
            if out.read_exact(&mut len).is_err() {
                break;
            }
            let Ok(n) = usize::from_str_radix(&String::from_utf8_lossy(&len), 16) else { break };
            let mut body = vec![0u8; n];
            if out.read_exact(&mut body).is_err() {
                break;
            }
            let present = String::from_utf8_lossy(&body)
                .lines()
                .filter_map(|l| l.split_once('\t'))
                .filter(|(_, state)| *state == "device")
                .map(|(s, _)| s.to_string())
                .collect();
            if tx.send(WatchEv::Devices(present)).is_err() {
                return;
            }
        }
        let _ = tx.send(WatchEv::TrackerEnded);
    });
    Ok(child)
}

/// Lance une commande sur l'appareil et transforme chaque ligne de sortie en événement.
fn spawn_device_stream(serial: &str, script: &str, tx: std::sync::mpsc::Sender<WatchEv>, map: fn(&str) -> Option<WatchEv>) -> Option<std::process::Child> {
    use std::io::BufRead;
    let mut child = std::process::Command::new("adb").args(["-s", serial, "shell", script]).stdout(std::process::Stdio::piped()).spawn().ok()?;
    let out = child.stdout.take()?;
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(ev) = map(&line) {
                if tx.send(ev).is_err() {
                    break;
                }
            }
        }
    });
    Some(child)
}

/// `am_proc_died: [user,pid,paquet,…]` → paquet.
fn proc_died(line: &str) -> Option<WatchEv> {
    let inner = line.split("am_proc_died:").nth(1)?.trim().trim_start_matches('[');
    inner.split(',').nth(2).map(|p| WatchEv::ProcDied(p.trim().to_string()))
}

/// Ligne d'`inotifyd -` : « <événement>\t<dossier>[\t<nom>] ». `n` (création) dans une racine
/// de sauvegardes = nouveau jeu.
fn save_event(line: &str) -> Option<WatchEv> {
    let mut parts = line.split('\t');
    let ev = parts.next()?;
    let dir = parts.next()?.to_string();
    Some(if ev.contains('n') && parts.next().is_some() && dir.matches('/').count() > 0 && dir.ends_with(|c: char| c.is_ascii_hexdigit()) && dir.rsplit('/').next().is_some_and(|d| d.len() == 32) {
        WatchEv::NewSaveDir
    } else {
        WatchEv::SaveWritten(dir)
    })
}

/// Dossiers de sauvegarde à surveiller sur l'appareil : racines par profil (création d'un jeu)
/// et dossier de chaque jeu (écritures). inotify n'est pas récursif.
fn watch_dirs(ctx: &Ctx, dev: &ludo_client::adb::Adb) -> (Vec<String>, Vec<String>) {
    use ludo_client::adb;
    let mut emulators: Vec<String> = ctx.cfg.consoles.values().cloned().collect();
    emulators.extend(ctx.cfg.jeux.values().map(|g| g.emulateur.clone()));
    emulators.sort();
    emulators.dedup();
    let mut roots = Vec::new();
    let mut packages = Vec::new();
    for e in emulators {
        if let Some(p) = adb::package_of(&ctx.cfg, &e) {
            packages.push(p);
        }
        for profile in ctx.cfg.profils.keys() {
            let probe = config::GameConf { emulateur: e.clone(), title_id: "0000000000000000".into(), profils: None };
            let Ok(tmp) = tempfile::tempdir() else { continue };
            if let Ok((_, pairs)) = adb::prepare_mirrored(&ctx.cfg, &probe, profile, tmp.path()) {
                for (_, remote) in pairs {
                    if let Some(root) = std::path::Path::new(&remote).parent().and_then(|p| p.to_str()) {
                        if !roots.contains(&root.to_string()) && dev.is_dir(root) {
                            roots.push(root.to_string());
                        }
                    }
                }
            }
        }
    }
    let mut dirs = roots.clone();
    for r in &roots {
        dirs.extend(dev.list(r).into_iter().filter(|n| !n.contains(".ludo-")).map(|n| format!("{r}/{n}")));
    }
    (dirs, packages)
}

/// Démon événementiel de synchronisation d'un appareil adb (voir `Cmd::AdbWatch`).
fn adb_watch(ctx: &Ctx) -> Result<ExitCode, String> {
    use std::sync::mpsc;
    use std::time::Duration;
    let conf = ctx.cfg.adb.as_ref().ok_or("section [adb] absente de la configuration")?;
    // Wi-Fi : une tentative de connexion au démarrage ; ensuite le serveur adb signale lui-même
    // les connexions (et reconnecte les appareils appairés en débogage sans fil, via mDNS).
    for s in conf.series.iter().filter(|s| s.contains(':')) {
        let _ = std::process::Command::new("adb").args(["connect", s]).output();
    }
    let (tx, rx) = mpsc::channel::<WatchEv>();
    let mut tracker = spawn_tracker(tx.clone()).map_err(|e| format!("adb track-devices : {e}"))?;
    let mut watchers: Vec<std::process::Child> = Vec::new();
    let mut connected: Option<String> = None;
    let mut last_present: Vec<String> = Vec::new();
    let mut packages: Vec<String> = Vec::new();
    // Anti-rebond : échéance de synchronisation repoussée à chaque événement, calculée (pas de tick).
    let mut due: Option<Instant> = None;
    const AFTER_WRITE: Duration = Duration::from_secs(20);
    const AFTER_EXIT: Duration = Duration::from_secs(3);
    let kill_all = |w: &mut Vec<std::process::Child>| {
        for c in w.iter_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
        w.clear();
    };
    println!("Surveillance de {} : en attente d'événements", ctx.cfg.appareil);
    loop {
        let ev = match due {
            None => rx.recv().map_err(|e| e.to_string())?,
            Some(t) => match rx.recv_timeout(t.saturating_duration_since(Instant::now())) {
                Ok(ev) => ev,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    due = None;
                    if let Err(e) = adb_sync(ctx, None, None, None) {
                        println!("Synchronisation impossible : {e}");
                    }
                    continue;
                }
                Err(e) => return Err(e.to_string()),
            },
        };
        match ev {
            WatchEv::Devices(present) => {
                last_present.clone_from(&present);
                // Connexion en cours gardée tant qu'elle reste présente (pas de bascule USB ↔ Wi-Fi inutile).
                let ours = if connected.as_ref().is_some_and(|c| present.contains(c)) {
                    connected.clone()
                } else {
                    conf.series.iter().find(|s| present.contains(s)).cloned()
                };
                if ours != connected {
                    kill_all(&mut watchers);
                    connected = ours.clone();
                    if let Some(serial) = ours {
                        println!("{} connecté ({serial}) : synchronisation", ctx.cfg.appareil);
                        let dev = ludo_client::adb::Adb { serial: serial.clone() };
                        let (dirs, pk) = watch_dirs(ctx, &dev);
                        packages = pk;
                        // Fin de partie : mort du processus de l'émulateur (journal d'événements Android).
                        if let Some(c) = spawn_device_stream(&serial, "logcat -b events -T 1 -s am_proc_died", tx.clone(), proc_died) {
                            watchers.push(c);
                        }
                        // Écritures de sauvegarde (inotifyd de toybox : w = fichier fermé après écriture, m = renommé vers
                        // le dossier (sauvegarde écrite à côté puis renommée), n = créé). Vérifié sur la Thor le 16/11.
                        if !dirs.is_empty() {
                            let args: Vec<String> = dirs.iter().map(|d| format!("'{d}':wmn")).collect();
                            if let Some(c) = spawn_device_stream(&serial, &format!("inotifyd - {}", args.join(" ")), tx.clone(), save_event) {
                                watchers.push(c);
                            }
                        }
                        due = Some(Instant::now());
                    } else {
                        println!("{} déconnecté", ctx.cfg.appareil);
                        due = None;
                    }
                }
            }
            WatchEv::TrackerEnded => {
                // Serveur adb relancé : on se réabonne (une seule attente si adb ne répond plus du tout).
                let _ = tracker.wait();
                std::thread::sleep(Duration::from_secs(2));
                tracker = spawn_tracker(tx.clone()).map_err(|e| format!("adb track-devices : {e}"))?;
            }
            WatchEv::ProcDied(pkg) if packages.contains(&pkg) => {
                println!("{pkg} fermé : synchronisation");
                due = Some(Instant::now() + AFTER_EXIT);
            }
            WatchEv::ProcDied(_) => {}
            WatchEv::SaveWritten(dir) => {
                if due.is_none() {
                    println!("Sauvegarde modifiée ({dir}) : synchronisation après {} s sans nouvelle écriture", AFTER_WRITE.as_secs());
                }
                due = Some(Instant::now() + AFTER_WRITE);
            }
            WatchEv::NewSaveDir => {
                // Relance la surveillance pour inclure le nouveau dossier de jeu.
                connected = None;
                let _ = tx.send(WatchEv::Devices(last_present.clone()));
            }
        }
    }
}

/// Démon de surveillance des sauvegardes de cet appareil (voir `Cmd::Watch`).
fn local_watch(ctx: &mut Ctx) -> Result<ExitCode, String> {
    use notify::{RecursiveMode, Watcher};
    use std::collections::BTreeMap;
    use std::sync::mpsc;
    use std::time::Duration;
    const QUIET: Duration = Duration::from_secs(20);
    let roots = ludo_client::watch::roots(&ctx.cfg);
    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(tx).map_err(|e| format!("inotify : {e}"))?;
    let mut watched = 0;
    for r in &roots {
        if r.path.is_dir() {
            watcher.watch(&r.path, RecursiveMode::Recursive).map_err(|e| format!("{} : {e}", r.path.display()))?;
            println!("surveillé\t{}\t{}", r.emulator, r.path.display());
            watched += 1;
        }
    }
    if watched == 0 {
        return Err("aucun dossier de sauvegarde trouvé sur cet appareil".into());
    }
    // Catalogue : relu quand un titre inconnu apparaît (événement), pas à intervalle.
    let mut catalog = ctx.client.games().unwrap_or_default();
    // Anti-rebond par (profil, émulateur, titre) : échéance repoussée à chaque écriture.
    let mut due: BTreeMap<(String, String, String), Instant> = BTreeMap::new();
    let lease_device = format!("{}-ui", ctx.cfg.appareil);
    loop {
        let next = due.values().min().copied();
        let ev = match next {
            None => Some(rx.recv().map_err(|e| e.to_string())?),
            Some(t) => match rx.recv_timeout(t.saturating_duration_since(Instant::now())) {
                Ok(ev) => Some(ev),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(e) => return Err(e.to_string()),
            },
        };
        if let Some(ev) = ev {
            let Ok(ev) = ev else { continue };
            if matches!(ev.kind, notify::EventKind::Access(_)) {
                continue;
            }
            for path in &ev.paths {
                if let Some(c) = ludo_client::watch::classify(&ctx.cfg, &roots, path) {
                    for p in c.profiles {
                        due.insert((p, c.emulator.clone(), c.title_id.clone()), Instant::now() + QUIET);
                    }
                }
            }
            continue;
        }
        // Échéances atteintes : synchroniser ces jeux.
        let now = Instant::now();
        let ready: Vec<(String, String, String)> = due.iter().filter(|(_, t)| **t <= now).map(|(k, _)| k.clone()).collect();
        for k in ready {
            due.remove(&k);
            let (profile, emulator, title) = k;
            let find = |cat: &[ludo_proto::CatalogGame]| {
                cat.iter()
                    .find(|g| g.title_id.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(&title)) && ctx.cfg.game_conf(&g.id, &profile, Some(g)).is_some_and(|c| c.emulateur == emulator))
                    .cloned()
            };
            let game = match find(&catalog) {
                Some(g) => g,
                None => {
                    catalog = ctx.client.games().unwrap_or_default();
                    match find(&catalog) {
                        Some(g) => g,
                        None => {
                            println!("{title} ({emulator}) : jeu absent du catalogue, ignoré");
                            continue;
                        }
                    }
                }
            };
            let Some(conf) = ctx.cfg.game_conf(&game.id, &profile, Some(&game)) else { continue };
            let key = SaveKey { profile: profile.clone(), game: game.id.clone() };
            match ctx.client.acquire_lease(&key, &lease_device, false) {
                Ok(LeaseOutcome::HeldElsewhere { lease }) => {
                    println!("{}\tpartie en cours sur {} : synchronisée par elle", game.id, launch::holder_device(&lease.device));
                    continue;
                }
                Ok(LeaseOutcome::Granted { .. }) | Err(_) => {}
            }
            let result = (|| -> Result<SyncOutcome, String> {
                let prepared = config::prepare(&ctx.cfg, &conf.emulateur, &profile, &conf.title_id).map_err(|e| e.to_string())?;
                let playtime_s = launch::head_playtime(&ctx.client, &key).unwrap_or(0);
                let mut engine = Engine { client: &ctx.client, journal: &mut ctx.journal, device: ctx.cfg.appareil.clone(), emulator: conf.emulateur.clone(), playtime_s };
                engine.sync(&key, prepared.layout.as_ref()).map_err(|e| e.to_string())
            })();
            let _ = ctx.client.release_lease(&key, &lease_device);
            match result {
                Ok(o) => println!("{}\t{}\t{}", game.id, profile, label(&o)),
                Err(e) => println!("{}\t{}\tÉchec : {e}", game.id, profile),
            }
        }
    }
}

/// Horodatage pour nommer les dossiers mis de côté (`.ludo-old-20261116-0712`).
fn chrono_stamp() -> String {
    let out = std::process::Command::new("date").arg("+%Y%m%d-%H%M%S").output();
    out.ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "ancien".into())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let path = cli.config.unwrap_or_else(Config::default_path);
    let result = (|| -> Result<ExitCode, String> {
        let cfg = Config::load(&path).map_err(|e| format!("{} : {e}", path.display()))?;
        let client = Client::new(&cfg.serveur);
        let journal = Journal::open(&config::journal_path()).map_err(|e| e.to_string())?;
        let mut ctx = Ctx { cfg, client, journal };
        match cli.cmd {
            Cmd::Sync(jeu) => {
                let key = key_for(&ctx, &jeu)?;
                let o = sync_once(&mut ctx, &jeu, &key, 0)?;
                println!("{}", label(&o));
                Ok(exit_for(&o).unwrap_or(ExitCode::SUCCESS))
            }
            Cmd::Run { jeu, rom, jouer_ici, via, lanceur } => run(&mut ctx, &jeu, &rom, jouer_ici, via.as_deref(), lanceur.as_deref()),
            Cmd::AdbSync { profil, jeu, resoudre } => adb_sync(&ctx, profil, jeu, resoudre),
            Cmd::AdbWatch => adb_watch(&ctx),
            Cmd::Watch => local_watch(&mut ctx),
            Cmd::Jeux => {
                let local = ludo_client::local::LocalIndex::scan(&ctx.cfg, &ludo_client::detect::Host::current());
                for (c, d) in &local.roots {
                    println!("dossier parcouru\t{c}\t{}", d.display());
                }
                let games = ctx.client.games().map_err(|e| e.to_string())?;
                let (mut present, mut total) = (0, 0);
                for g in &games {
                    for f in &g.files {
                        total += 1;
                        match local.locate(ctx.cfg.roms.as_deref(), &f.path) {
                            Some(p) => {
                                present += 1;
                                println!("{}\t{:?}\tprésent\t{}", g.id, f.role, p.display());
                            }
                            None => println!("{}\t{:?}\tabsent\t{}", g.id, f.role, f.path),
                        }
                    }
                }
                println!("{present}/{total} fichiers du catalogue présents sur l'appareil");
                Ok(ExitCode::SUCCESS)
            }
            Cmd::Emulateurs => {
                let host = ludo_client::detect::Host::current();
                for id in ctx.cfg.fiche_ids() {
                    let fiche = ctx.cfg.fiche(&id).map_err(|e| format!("{id} : {e}"))?;
                    let defaut: Vec<&str> =
                        fiche.consoles.iter().filter(|c| ctx.cfg.default_emulator(c).as_deref() == Some(id.as_str())).map(String::as_str).collect();
                    let defaut = if defaut.is_empty() { String::new() } else { format!(" (par défaut : {})", defaut.join(", ")) };
                    match ctx.cfg.locate(&fiche, &host) {
                        Ok(l) => {
                            let origine = match l.origin {
                                config::Origin::Config => "réglage de l'appareil".to_string(),
                                config::Origin::Detected(s) => s,
                                config::Origin::Fiche => "chemin de la fiche (non vérifié)".to_string(),
                            };
                            println!("{id}{defaut}\t{origine}\n\tlancement : {}\n\tdonnées : {}", l.bin, l.data.display());
                        }
                        Err(e) => println!("{id}{defaut}\tintrouvable ({e})"),
                    }
                }
                Ok(ExitCode::SUCCESS)
            }
            Cmd::Statut => {
                for (k, e) in ctx.journal.entries() {
                    let etat = if e.conflict {
                        "Conflit de sauvegarde"
                    } else if e.pending_upload {
                        "Envoi en attente"
                    } else {
                        "À jour"
                    };
                    println!("{k}\t{etat}\trévision {}", e.base_rev);
                }
                Ok(ExitCode::SUCCESS)
            }
        }
    })();
    result.unwrap_or_else(|e| {
        eprintln!("{e}");
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests_sunshine {
    use super::*;

    #[test]
    fn appli_sunshine_par_jeu_ludo_run_prioritaire() {
        let json = r#"{"apps":[
            {"name":"Bureau","cmd":""},
            {"name":"Mario Kart 8 Deluxe","cmd":"env X=1 /bin/ryu-stream \"/mnt/Rom/Mario Kart 8 Deluxe [0100152000022000][v0].nsp\""},
            {"name":"Mario Kart 8 Deluxe · Ludothèque","cmd":"/bin/ludo run --profil g --jeu switch-0100152000022000 --rom '/mnt/Rom/Mario Kart 8 Deluxe [0100152000022000][v0].nsp'"},
            {"name":"Pokopia","cmd":"/bin/lichii-stream \"/mnt/Rom2/Pokemon Pokopia [0400A3A01EE4C000]/updates/327680/exefs/main\""}
        ]}"#;
        let apps = sunshine_apps(json);
        assert_eq!(apps.by_game.get("switch-0100152000022000").map(String::as_str), Some("Mario Kart 8 Deluxe · Ludothèque"));
        let mk8 = ludo_client::local::match_key("Mario Kart 8 Deluxe [0100152000022000][v0].nsp");
        assert_eq!(apps.by_rom.get(&mk8).map(String::as_str), Some("Mario Kart 8 Deluxe · Ludothèque"));
        let pokopia = ludo_client::local::match_key("Pokemon Pokopia [0400A3A01EE4C000]");
        assert_eq!(apps.by_rom.get(&pokopia).map(String::as_str), Some("Pokopia"));
        assert_eq!(sunshine_apps("pas du json"), SunshineApps::default());
    }
}

#[cfg(test)]
mod tests_evenements {
    use super::*;

    #[test]
    fn mort_de_processus() {
        let l = "11-16 21:39:17.846  2143  8889 I am_proc_died: [0,18736,dev.eden.eden_emulator,905,19]";
        assert!(matches!(proc_died(l), Some(WatchEv::ProcDied(p)) if p == "dev.eden.eden_emulator"));
        assert!(proc_died("--------- beginning of events").is_none());
    }

    #[test]
    fn evenements_inotifyd() {
        let root = "/sdcard/Android/data/dev.eden.eden_emulator/files/nand/user/save/0000000000000000/A2D2DFD2700F51C158E87A336C77FE55";
        // Écriture dans le dossier d'un jeu.
        assert!(matches!(save_event(&format!("w\t{root}/0100152000022000\tuserdata.dat")), Some(WatchEv::SaveWritten(_))));
        // Création d'un dossier de jeu dans la racine du profil : surveillance à relancer.
        assert!(matches!(save_event(&format!("n\t{root}\t0100ABCD00000000")), Some(WatchEv::NewSaveDir)));
        assert!(save_event("ligne-incomplète").is_none());
    }
}
