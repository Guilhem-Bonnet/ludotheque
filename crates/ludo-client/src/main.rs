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
    let argv = config::launch_command(&prepared, rom).map_err(|e| e.to_string())?;
    let (bin, args) = argv.split_first().ok_or("commande de lancement vide")?;
    let started = Instant::now();
    // Le verrou expire au bout de 120 s : renouvelé pendant toute la partie (comme l'app).
    let stop = std::sync::atomic::AtomicBool::new(false);
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
            let installed = g.files.iter().any(|f| roms.contains(&ludo_client::local::match_key(&f.path)));
            let stream = g.files.iter().any(|f| streamable.contains(&ludo_client::local::match_key(&f.path)));
            (g.id.clone(), ludo_proto::DeviceGame { game: g.id.clone(), installed, stream, save: false })
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
        let has_rom = g.files.iter().any(|f| roms.contains(&ludo_client::local::match_key(&f.path)));
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

/// ROM jouables en flux depuis ce PC : chemins cités dans les commandes des applis Sunshine
/// (`~/.config/sunshine/apps.json`), réduits à leur clé de rapprochement (nom, format équivalent).
fn sunshine_roms() -> std::collections::BTreeSet<(String, String)> {
    let path = config::home().join(".config/sunshine/apps.json");
    let Ok(text) = std::fs::read_to_string(path) else { return Default::default() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return Default::default() };
    let mut out = std::collections::BTreeSet::new();
    for app in v.get("apps").and_then(|a| a.as_array()).into_iter().flatten() {
        let cmd = app.get("cmd").and_then(|c| c.as_str()).unwrap_or("");
        // Chemins entre guillemets doubles ou simples dans la commande.
        for quote in ['"', '\'']
        {
            let mut parts = cmd.split(quote);
            parts.next();
            while let Some(inside) = parts.next() {
                if inside.starts_with('/') {
                    // Le chemin et ses dossiers parents : un jeu en dossier (Switch 2 extrait) est
                    // souvent cité par un fichier intérieur (`…/Pokemon Pokopia [id]/updates/…`).
                    for anc in std::path::Path::new(inside).ancestors().take(6) {
                        if let Some(n) = anc.file_name() {
                            out.insert(ludo_client::local::match_key(&n.to_string_lossy()));
                        }
                    }
                }
                parts.next();
            }
        }
    }
    out
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
