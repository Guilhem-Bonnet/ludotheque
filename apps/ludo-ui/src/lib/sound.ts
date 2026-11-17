// Palette sonore « Feutré doux v6 », validée par Guilhem le 15/11/2026.
// Tout est synthétisé en direct (Web Audio) : aucun fichier, aucun son copié.
//
// Règles de la palette (à respecter pour tout nouveau son) :
// - sinus uniquement, attaque ≥ 12 ms (aucun clic), passe-bas autour de 1,1 à 1,8 kHz ;
// - même son sur toutes les tuiles (pas de note qui suit la position) ;
// - jamais de son qui descend pour un moment positif : les réussites montent doucement ;
// - légère variation aléatoire (±6 cents, ±8 % de niveau) pour ne pas lasser.

export type SoundName =
  | 'move' | 'ok' | 'back' | 'tab' | 'err'
  | 'saved' | 'conflict' | 'arrive' | 'launch'
  | 'loading' | 'downloaded' | 'updated'
  | 'syncStart' | 'syncTick';

let ctx: AudioContext | null = null;
let master: GainNode | null = null;
let verb: GainNode | null = null;
let volume = 0.7;
let muted = false;

function setup(): boolean {
  if (ctx) {
    if (ctx.state === 'suspended') void ctx.resume();
    return true;
  }
  const AC = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!AC) return false;
  ctx = new AC();
  const comp = ctx.createDynamicsCompressor();
  comp.threshold.value = -18;
  comp.ratio.value = 3;
  master = ctx.createGain();
  master.gain.value = muted ? 0 : volume * 0.8;
  master.connect(comp).connect(ctx.destination);
  // Réverbération courte générée (bruit à décroissance rapide).
  const conv = ctx.createConvolver();
  const len = Math.floor(ctx.sampleRate * 1.6);
  const ir = ctx.createBuffer(2, len, ctx.sampleRate);
  for (let ch = 0; ch < 2; ch++) {
    const d = ir.getChannelData(ch);
    for (let i = 0; i < len; i++) d[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / len, 3.2);
  }
  conv.buffer = ir;
  verb = ctx.createGain();
  verb.connect(conv).connect(master);
  return true;
}

const midi = (n: number) => 440 * Math.pow(2, (n - 69) / 12);
const vary = (a: number) => 1 + (Math.random() * 2 - 1) * a;

type Tone = { n: number; t?: number; a?: number; d?: number; g?: number; lp?: number; wet?: number; glide?: number };

function tone({ n, t = 0, a = 0.012, d = 0.2, g = 0.15, lp = 1300, wet = 0.15, glide = 0 }: Tone) {
  if (!ctx || !master || !verb) return;
  const now = ctx.currentTime + t;
  const f = midi(n) * Math.pow(2, ((Math.random() * 2 - 1) * 6) / 1200);
  const o = ctx.createOscillator();
  o.type = 'sine';
  o.frequency.setValueAtTime(f, now);
  if (glide) o.frequency.exponentialRampToValueAtTime(f * glide, now + d * 0.8);
  const v = ctx.createGain();
  v.gain.setValueAtTime(0.0001, now);
  v.gain.exponentialRampToValueAtTime(g * vary(0.08), now + a);
  v.gain.exponentialRampToValueAtTime(0.0001, now + a + d);
  const fl = ctx.createBiquadFilter();
  fl.type = 'lowpass';
  fl.frequency.value = lp;
  fl.Q.value = 0.4;
  o.connect(v).connect(fl);
  fl.connect(master);
  if (wet > 0) {
    const s = ctx.createGain();
    s.gain.value = wet;
    fl.connect(s).connect(verb);
  }
  o.start(now);
  o.stop(now + a + d + 0.05);
}

function pad(notes: number[], t = 0, d = 1.5, g = 0.035, wet = 0.45) {
  notes.forEach((n, i) => tone({ n, t: t + i * 0.012, a: 0.06, d, g, lp: 2600, wet }));
}

const SOUNDS: Record<SoundName, () => void> = {
  // Validés (v6)
  move: () => tone({ n: 69, d: 0.16, g: 0.16, wet: 0.12, glide: 0.97 }),
  ok: () => { tone({ n: 76, d: 0.2, g: 0.17, lp: 1600, glide: 1.06 }); tone({ n: 81, t: 0.07, a: 0.015, d: 0.3, g: 0.11, lp: 1800, wet: 0.2 }); },
  back: () => tone({ n: 69, d: 0.2, g: 0.14, lp: 1100, glide: 0.88 }),
  tab: () => tone({ n: 84, a: 0.008, d: 0.06, g: 0.07, lp: 1800, wet: 0.05 }),
  err: () => tone({ n: 52, a: 0.02, d: 0.3, g: 0.16, lp: 600, glide: 0.95 }),
  saved: () => pad([57, 64, 69]),
  conflict: () => tone({ n: 72, a: 0.03, d: 0.6, g: 0.11, lp: 1200, wet: 0.3 }),
  arrive: () => { pad([60, 67, 72], 0, 1.0, 0.045); [72, 79].forEach((n, k) => tone({ n, t: 0.14 + k * 0.09, a: 0.015, d: 0.3, g: 0.1, lp: 1800, wet: 0.2 })); },
  launch: () => { [60, 64, 69, 72].forEach((n, k) => tone({ n, t: k * 0.095, a: 0.015, d: k === 3 ? 0.9 : 0.55, g: 0.14, lp: 1800, wet: 0.22 })); },
  // Nouveaux (à affiner avec Guilhem) : même famille, toujours en montée douce.
  loading: () => { tone({ n: 64, a: 0.03, d: 0.35, g: 0.08, lp: 1100, wet: 0.25 }); tone({ n: 69, t: 0.16, a: 0.03, d: 0.4, g: 0.07, lp: 1200, wet: 0.3 }); },
  downloaded: () => { [64, 69, 76].forEach((n, k) => tone({ n, t: k * 0.085, a: 0.015, d: k === 2 ? 0.8 : 0.4, g: 0.12, lp: 1700, wet: 0.25 })); },
  // Synchronisation manuelle : départ en deux notes qui montent, puis une goutte très douce
  // toutes les 0,9 s tant que ça charge (toujours la même note : ni montée ni descente, juste « ça vit »).
  syncStart: () => { tone({ n: 67, a: 0.02, d: 0.22, g: 0.09, lp: 1400, wet: 0.2 }); tone({ n: 74, t: 0.09, a: 0.02, d: 0.32, g: 0.075, lp: 1500, wet: 0.28 }); },
  syncTick: () => tone({ n: 79, a: 0.02, d: 0.16, g: 0.032, lp: 1500, wet: 0.35 }),
  updated: () => { [69, 72, 76].forEach((n, k) => tone({ n, t: k * 0.085, a: 0.015, d: k === 2 ? 0.8 : 0.4, g: 0.11, lp: 1700, wet: 0.25 })); },
};

/** Joue un son de la palette (sans effet tant que l'utilisateur n'a pas interagi : règle des navigateurs). */
export function play(name: SoundName) {
  if (muted || !setup()) return;
  SOUNDS[name]();
}

/** À appeler sur la première interaction (touche, bouton manette) pour débloquer l'audio. */
export function unlock() {
  setup();
}

export function setVolume(v: number) {
  volume = Math.max(0, Math.min(1, v));
  if (master && ctx && !muted) master.gain.setTargetAtTime(volume * 0.8, ctx.currentTime, 0.02);
}

export function setMuted(m: boolean) {
  muted = m;
  if (master && ctx) master.gain.setTargetAtTime(m ? 0 : volume * 0.8, ctx.currentTime, 0.02);
}

/**
 * Cliquetis régulier pendant une synchronisation : un RYTHME sonore (comme une animation), pas une
 * scrutation — il ne lit aucun état, la fin de l'opération l'arrête (fonction renvoyée).
 */
export function syncRhythm(): () => void {
  const t = setInterval(() => play('syncTick'), 900);
  return () => clearInterval(t);
}
