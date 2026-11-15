// Retour sensoriel unifié : chaque événement de l'interface a son son ET sa vibration,
// déclenchés ensemble pour qu'ils tombent sur la même image que l'animation.

import { play, type SoundName } from './sound';

type Rumble = { duration: number; weak: number; strong: number; delay?: number };

// Vibrations de la manette (Gamepad API, effet « dual-rumble »). Discrètes : un toucher, pas un choc.
const RUMBLE: Partial<Record<SoundName, Rumble[]>> = {
  move: [{ duration: 14, weak: 0.18, strong: 0 }],
  tab: [{ duration: 12, weak: 0.22, strong: 0 }],
  ok: [{ duration: 28, weak: 0.3, strong: 0.12 }],
  back: [{ duration: 18, weak: 0.18, strong: 0 }],
  err: [{ duration: 40, weak: 0, strong: 0.25 }, { duration: 40, weak: 0, strong: 0.2, delay: 90 }],
  saved: [{ duration: 60, weak: 0.2, strong: 0.05 }],
  conflict: [{ duration: 80, weak: 0.1, strong: 0.2 }],
  arrive: [{ duration: 50, weak: 0.25, strong: 0.08 }],
  launch: [
    { duration: 30, weak: 0.15, strong: 0 }, { duration: 30, weak: 0.2, strong: 0, delay: 95 },
    { duration: 30, weak: 0.25, strong: 0.05, delay: 190 }, { duration: 90, weak: 0.3, strong: 0.15, delay: 285 },
  ],
  downloaded: [{ duration: 50, weak: 0.25, strong: 0.08 }],
  updated: [{ duration: 50, weak: 0.25, strong: 0.08 }],
};

type Actuator = { playEffect?: (type: string, p: Record<string, number>) => Promise<unknown> };

function rumble(steps: Rumble[]) {
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

export function feedback(event: SoundName) {
  play(event);
  const r = RUMBLE[event];
  if (r) rumble(r);
}
