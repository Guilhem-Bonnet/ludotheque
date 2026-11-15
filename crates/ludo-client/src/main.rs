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
    },
    /// État des sauvegardes connues de cet appareil.
    Statut,
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

fn run(ctx: &mut Ctx, jeu: &Jeu, rom: &std::path::Path, jouer_ici: bool) -> Result<ExitCode, String> {
    let key = key_for(ctx, jeu)?;
    let device = ctx.cfg.appareil.clone();

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

    let prepared = config::prepare(&ctx.cfg, &jeu.emulateur, &key.profile, &jeu.title_id).map_err(|e| e.to_string())?;
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
        std::process::Command::new(bin).args(args).status()
    })
    .map_err(|e| format!("Échec du lancement : {e}"))?;
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
            Cmd::Run { jeu, rom, jouer_ici } => run(&mut ctx, &jeu, &rom, jouer_ici),
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
