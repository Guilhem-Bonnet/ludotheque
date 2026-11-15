//! Diagnostic : liste les manettes, indique si elles acceptent les vibrations, puis joue
//! deux impulsions courtes. `cargo run -p ludo-ui --example manettes`
use gilrs::ff::{BaseEffect, BaseEffectType, EffectBuilder, Replay, Ticks};
use gilrs::Gilrs;

fn main() {
    let mut gilrs = Gilrs::new().expect("gilrs");
    let mut ids = Vec::new();
    for (id, g) in gilrs.gamepads() {
        println!("{} | connectée={} | vibrations={} | uuid={:?}", g.name(), g.is_connected(), g.is_ff_supported(), g.uuid());
        if g.is_ff_supported() {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        println!("aucune manette compatible vibrations");
        return;
    }
    let pulse = |delay: u32, strong: u16, weak: u16| {
        let s = Replay { play_for: Ticks::from_ms(160), with_delay: Ticks::from_ms(delay), ..Default::default() };
        [
            BaseEffect { kind: BaseEffectType::Strong { magnitude: strong }, scheduling: s, envelope: Default::default() },
            BaseEffect { kind: BaseEffectType::Weak { magnitude: weak }, scheduling: s, envelope: Default::default() },
        ]
    };
    let mut b = EffectBuilder::new();
    for e in pulse(0, 20_000, 30_000).into_iter().chain(pulse(320, 30_000, 40_000)) {
        b.add_effect(e);
    }
    let effect = b.gamepads(&ids).finish(&mut gilrs).expect("effet");
    effect.play().expect("lecture");
    std::thread::sleep(std::time::Duration::from_millis(700));
    println!("deux impulsions envoyées");
}
