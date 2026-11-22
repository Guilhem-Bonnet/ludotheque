// Outils d'animation partagés.
//
// Durées et courbes : une seule source, les variables de `app.css` (:root, « Charte d'animation »).
// Le code les lit ici au lieu de répéter des millisecondes en dur.

export const reducedMotion = () => window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;

/** Défilement rapide en cours (maintien d'une direction) : la racine porte `data-rush`. */
export const rushing = () => document.documentElement.hasAttribute('data-rush');

const cache = new Map<string, number>();
function cssMs(name: string, fallback: number) {
  let v = cache.get(name);
  if (v === undefined) {
    const raw = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
    const n = parseFloat(raw);
    v = Number.isFinite(n) ? (raw.endsWith('ms') || !raw.endsWith('s') ? n : n * 1000) : fallback;
    cache.set(name, v);
  }
  return v;
}

/** Durées de la charte (ms), lues dans app.css ; repli = mêmes valeurs. */
export const DUR = {
  get quick() { return cssMs('--t-quick', 120); },
  get std() { return cssMs('--t-std', 240); },
  get slow() { return cssMs('--t-slow', 420); },
  get move() { return cssMs('--t-move', 260); },
  get rush() { return cssMs('--t-rush', 150); },
  get world() { return cssMs('--t-world', 520); },
};

/** Courbes de la charte pour les transitions Svelte (mêmes points que --ease-land / --ease-exit). */
export const EASE = {
  land: (t: number) => bezier(0.2, 0.8, 0.2, 1, t),
  exit: (t: number) => bezier(0.4, 0, 1, 1, t),
};

/** Évalue une courbe de Bézier cubique CSS (x1, y1, x2, y2) en `t` (résolution de x par Newton). */
export function bezier(x1: number, y1: number, x2: number, y2: number, x: number) {
  if (x <= 0) return 0;
  if (x >= 1) return 1;
  const cx = 3 * x1, bx = 3 * (x2 - x1) - cx, ax = 1 - cx - bx;
  const cy = 3 * y1, by = 3 * (y2 - y1) - cy, ay = 1 - cy - by;
  let u = x;
  for (let i = 0; i < 8; i++) {
    const fx = ((ax * u + bx) * u + cx) * u - x;
    const d = (3 * ax * u + 2 * bx) * u + cx;
    if (Math.abs(fx) < 1e-5 || d === 0) break;
    u -= fx / d;
  }
  return ((ay * u + by) * u + cy) * u;
}

/** Rejoue une classe d'animation CSS sur un élément (retire, force un reflow, remet). */
export function replay(el: Element | null | undefined, cls: string, ms = 400) {
  if (!el) return;
  el.classList.remove(cls);
  void (el as HTMLElement).offsetWidth;
  el.classList.add(cls);
  setTimeout(() => el.classList.remove(cls), ms);
}

type VT = { startViewTransition?: (cb: () => Promise<void> | void) => { finished: Promise<void> } };

/** Change d'écran dans une View Transition si le moteur la connaît (WebKitGTK récent, Chromium). */
export function withTransition(dir: 'forward' | 'back', update: () => Promise<void> | void) {
  const doc = document as unknown as VT;
  document.documentElement.dataset.dir = dir;
  if (!doc.startViewTransition || reducedMotion()) {
    void update();
    return;
  }
  doc.startViewTransition(update);
}
