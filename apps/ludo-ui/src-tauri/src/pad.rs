//! Navigation à la manette, lue nativement (bureau).
//!
//! Pourquoi pas l'API Gamepad du moteur web : sous Linux, WebKitGTK passe par libmanette, qui ne
//! traduit la croix en boutons que pour les manettes de sa base de correspondances. Mesuré le
//! 21/11 sur une manette virtuelle aux identifiants de la manette du PC (GameSir 3537:10c8, nom
//! « Generic X-Box pad ») : libmanette n'émet qu'un `hat-axis-event`, que l'API Gamepad n'expose
//! pas en boutons 12-15 ; gilrs, lui, la traduit en `DPadUp/Down/Left/Right`. De plus, l'API
//! Gamepad n'a aucun événement de bouton : il fallait la relire à chaque image.
//!
//! Ici, un fil dédié possède `Gilrs` (qui n'est pas `Send`) et **attend** les événements
//! (`next_event_blocking(None)` = epoll sur les périphériques evdev et sur le branchement à
//! chaud) : aucun réveil périodique. Chaque appui est traduit en touche de l'interface et envoyé
//! sous l'événement [`EVENT_PAD`], **seulement si la fenêtre a le focus** (un appui pendant un
//! jeu n'agit jamais sur la Ludothèque).

use gilrs::{Axis, Button, EventType, Gilrs};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Événement envoyé à l'interface : `{ key, pressed }`.
pub const EVENT_PAD: &str = "ludo://manette";
/// Événement envoyé à l'interface quand la fenêtre gagne (`true`) ou perd (`false`) le focus.
pub const EVENT_FOCUS: &str = "ludo://focus";

/// Touches de l'interface (mêmes noms que la barre d'aide et que nav.ts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Key {
    A,
    B,
    X,
    Y,
    L1,
    R1,
    #[serde(rename = "up")]
    Up,
    #[serde(rename = "down")]
    Down,
    #[serde(rename = "left")]
    Left,
    #[serde(rename = "right")]
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PadEvent {
    pub key: Key,
    pub pressed: bool,
}

/// La fenêtre a-t-elle le focus ? Mis à jour par `WindowEvent::Focused` (lib.rs).
static FOCUSED: AtomicBool = AtomicBool::new(true);

pub fn set_focus(focused: bool) {
    FOCUSED.store(focused, Ordering::Relaxed);
}

pub fn focused() -> bool {
    FOCUSED.load(Ordering::Relaxed)
}

/// Seuils du stick (hystérésis) : appui au-delà de 0,6, relâchement en deçà de 0,4.
const STICK_ON: f32 = 0.6;
const STICK_OFF: f32 = 0.4;

/// Traduit les événements gilrs d'UNE manette en touches ; garde l'état du stick gauche (direction
/// par axe) et des touches de croix tenues, pour tout relâcher au débranchement (gilrs n'émet alors
/// aucun `ButtonReleased`).
#[derive(Debug, Default, Clone)]
pub struct Mapper {
    x: i8,
    y: i8,
    dpad: Vec<Key>,
}

fn button(b: Button) -> Option<Key> {
    Some(match b {
        Button::South => Key::A,
        Button::East => Key::B,
        Button::West => Key::X,
        Button::North => Key::Y,
        Button::LeftTrigger => Key::L1,
        Button::RightTrigger => Key::R1,
        Button::DPadUp => Key::Up,
        Button::DPadDown => Key::Down,
        Button::DPadLeft => Key::Left,
        Button::DPadRight => Key::Right,
        _ => return None,
    })
}

/// Nouvel état d'un axe du stick (-1, 0, 1) selon la valeur lue et l'état précédent.
fn stick_state(v: f32, old: i8) -> i8 {
    if v > STICK_ON {
        1
    } else if v < -STICK_ON {
        -1
    } else if v.abs() < STICK_OFF {
        0
    } else {
        old
    }
}

/// Changement d'état d'un axe → relâchement de l'ancienne direction, appui de la nouvelle.
fn transition(out: &mut Vec<PadEvent>, old: i8, new: i8, neg: Key, pos: Key) {
    if old == new {
        return;
    }
    if old != 0 {
        out.push(PadEvent { key: if old < 0 { neg } else { pos }, pressed: false });
    }
    if new != 0 {
        out.push(PadEvent { key: if new < 0 { neg } else { pos }, pressed: true });
    }
}

/// Ce que la navigation retient d'un événement gilrs (le code matériel est ignoré).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    Button(Button, bool),
    Axis(Axis, f32),
    Disconnected,
}

impl Input {
    pub fn from_gilrs(ev: &EventType) -> Option<Input> {
        Some(match *ev {
            EventType::ButtonPressed(b, _) => Input::Button(b, true),
            EventType::ButtonReleased(b, _) => Input::Button(b, false),
            EventType::AxisChanged(a, v, _) => Input::Axis(a, v),
            EventType::Disconnected => Input::Disconnected,
            _ => return None,
        })
    }
}

impl Mapper {
    /// Ce que `map` produirait, sans changer l'état (diagnostic).
    pub fn peek(&self, ev: &Input) -> Vec<PadEvent> {
        self.clone().map(ev)
    }

    pub fn map(&mut self, ev: &Input) -> Vec<PadEvent> {
        let mut out = Vec::new();
        match *ev {
            Input::Button(b, pressed) => {
                if let Some(key) = button(b) {
                    if matches!(key, Key::Up | Key::Down | Key::Left | Key::Right) {
                        self.dpad.retain(|k| *k != key);
                        if pressed {
                            self.dpad.push(key);
                        }
                    }
                    out.push(PadEvent { key, pressed });
                }
            }
            Input::Axis(Axis::LeftStickX, v) => {
                let new = stick_state(v, self.x);
                transition(&mut out, self.x, new, Key::Left, Key::Right);
                self.x = new;
            }
            // gilrs : Y positif = haut (inverse d'evdev).
            Input::Axis(Axis::LeftStickY, v) => {
                let new = stick_state(v, self.y);
                transition(&mut out, self.y, new, Key::Down, Key::Up);
                self.y = new;
            }
            // Manette débranchée : croix et stick tenus sont relâchés (sinon répétition sans fin).
            Input::Disconnected => {
                out.extend(self.dpad.drain(..).map(|key| PadEvent { key, pressed: false }));
                transition(&mut out, self.x, 0, Key::Left, Key::Right);
                transition(&mut out, self.y, 0, Key::Down, Key::Up);
                self.x = 0;
                self.y = 0;
            }
            _ => {}
        }
        out
    }
}

/// Appuis à transmettre : aucun si la fenêtre n'a pas le focus (l'état du stick reste suivi, pour
/// qu'un stick relâché pendant un jeu ne laisse pas une direction « tenue » au retour).
pub fn dispatch(mapper: &mut Mapper, ev: &Input, focused: bool) -> Vec<PadEvent> {
    let out = mapper.map(ev);
    if focused { out } else { Vec::new() }
}

/// Démarre le fil de lecture. `emit` reçoit chaque appui à transmettre (fenêtre au premier plan).
pub fn init<F: Fn(PadEvent) + Send + 'static>(emit: F) {
    let spawned = std::thread::Builder::new().name("ludo-manette".into()).spawn(move || {
        let Ok(mut gilrs) = Gilrs::new() else {
            eprintln!("ludo-ui : manettes indisponibles (gilrs), navigation à la manette désactivée");
            return;
        };
        // Un état par manette : le débranchement de l'une ne relâche pas ce que tient l'autre.
        let mut mappers: std::collections::HashMap<gilrs::GamepadId, Mapper> = std::collections::HashMap::new();
        // Diagnostic : `LUDO_PAD_TRACE=1` affiche chaque appui lu et s'il est transmis à l'interface.
        let trace = std::env::var_os("LUDO_PAD_TRACE").is_some();
        // Sans délai, `next_event_blocking(None)` ne rend `None` que sur une anomalie. Mesuré le 21/11 :
        // au débranchement d'une manette, gilrs ignore l'EPOLLHUP du périphérique tant que udev n'a pas
        // confirmé le retrait (≈ 1,5 s) et rend `None` en rafale (≈ 8 500 fois). Pendant ce trou : attente
        // de repli de 10 ms (exception « source d'événement tombée » de CLAUDE.md), jamais une boucle à vide ;
        // au-delà de 30 s d'anomalie continue (epoll en échec), gilrs est recréé.
        let mut empty_streak = 0u32;
        loop {
            match gilrs.next_event_blocking(None) {
                Some(ev) => {
                    empty_streak = 0;
                    let Some(input) = Input::from_gilrs(&ev.event) else { continue };
                    let focus = focused();
                    let mapper = mappers.entry(ev.id).or_default();
                    if trace {
                        for e in mapper.peek(&input) {
                            eprintln!("ludo-ui manette : {e:?} {}", if focus { "transmis" } else { "ignoré (fenêtre sans focus)" });
                        }
                    }
                    for e in dispatch(mapper, &input, focus) {
                        emit(e);
                    }
                }
                None => {
                    empty_streak += 1;
                    if empty_streak > 20 {
                        if empty_streak > 20 + 3000 {
                            // 30 s d'anomalie continue : lecteur recréé (manettes rouvertes) plutôt qu'abandonné.
                            eprintln!("ludo-ui : lecture des manettes en échec (epoll), relance de gilrs");
                            match Gilrs::new() {
                                Ok(g) => gilrs = g,
                                Err(_) => {
                                    eprintln!("ludo-ui : manettes indisponibles (gilrs), navigation à la manette désactivée");
                                    return;
                                }
                            }
                            mappers.clear();
                            empty_streak = 0;
                            continue;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
            }
        }
    });
    if let Err(e) = spawned {
        eprintln!("ludo-ui : fil des manettes non démarré : {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn croix_et_boutons() {
        let mut m = Mapper::default();
        assert_eq!(m.map(&Input::Button(Button::DPadRight, true)), vec![PadEvent { key: Key::Right, pressed: true }]);
        assert_eq!(m.map(&Input::Button(Button::DPadRight, false)), vec![PadEvent { key: Key::Right, pressed: false }]);
        assert_eq!(m.map(&Input::Button(Button::South, true)), vec![PadEvent { key: Key::A, pressed: true }]);
        assert_eq!(m.map(&Input::Button(Button::LeftTrigger, true)), vec![PadEvent { key: Key::L1, pressed: true }]);
        assert!(m.map(&Input::Button(Button::LeftTrigger2, true)).is_empty());
    }

    #[test]
    fn stick_avec_hysteresis() {
        let mut m = Mapper::default();
        assert!(m.map(&Input::Axis(Axis::LeftStickX, 0.5)).is_empty());
        assert_eq!(m.map(&Input::Axis(Axis::LeftStickX, 0.8)), vec![PadEvent { key: Key::Right, pressed: true }]);
        // Entre 0,4 et 0,6 : la direction reste tenue.
        assert!(m.map(&Input::Axis(Axis::LeftStickX, 0.5)).is_empty());
        assert_eq!(m.map(&Input::Axis(Axis::LeftStickX, 0.1)), vec![PadEvent { key: Key::Right, pressed: false }]);
        // Passage direct de droite à gauche : relâchement puis appui.
        m.map(&Input::Axis(Axis::LeftStickX, 0.9));
        assert_eq!(
            m.map(&Input::Axis(Axis::LeftStickX, -0.9)),
            vec![PadEvent { key: Key::Right, pressed: false }, PadEvent { key: Key::Left, pressed: true }]
        );
        // Y positif = haut.
        assert_eq!(m.map(&Input::Axis(Axis::LeftStickY, 0.9)), vec![PadEvent { key: Key::Up, pressed: true }]);
    }

    #[test]
    fn debranchement_relache_tout() {
        let mut m = Mapper::default();
        m.map(&Input::Axis(Axis::LeftStickX, -0.9));
        m.map(&Input::Axis(Axis::LeftStickY, -0.9));
        assert_eq!(
            m.map(&Input::Disconnected),
            vec![PadEvent { key: Key::Left, pressed: false }, PadEvent { key: Key::Down, pressed: false }]
        );
    }

    #[test]
    fn serialisation_attendue_par_nav_ts() {
        let j = serde_json::to_string(&PadEvent { key: Key::Up, pressed: true }).unwrap();
        assert_eq!(j, r#"{"key":"up","pressed":true}"#);
        let j = serde_json::to_string(&PadEvent { key: Key::L1, pressed: false }).unwrap();
        assert_eq!(j, r#"{"key":"L1","pressed":false}"#);
    }

    #[test]
    fn debranchement_croix_tenue() {
        let mut m = Mapper::default();
        m.map(&Input::Button(Button::DPadRight, true));
        m.map(&Input::Button(Button::DPadUp, true));
        m.map(&Input::Button(Button::DPadUp, false));
        assert_eq!(m.map(&Input::Disconnected), vec![PadEvent { key: Key::Right, pressed: false }]);
        assert!(m.map(&Input::Disconnected).is_empty());
    }

    #[test]
    fn rien_sans_focus() {
        let mut m = Mapper::default();
        assert!(dispatch(&mut m, &Input::Button(Button::South, true), false).is_empty());
        assert!(dispatch(&mut m, &Input::Axis(Axis::LeftStickX, 0.9), false).is_empty());
        // Retour du focus, stick relâché entre-temps : aucun relâchement fantôme, pas d'appui tenu.
        assert!(dispatch(&mut m, &Input::Axis(Axis::LeftStickX, 0.0), false).is_empty());
        assert_eq!(dispatch(&mut m, &Input::Button(Button::DPadUp, true), true), vec![PadEvent { key: Key::Up, pressed: true }]);
    }

    #[test]
    fn focus() {
        set_focus(false);
        assert!(!focused());
        set_focus(true);
        assert!(focused());
    }
}
