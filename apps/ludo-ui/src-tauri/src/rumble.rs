//! Vibrations de la manette, gérées nativement (gilrs + retour de force evdev sous Linux).
//!
//! L'API Gamepad du moteur web ne fait pas vibrer de façon fiable (WebKitGTK, iframe du
//! navigateur) : l'interface envoie donc ses motifs ici. Un fil dédié possède `Gilrs` (qui
//! n'est pas `Send`), garde les effets en vie le temps qu'ils jouent, et suit le branchement
//! des manettes.

use gilrs::ff::{BaseEffect, BaseEffectType, Effect, EffectBuilder, Replay, Ticks};
use gilrs::Gilrs;
use serde::Deserialize;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Une impulsion : durée et délai en millisecondes, intensités entre 0 et 1.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub struct Pulse {
    pub duration: u32,
    #[serde(default)]
    pub delay: u32,
    #[serde(default)]
    pub weak: f32,
    #[serde(default)]
    pub strong: f32,
}

/// Intensité 0..1 vers l'échelle 0..65535 de gilrs.
pub fn magnitude(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16
}

/// Durée totale d'un motif (fin de la dernière impulsion).
pub fn pattern_length(pulses: &[Pulse]) -> u32 {
    pulses.iter().map(|p| p.delay + p.duration).max().unwrap_or(0)
}

fn worker(rx: Receiver<Vec<Pulse>>) {
    let Ok(mut gilrs) = Gilrs::new() else {
        eprintln!("ludo-ui : manettes indisponibles (gilrs), vibrations désactivées");
        return;
    };
    let mut alive: Vec<(Effect, Instant)> = Vec::new();
    loop {
        // Événementiel : on dort jusqu'à la prochaine demande ; s'il reste des effets en cours, au plus
        // jusqu'à la fin du premier (échéance calculée, pas de réveil périodique).
        let next = alive.iter().map(|(_, until)| *until).min();
        let got = match next {
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Some(t) => rx.recv_timeout(t.saturating_duration_since(Instant::now())),
        };
        // Branchements / débranchements des manettes : file d'événements gilrs vidée au réveil.
        while gilrs.next_event().is_some() {}
        match got {
            Ok(pulses) => {
                let pads: Vec<_> = gilrs.gamepads().filter(|(_, g)| g.is_ff_supported()).map(|(id, _)| id).collect();
                if pads.is_empty() || pulses.is_empty() {
                    continue;
                }
                let mut b = EffectBuilder::new();
                for p in &pulses {
                    let scheduling = Replay {
                        play_for: Ticks::from_ms(p.duration),
                        with_delay: Ticks::from_ms(p.delay),
                        ..Default::default()
                    };
                    if p.strong > 0.0 {
                        b.add_effect(BaseEffect { kind: BaseEffectType::Strong { magnitude: magnitude(p.strong) }, scheduling, envelope: Default::default() });
                    }
                    if p.weak > 0.0 {
                        b.add_effect(BaseEffect { kind: BaseEffectType::Weak { magnitude: magnitude(p.weak) }, scheduling, envelope: Default::default() });
                    }
                }
                match b.gamepads(&pads).finish(&mut gilrs) {
                    Ok(effect) => {
                        if effect.play().is_ok() {
                            let until = Instant::now() + Duration::from_millis(u64::from(pattern_length(&pulses)) + 50);
                            alive.push((effect, until));
                        }
                    }
                    Err(e) => eprintln!("ludo-ui : vibration refusée ({e})"),
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        // Libère les effets terminés.
        let now = Instant::now();
        alive.retain(|(_, until)| *until > now);
    }
}

fn sender() -> &'static Mutex<Sender<Vec<Pulse>>> {
    static TX: OnceLock<Mutex<Sender<Vec<Pulse>>>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = channel();
        std::thread::Builder::new().name("ludo-rumble".into()).spawn(move || worker(rx)).expect("fil des vibrations");
        Mutex::new(tx)
    })
}

/// Démarre le fil des vibrations (à l'ouverture de l'app, pour détecter les manettes tôt).
pub fn init() {
    let _ = sender();
}

/// Motifs de l'interface (`feedback.ts`). PC et Deck : manette par gilrs. Android : moteur de vibration
/// de l'appareil (`Vibrator`, LudoPlugin.kt) ; gilrs n'y pilote rien (la manette Odin de la Thor est
/// intégrée, son moteur est celui de l'appareil : réglage AYN « Vibrator strength »).
/// Commande asynchrone : une commande synchrone de Tauri tourne sur le fil principal, et l'appel au
/// plugin Kotlin (Android) attend sa réponse.
#[tauri::command]
pub async fn rumble(app: tauri::AppHandle, pulses: Vec<Pulse>) {
    #[cfg(target_os = "android")]
    {
        crate::android::vibrer(&app, &vibration_pattern(&pulses));
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        if let Ok(tx) = sender().lock() {
            let _ = tx.send(pulses);
        }
    }
}

/// Forme d'onde pour `VibrationEffect.createWaveform` : durées alternées (pause, vibration…) en ms et
/// amplitude de chaque segment (0 = arrêt, 1..255).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct VibrationPattern {
    pub timings: Vec<i64>,
    pub amplitudes: Vec<i32>,
}

/// Durée minimale d'une impulsion sur un moteur d'appareil : en dessous, le moteur n'a pas le temps de
/// démarrer (les motifs de 20 à 24 ms de la manette ne se sentiraient pas). Valeur de départ, ressenti
/// non mesuré.
pub const MIN_MOTEUR_MS: u32 = 30;

/// Motif de manette (deux moteurs, intensités 0..1) → forme d'onde d'un seul moteur : impulsions mises
/// bout à bout (un chevauchement est rogné), intensité = la plus forte des deux, rehaussée (0,3 + 0,7 × v)
/// pour rester perceptible sur un petit moteur.
pub fn vibration_pattern(pulses: &[Pulse]) -> VibrationPattern {
    let mut sorted: Vec<&Pulse> = pulses.iter().filter(|p| p.duration > 0 && (p.weak > 0.0 || p.strong > 0.0)).collect();
    sorted.sort_by_key(|p| p.delay);
    let mut out = VibrationPattern::default();
    let mut t: u32 = 0;
    for p in sorted {
        let end = p.delay + p.duration.max(MIN_MOTEUR_MS);
        if end <= t {
            continue;
        }
        if p.delay > t {
            out.timings.push(i64::from(p.delay - t));
            out.amplitudes.push(0);
            t = p.delay;
        }
        let v = p.weak.max(p.strong).clamp(0.0, 1.0);
        out.timings.push(i64::from(end - t));
        out.amplitudes.push(((0.3 + 0.7 * v) * 255.0).round().clamp(1.0, 255.0) as i32);
        t = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forme_d_onde_android() {
        // move : une impulsion courte, rallongée au minimum du moteur.
        let m = vibration_pattern(&[Pulse { duration: 24, delay: 0, weak: 0.3, strong: 0.0 }]);
        assert_eq!(m, VibrationPattern { timings: vec![30], amplitudes: vec![130] });
        // err : deux impulsions séparées d'une pause.
        let e = vibration_pattern(&[Pulse { duration: 45, delay: 0, weak: 0.0, strong: 0.32 }, Pulse { duration: 45, delay: 100, weak: 0.0, strong: 0.26 }]);
        assert_eq!(e.timings, vec![45, 55, 45]);
        assert_eq!(e.amplitudes[1], 0);
        assert!(e.amplitudes[0] > e.amplitudes[2]);
        // Chevauchement rogné, impulsion vide ignorée, ordre rétabli.
        let o = vibration_pattern(&[
            Pulse { duration: 40, delay: 20, weak: 1.0, strong: 0.0 },
            Pulse { duration: 50, delay: 0, weak: 0.5, strong: 0.0 },
            Pulse { duration: 50, delay: 0, weak: 0.0, strong: 0.0 },
        ]);
        assert_eq!(o.timings, vec![50, 10]);
        assert_eq!(o.amplitudes, vec![166, 255]);
        assert_eq!(vibration_pattern(&[]), VibrationPattern::default());
    }

    #[test]
    fn echelle_des_intensites() {
        assert_eq!(magnitude(0.0), 0);
        assert_eq!(magnitude(1.0), u16::MAX);
        assert_eq!(magnitude(2.0), u16::MAX);
        assert_eq!(magnitude(-1.0), 0);
    }

    #[test]
    fn longueur_d_un_motif() {
        let p = [
            Pulse { duration: 30, delay: 0, weak: 0.2, strong: 0.0 },
            Pulse { duration: 90, delay: 285, weak: 0.3, strong: 0.15 },
        ];
        assert_eq!(pattern_length(&p), 375);
        assert_eq!(pattern_length(&[]), 0);
    }
}
