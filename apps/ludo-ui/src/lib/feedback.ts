// Retour sensoriel unifié : chaque événement de l'interface a son son ET sa vibration,
// déclenchés ensemble pour qu'ils tombent sur la même image que l'animation.

import { play, type SoundName } from './sound';

type Rumble = { duration: number; weak: number; strong: number; delay?: number };

// Vibrations de la manette (Gamepad API, effet « dual-rumble »). Discrètes : un toucher, pas un choc.
// Intensités relevées le 15/11 : en dessous de ~0,25, le petit moteur d'une manette Xbox ne se sent pas.
const RUMBLE: Partial<Record<SoundName, Rumble[]>> = {
  move: [{ duration: 24, weak: 0.3, strong: 0 }],
  tab: [{ duration: 20, weak: 0.32, strong: 0 }],
  ok: [{ duration: 40, weak: 0.42, strong: 0.16 }],
  back: [{ duration: 28, weak: 0.3, strong: 0 }],
  err: [{ duration: 45, weak: 0, strong: 0.32 }, { duration: 45, weak: 0, strong: 0.26, delay: 100 }],
  saved: [{ duration: 70, weak: 0.32, strong: 0.08 }],
  conflict: [{ duration: 90, weak: 0.22, strong: 0.24 }],
  arrive: [{ duration: 60, weak: 0.36, strong: 0.1 }],
  launch: [
    { duration: 34, weak: 0.26, strong: 0 }, { duration: 34, weak: 0.3, strong: 0, delay: 95 },
    { duration: 34, weak: 0.34, strong: 0.06, delay: 190 }, { duration: 110, weak: 0.4, strong: 0.18, delay: 285 },
  ],
  downloaded: [{ duration: 60, weak: 0.34, strong: 0.1 }],
  updated: [{ duration: 60, weak: 0.34, strong: 0.1 }],
  syncStart: [{ duration: 30, weak: 0.3, strong: 0 }],
};

type Actuator = { playEffect?: (type: string, p: Record<string, number>) => Promise<unknown> };

const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

function rumble(steps: Rumble[]) {
  if (inTauri) {
    // Vibrations natives (gilrs côté Rust) : fiables, contrairement à l'API Gamepad de WebKitGTK.
    void import('@tauri-apps/api/core')
      .then(({ invoke }) => invoke('rumble', { pulses: steps }))
      .catch(() => {});
    return;
  }
  for (const pad of navigator.getGamepads?.() ?? []) {
    const act = (pad as unknown as { vibrationActuator?: Actuator } | null)?.vibrationActuator;
    if (!act?.playEffect) continue;
    for (const s of steps) {
      void act
        .playEffect('dual-rumble', { startDelay: s.delay ?? 0, duration: s.duration, weakMagnitude: s.weak, strongMagnitude: s.strong })
        .catch(() => {});
    }
  }
}

let rumbleOn = true;
/** Préférence du profil : vibrations de la manette activées ou non. */
export function setRumble(on: boolean) {
  rumbleOn = on;
}

export function feedback(event: SoundName) {
  play(event);
  const r = RUMBLE[event];
  if (r && rumbleOn) rumble(r);
}
