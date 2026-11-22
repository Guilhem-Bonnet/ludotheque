// Navigation à la manette et au clavier, avec retour sensoriel.
//
// - Croix / stick gauche / flèches : focus spatial vers l'élément `.focusable` le plus proche
//   (son « move » + vibration légère) ; au bord, petite poussée visuelle sans son.
// - A / Entrée : l'élément s'enfonce, dépasse légèrement puis se pose, son « ok » (sauf `data-sound="none"` ou
//   `data-sound="<nom>"`), puis clic natif.
// - B / Échap / Retour arrière : événement `ludo:back`.
// - X, Y : événement `ludo:action` (detail = 'x' | 'y').
// - L1 / R1 (Q / E au clavier) : événement `ludo:tab` (detail = -1 | 1).
//
// - Tab / Maj+Tab : élément suivant / précédent dans l'ordre de la page (dans le menu ouvert s'il y en a un).
//
// Répétition (flèche, croix ou stick maintenus) : un pas, puis après REPEAT_DELAY_MS un pas toutes les
// REPEAT_MS, quelle que soit la source (clavier, manette, Thor) ; la répétition du clavier du système est
// ignorée. Pendant la répétition, la racine porte `data-rush` (les écrans raccourcissent leurs animations).
// Souris : le survol donne le focus, mais seulement sur un vrai déplacement du pointeur — un
// contenu qui défile sous un pointeur immobile ne vole jamais le focus.
//
// Sources de la manette (aucune lecture par image hors du navigateur de développement) :
// - PC (Tauri) : lecture native par gilrs (src-tauri/src/pad.rs), événement `ludo://manette`, émis
//   seulement quand la fenêtre a le focus. Raison : WebKitGTK ne voit pas la croix « chapeau » des
//   manettes absentes de la base de libmanette (mesuré le 21/11, manette GameSir du PC).
// - Thor (Android) : MainActivity traduit boutons, croix (chapeau ou touches) et stick en touches.
// - Navigateur (développement) : API Gamepad, lue à chaque image UNIQUEMENT fenêtre au premier plan et
//   manette branchée (l'API n'a pas d'événement de bouton) ; arrêtée sur `blur`/onglet caché.
//
// Mise en page de la navigation (attributs, valables sur l'élément ou un ancêtre) :
// - `data-nav-group="<nom>"` : gauche / droite restent dans le groupe (ex. en-tête profil + synchro) ;
// - `data-nav-up|down|left|right="<sélecteur>"` : cible explicite (premier élément visible qui
//   correspond), prioritaire sur le calcul spatial.
//
// Hors focus (`blur`, onglet caché) : la racine porte `data-idle` (app.css met les animations en pause),
// toute direction tenue est relâchée, la manette n'est plus lue.

import { isAndroid } from './api';
import { feedback } from './feedback';
import { glideIntoView } from './glide';
import { reducedMotion } from './motion';
import { unlock, type SoundName } from './sound';

type Dir = 'up' | 'down' | 'left' | 'right';

/** Cadence de la répétition (ms) : lisible (≈ 9 pas par seconde) sans paraître lent. */
export const REPEAT_MS = 110;
/** Délai avant la première répétition de la croix et du stick (le clavier garde celui du système). */
const REPEAT_DELAY_MS = 380;

const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Dernière source d'entrée : directionnelle (`key`, `pad`) ou de pointage (`mouse`, `touch`). */
export const input: { kind: 'key' | 'pad' | 'mouse' | 'touch' } = { kind: 'key' };

/** Change la source d'entrée ; la racine porte `data-input` (styles propres au pointeur, au toucher…). */
function setInput(kind: (typeof input)['kind']) {
  input.kind = kind;
  if (document.documentElement.dataset.input !== kind) document.documentElement.dataset.input = kind;
}

function setRush(on: boolean) {
  const root = document.documentElement;
  if (on) root.dataset.rush = '';
  else delete root.dataset.rush;
}

function navScope(): ParentNode {
  // Un menu ouvert (`[data-modal]`) garde le focus : seuls ses éléments sont navigables.
  const modals = document.querySelectorAll<HTMLElement>('[data-modal]');
  return modals.length ? modals[modals.length - 1] : document;
}

const visible = (el: HTMLElement) => {
  const r = el.getBoundingClientRect();
  return r.width > 0 && r.height > 0 && !el.hasAttribute('disabled');
};

function focusables(): HTMLElement[] {
  return [...navScope().querySelectorAll<HTMLElement>('.focusable')].filter(visible);
}

/** Cible explicite `data-nav-<dir>` portée par l'élément ou un ancêtre (dans le menu ouvert, s'il y en a un). */
function explicitTarget(active: HTMLElement, dir: Dir): HTMLElement | null {
  const attr = `data-nav-${dir}`;
  const holder = active.closest<HTMLElement>(`[${attr}]`);
  const sel = holder?.getAttribute(attr);
  if (!sel) return null;
  try {
    return [...navScope().querySelectorAll<HTMLElement>(sel)].find((el) => el !== active && el.classList.contains('focusable') && visible(el)) ?? null;
  } catch {
    return null; // sélecteur invalide : calcul spatial
  }
}

/** Choisit le voisin dans une direction : distance principale + 2 × décalage orthogonal. */
export function neighbour(from: DOMRect, candidates: { el: HTMLElement; r: DOMRect }[], dir: Dir): HTMLElement | null {
  const cx = from.left + from.width / 2;
  const cy = from.top + from.height / 2;
  let best: HTMLElement | null = null;
  let bestScore = Infinity;
  for (const { el, r } of candidates) {
    const dx = r.left + r.width / 2 - cx;
    const dy = r.top + r.height / 2 - cy;
    const ok =
      (dir === 'right' && r.left >= from.right - 4) ||
      (dir === 'left' && r.right <= from.left + 4) ||
      (dir === 'down' && r.top >= from.bottom - 4) ||
      (dir === 'up' && r.bottom <= from.top + 4);
    if (!ok) continue;
    const main = dir === 'left' || dir === 'right' ? Math.abs(dx) : Math.abs(dy);
    const ortho = dir === 'left' || dir === 'right' ? Math.abs(dy) : Math.abs(dx);
    const score = main + 2 * ortho;
    if (score < bestScore) {
      bestScore = score;
      best = el;
    }
  }
  return best;
}

export function move(dir: Dir, repeat = false) {
  const all = focusables();
  const active = document.activeElement as HTMLElement | null;
  if (!active || !all.includes(active)) {
    all[0]?.focus();
    return;
  }
  // Réglage horizontal (volume…) : gauche / droite ajustent la valeur au lieu de déplacer le focus.
  if ((dir === 'left' || dir === 'right') && active.hasAttribute('data-adjust')) {
    active.dispatchEvent(new CustomEvent('adjust', { detail: dir === 'right' ? 1 : -1 }));
    return;
  }
  // Gauche / droite dans un groupe (`data-nav-group`) : on reste dans le groupe.
  const group = dir === 'left' || dir === 'right' ? active.closest<HTMLElement>('[data-nav-group]') : null;
  const pool = all.filter((el) => el !== active && (!group || group.contains(el)));
  const from = active.getBoundingClientRect();
  const next = explicitTarget(active, dir) ?? neighbour(from, pool.map((el) => ({ el, r: el.getBoundingClientRect() })), dir);
  if (next) {
    // Sans `preventScroll`, le navigateur défile d'un coup avant le défilement doux (saccade).
    // Un conteneur `data-own-scroll` gère lui-même son défilement (rangée de l'accueil).
    // Défilement par ressort (voir glide.ts) : une répétition infléchit le mouvement sans le relancer.
    next.focus({ preventScroll: true });
    if (!next.closest('[data-own-scroll]')) glideIntoView(next);
    feedback('move');
  } else if (!repeat) {
    // Bord atteint : une seule oscillation au premier appui, pas une vibration continue en maintenant.
    wobble(active, dir);
  }
}

/** Bord atteint : petite oscillation amortie vers la direction demandée (pas de son). */
function wobble(el: HTMLElement, dir: Dir) {
  if (reducedMotion()) return;
  const axis = dir === 'left' || dir === 'right' ? 'X' : 'Y';
  const sign = dir === 'right' || dir === 'down' ? 1 : -1;
  const t = (v: number) => ({ transform: `translate${axis}(${v * sign}px)` });
  el.animate([t(0), t(6), t(-3), t(1.5), t(0)], { duration: 300, easing: 'ease-out', composite: 'add' });
}

/** Appui : enfoncement (anticipation), léger dépassement au relâchement, retour au repos. */
function squash(el: HTMLElement) {
  if (reducedMotion()) return;
  el.animate(
    [
      { transform: 'scale(1)' },
      { transform: 'scale(0.95)', offset: 0.25, easing: 'ease-out' },
      { transform: 'scale(1.035)', offset: 0.6, easing: 'ease-in-out' },
      { transform: 'scale(1)' },
    ],
    { duration: 280, composite: 'add' },
  );
}

/** Appui sur A : enfoncement, son, puis action. */
export function press(el: HTMLElement | null) {
  if (!el || el.hasAttribute('disabled')) {
    if (el) feedback('err');
    return;
  }
  squash(el);
  const s = el.dataset.sound;
  if (s !== 'none') feedback((s as SoundName) || 'ok');
  setTimeout(() => el.click(), 60);
}

/** Tab / Maj+Tab : élément suivant / précédent dans l'ordre de la page, en boucle (menu ouvert compris). */
function tab(back: boolean) {
  const all = focusables();
  if (!all.length) return;
  const i = all.indexOf(document.activeElement as HTMLElement);
  const next = all[i < 0 ? 0 : (i + (back ? -1 : 1) + all.length) % all.length];
  next.focus({ preventScroll: true });
  if (!next.closest('[data-own-scroll]')) glideIntoView(next);
  feedback('move');
}

/** Met le focus sur `[data-autofocus]`, sinon sur le premier élément navigable. */
export function focusFirst() {
  requestAnimationFrame(() => {
    const modals = document.querySelectorAll<HTMLElement>('[data-modal]');
    const scope: ParentNode = modals.length ? modals[modals.length - 1] : document;
    const el = scope.querySelector<HTMLElement>('[data-autofocus]') ?? focusables()[0];
    el?.focus({ preventScroll: true });
  });
}

function emit(name: string, detail?: unknown) {
  window.dispatchEvent(new CustomEvent(name, { detail }));
}

/** Signale la touche appuyée à la barre d'aide (la touche dessinée s'enfonce aussi). */
function key(k: 'A' | 'B' | 'X' | 'Y' | 'L1' | 'R1') {
  emit('ludo:key', k);
}

const ARROWS: Record<string, Dir> = { ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right' };

const KEYS: Record<string, () => void> = {
  Enter: () => (key('A'), press(document.activeElement as HTMLElement | null)),
  ' ': () => (key('A'), press(document.activeElement as HTMLElement | null)),
  Escape: () => (key('B'), emit('ludo:back')),
  Backspace: () => (key('B'), emit('ludo:back')),
  x: () => (key('X'), emit('ludo:action', 'x')),
  y: () => (key('Y'), emit('ludo:action', 'y')),
  q: () => (key('L1'), emit('ludo:tab', -1)),
  e: () => (key('R1'), emit('ludo:tab', 1)),
};

/**
 * Touche « virtuelle » (clic ou toucher sur la barre d'aide, sur L1 / R1) : même effet que la manette.
 * A agit sur l'élément qui a le focus ; la barre d'aide ne prend jamais le focus (voir ButtonHints).
 */
export function virtualKey(k: 'A' | 'B' | 'X' | 'Y' | 'L1' | 'R1') {
  ({ A: PAD[0], B: PAD[1], X: PAD[2], Y: PAD[3], L1: PAD[4], R1: PAD[5] })[k]();
}

// Boutons de la manette standard (Gamepad API, et touches natives de pad.rs) : 0 A, 1 B, 2 X, 3 Y, 4 L1, 5 R1.
const PAD: Record<number, () => void> = {
  0: () => (key('A'), press(document.activeElement as HTMLElement | null)),
  1: () => (key('B'), emit('ludo:back')),
  2: () => (key('X'), emit('ludo:action', 'x')),
  3: () => (key('Y'), emit('ludo:action', 'y')),
  4: () => (key('L1'), emit('ludo:tab', -1)),
  5: () => (key('R1'), emit('ludo:tab', 1)),
};

/* ——— Directions tenues ———
   Chaque source (clavier, manette native, manette du navigateur) déclare l'appui et le relâchement d'une
   direction ; la répétition est cadencée ici. Une même direction tenue par deux sources (croix envoyée à la
   fois en touches et en chapeau, par exemple) ne fait qu'un pas. Le délai de répétition est un rythme
   d'interaction tant qu'une direction est tenue, pas une scrutation : il s'arrête au relâchement. */
const holds = new Map<string, Dir>();
let repeatDir: Dir | null = null;
let repeatTimer: ReturnType<typeof setTimeout> | undefined;

function stopRepeat() {
  clearTimeout(repeatTimer);
  repeatTimer = undefined;
  repeatDir = null;
  setRush(false);
}

/** Répète `dir` tant qu'elle est tenue : premier pas après `delay`, puis toutes les REPEAT_MS. */
function startRepeat(dir: Dir, delay: number) {
  clearTimeout(repeatTimer);
  repeatDir = dir;
  const step = () => {
    if (repeatDir !== dir) return;
    setRush(true);
    move(dir, true);
    repeatTimer = setTimeout(step, REPEAT_MS);
  };
  repeatTimer = setTimeout(step, delay);
}

function hold(source: string, dir: Dir) {
  // Même appui reçu deux fois (même source) : rien de plus.
  if (holds.get(source) === dir) return;
  const already = [...holds.values()].includes(dir);
  holds.set(source, dir);
  if (already) return;
  stopRepeat();
  move(dir);
  startRepeat(dir, REPEAT_DELAY_MS);
}

function release(source: string) {
  const dir = holds.get(source);
  holds.delete(source);
  if (!dir || dir !== repeatDir || [...holds.values()].includes(dir)) return;
  // La direction répétée est relâchée ; une autre encore tenue (diagonale, deux touches) reprend la main.
  const other = [...holds.values()].pop();
  if (other) startRepeat(other, REPEAT_DELAY_MS);
  else stopRepeat();
}

function releaseAll() {
  holds.clear();
  stopRepeat();
}

/** Appui sur un bouton de la manette (hors croix), quelle que soit la source. */
function padButton(i: number) {
  unlock();
  setInput('pad');
  PAD[i]?.();
}

const PAD_KEY: Record<string, number> = { A: 0, B: 1, X: 2, Y: 3, L1: 4, R1: 5 };
const DIRS: readonly Dir[] = ['up', 'down', 'left', 'right'];

/** PC (Tauri) : focus de la fenêtre annoncé par la coque (`ludo://focus`), plus fiable que `blur` sous WebKitGTK. */
let nativeFocus = true;
const nativeShell = inTauri && !isAndroid;

/** Fenêtre au premier plan et page visible : seul état où la navigation agit. */
const active = () => document.visibilityState === 'visible' && (nativeShell ? nativeFocus : document.hasFocus());

export function installNav() {
  window.addEventListener('keydown', (ev) => {
    unlock();
    // Thor : les boutons de la manette arrivent en touches (MainActivity) ; la barre d'aide garde A/B.
    setInput(isAndroid ? 'pad' : 'key');
    // Saisie de texte (nom du profil) : les touches tapent ; Échap quitte le champ sans revenir en arrière.
    const t = ev.target as HTMLElement | null;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA')) {
      if (ev.key === 'Escape') { ev.preventDefault(); t.blur(); t.closest<HTMLElement>('.field')?.focus(); }
      return;
    }
    const dir = ARROWS[ev.key];
    if (dir) {
      ev.preventDefault();
      // Répétition du système ignorée : la cadence est celle de `hold` (identique à la manette).
      if (!ev.repeat) hold(`key:${ev.key}`, dir);
      return;
    }
    if (ev.key === 'Tab' && !ev.altKey && !ev.ctrlKey && !ev.metaKey) {
      ev.preventDefault();
      tab(ev.shiftKey);
      return;
    }
    const handler = KEYS[ev.key];
    if (handler && !ev.ctrlKey && !ev.altKey && !ev.metaKey) {
      ev.preventDefault();
      if (!ev.repeat) handler();
    }
  });
  window.addEventListener('keyup', (ev) => { if (ARROWS[ev.key]) release(`key:${ev.key}`); });
  window.addEventListener('pointerdown', unlock, { once: true });
  window.addEventListener('pointerdown', (e) => setInput(e.pointerType === 'mouse' ? 'mouse' : 'touch'), { capture: true, passive: true });

  // Survol = focus, sur un VRAI déplacement de la souris (coordonnées changées). Les moteurs
  // envoient aussi des mouvements simulés quand le contenu défile sous un pointeur immobile : sans
  // ce garde, défiler la rangée ferait sauter le focus de tuile en tuile (et relancerait le défilement).
  let px = NaN;
  let py = NaN;
  window.addEventListener('pointermove', (e) => {
    if (e.pointerType !== 'mouse') return;
    if (e.clientX === px && e.clientY === py) return;
    px = e.clientX;
    py = e.clientY;
    // Mode souris dès le premier vrai mouvement : les écrans stabilisent leur mise en page (pas de
    // décalage des voisines) avant que le pointeur atteigne une cible.
    setInput('mouse');
    const el = (e.target as Element | null)?.closest?.<HTMLElement>('.tile.focusable, [data-hover-focus]');
    if (!el || el === document.activeElement || el.hasAttribute('disabled')) return;
    const modals = document.querySelectorAll<HTMLElement>('[data-modal]');
    if (modals.length && !modals[modals.length - 1].contains(el)) return;
    el.focus({ preventScroll: true });
  }, { passive: true });

  // ——— Manette ———
  if (nativeShell) {
    // PC : appuis natifs (pad.rs), déjà filtrés par le focus de la fenêtre côté Rust ; filtrés ici aussi.
    void import('@tauri-apps/api/event')
      .then(({ listen }) =>
        listen<{ key: string; pressed: boolean }>('ludo://manette', ({ payload: { key: k, pressed } }) => {
          if (!active()) return;
          if ((DIRS as readonly string[]).includes(k)) {
            if (pressed) { unlock(); setInput('pad'); hold(`pad:${k}`, k as Dir); } else release(`pad:${k}`);
          } else if (pressed && k in PAD_KEY) padButton(PAD_KEY[k]);
        }),
      )
      .catch(() => {});
  } else if (!isAndroid) {
    installBrowserPad();
  }

  // ——— Hors focus : animations en pause, directions relâchées ———
  const root = document.documentElement;
  const sleep = () => { root.dataset.idle = ''; releaseAll(); stopBrowserPad(); };
  const wake = () => { if (!active()) return; delete root.dataset.idle; startBrowserPad(); };
  window.addEventListener('blur', sleep);
  // PC : la vérité du focus vient de la coque (`ludo://focus`) ; ailleurs, de la page.
  window.addEventListener('focus', () => { if (!nativeShell) nativeFocus = true; wake(); });
  if (nativeShell) {
    void import('@tauri-apps/api/event')
      .then(({ listen }) => listen<boolean>('ludo://focus', ({ payload }) => { nativeFocus = payload; if (payload) wake(); else sleep(); }))
      .catch(() => {});
  }
  document.addEventListener('visibilitychange', () => (document.visibilityState === 'hidden' ? sleep() : wake()));
}

/* ——— Manette dans un navigateur (développement, démonstration) ———
   L'API Gamepad n'émet aucun événement de bouton : lecture à chaque image, admise UNIQUEMENT fenêtre au
   premier plan et manette branchée (`gamepadconnected`) ; arrêt sur `blur`, onglet caché, débranchement. */
let padFrame = 0;
let startBrowserPad = () => {};
let stopBrowserPad = () => {};

/** Croix d'une manette sans correspondance standard (navigateur) : chapeau exposé en axes 6 / 7
 *  (Firefox sous Linux). L'axe « POV » unique n'est pas lu : sur une autre manette, l'axe 9 au repos (0)
 *  serait pris pour « bas » en permanence. */
export function hatDir(pad: { mapping: string; axes: readonly number[] }): Dir | null {
  if (pad.mapping === 'standard' || pad.axes.length < 8) return null;
  const [x, y] = [pad.axes[6], pad.axes[7]];
  return x > 0.5 ? 'right' : x < -0.5 ? 'left' : y > 0.5 ? 'down' : y < -0.5 ? 'up' : null;
}

function installBrowserPad() {
  const DPAD: Record<number, Dir> = { 12: 'up', 13: 'down', 14: 'left', 15: 'right' };
  const pressed = new Set<string>();
  let held: Dir | null = null;
  const poll = () => {
    padFrame = 0;
    let dir: Dir | null = null;
    let any = false;
    for (const pad of navigator.getGamepads?.() ?? []) {
      if (!pad) continue;
      any = true;
      pad.buttons.forEach((b, i) => {
        const id = `${pad.index}:${i}`;
        if (b.pressed && DPAD[i]) dir ??= DPAD[i];
        if (b.pressed && !pressed.has(id)) {
          pressed.add(id);
          if (!DPAD[i] && i in PAD) padButton(i);
        } else if (!b.pressed) pressed.delete(id);
      });
      dir ??= hatDir(pad);
      const [ax, ay] = pad.axes;
      dir ??= ax > 0.6 ? 'right' : ax < -0.6 ? 'left' : ay > 0.6 ? 'down' : ay < -0.6 ? 'up' : null;
    }
    if (dir !== held) {
      if (held) release('gamepad');
      if (dir) { unlock(); setInput('pad'); hold('gamepad', dir); }
      held = dir;
    }
    if (any) padFrame = requestAnimationFrame(poll);
  };
  startBrowserPad = () => {
    if (padFrame || !active() || !(navigator.getGamepads?.() ?? []).some(Boolean)) return;
    padFrame = requestAnimationFrame(poll);
  };
  stopBrowserPad = () => {
    if (padFrame) cancelAnimationFrame(padFrame);
    padFrame = 0;
    if (held) release('gamepad');
    held = null;
    pressed.clear();
  };
  window.addEventListener('gamepadconnected', () => startBrowserPad());
  window.addEventListener('gamepaddisconnected', () => {
    if (!(navigator.getGamepads?.() ?? []).some(Boolean)) stopBrowserPad();
  });
}
