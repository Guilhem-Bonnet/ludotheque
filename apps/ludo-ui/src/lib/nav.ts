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
// Répétition (flèche, croix ou stick maintenus) : un pas toutes les REPEAT_MS au plus, quelle que
// soit la cadence du clavier du système (25 à 33 par seconde) ; pendant la répétition, la racine
// porte `data-rush` (les écrans raccourcissent leurs animations), retiré au relâchement.
// Souris : le survol donne le focus, mais seulement sur un vrai déplacement du pointeur — un
// contenu qui défile sous un pointeur immobile ne vole jamais le focus.

import { feedback } from './feedback';
import { glideIntoView } from './glide';
import { reducedMotion } from './motion';
import { unlock, type SoundName } from './sound';

type Dir = 'up' | 'down' | 'left' | 'right';

/** Cadence de la répétition (ms) : lisible (≈ 9 pas par seconde) sans paraître lent. */
export const REPEAT_MS = 110;
/** Délai avant la première répétition de la croix et du stick (le clavier garde celui du système). */
const REPEAT_DELAY_MS = 380;

/** Dernière source d'entrée : directionnelle (`key`, `pad`) ou de pointage (`mouse`, `touch`). */
export const input: { kind: 'key' | 'pad' | 'mouse' | 'touch' } = { kind: 'key' };

function setRush(on: boolean) {
  const root = document.documentElement;
  if (on) root.dataset.rush = '';
  else delete root.dataset.rush;
}

function focusables(): HTMLElement[] {
  // Un menu ouvert (`[data-modal]`) garde le focus : seuls ses éléments sont navigables.
  const modals = document.querySelectorAll<HTMLElement>('[data-modal]');
  const scope: ParentNode = modals.length ? modals[modals.length - 1] : document;
  return [...scope.querySelectorAll<HTMLElement>('.focusable')].filter((el) => {
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0 && !el.hasAttribute('disabled');
  });
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
  const from = active.getBoundingClientRect();
  const next = neighbour(from, all.filter((el) => el !== active).map((el) => ({ el, r: el.getBoundingClientRect() })), dir);
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

// Boutons de la manette standard (Gamepad API) : 0 A, 1 B, 2 X, 3 Y, 4 L1, 5 R1, 12-15 croix.
const PAD: Record<number, () => void> = {
  0: () => (key('A'), press(document.activeElement as HTMLElement | null)),
  1: () => (key('B'), emit('ludo:back')),
  2: () => (key('X'), emit('ludo:action', 'x')),
  3: () => (key('Y'), emit('ludo:action', 'y')),
  4: () => (key('L1'), emit('ludo:tab', -1)),
  5: () => (key('R1'), emit('ludo:tab', 1)),
  12: () => move('up'),
  13: () => move('down'),
  14: () => move('left'),
  15: () => move('right'),
};

export function installNav() {
  let lastStep = 0;
  window.addEventListener('keydown', (ev) => {
    unlock();
    input.kind = 'key';
    // Saisie de texte (nom du profil) : les touches tapent ; Échap quitte le champ sans revenir en arrière.
    const t = ev.target as HTMLElement | null;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA')) {
      if (ev.key === 'Escape') { ev.preventDefault(); t.blur(); t.closest<HTMLElement>('.field')?.focus(); }
      return;
    }
    const dir = ARROWS[ev.key];
    if (dir) {
      ev.preventDefault();
      if (ev.repeat) {
        // Répétition du système : cadencée ici, les événements en trop sont ignorés.
        if (ev.timeStamp - lastStep < REPEAT_MS) return;
        setRush(true);
      }
      lastStep = ev.timeStamp;
      move(dir, ev.repeat);
      return;
    }
    const handler = KEYS[ev.key];
    if (handler) {
      ev.preventDefault();
      handler();
    }
  });
  window.addEventListener('keyup', (ev) => { if (ARROWS[ev.key]) setRush(false); });
  window.addEventListener('blur', () => setRush(false));
  window.addEventListener('pointerdown', unlock, { once: true });
  window.addEventListener('pointerdown', (e) => { input.kind = e.pointerType === 'mouse' ? 'mouse' : 'touch'; }, { capture: true, passive: true });

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
    const el = (e.target as Element | null)?.closest?.<HTMLElement>('.tile.focusable, [data-hover-focus]');
    if (!el || el === document.activeElement || el.hasAttribute('disabled')) return;
    const modals = document.querySelectorAll<HTMLElement>('[data-modal]');
    if (modals.length && !modals[modals.length - 1].contains(el)) return;
    input.kind = 'mouse';
    el.focus({ preventScroll: true });
  }, { passive: true });

  // Manette : l'API Gamepad n'émet aucun événement pour les boutons, on lit son état à chaque image
  // (rythme d'affichage, comme une boucle de jeu). Croix et stick : un pas, puis répétition après
  // REPEAT_DELAY_MS, toutes les REPEAT_MS.
  const pressed = new Set<string>();
  const DPAD: Record<number, Dir> = { 12: 'up', 13: 'down', 14: 'left', 15: 'right' };
  let held: Dir | null = null;
  let repeatAt = 0;
  let repeating = false;
  const poll = (t: number) => {
    let dir: Dir | null = null;
    for (const pad of navigator.getGamepads?.() ?? []) {
      if (!pad) continue;
      pad.buttons.forEach((b, i) => {
        const id = `${pad.index}:${i}`;
        if (b.pressed && DPAD[i]) dir ??= DPAD[i];
        if (b.pressed && !pressed.has(id)) {
          pressed.add(id);
          unlock();
          input.kind = 'pad';
          if (!DPAD[i]) PAD[i]?.();
        } else if (!b.pressed) pressed.delete(id);
      });
      const [ax, ay] = pad.axes;
      dir ??= ax > 0.6 ? 'right' : ax < -0.6 ? 'left' : ay > 0.6 ? 'down' : ay < -0.6 ? 'up' : null;
    }
    if (dir && dir !== held) {
      input.kind = 'pad';
      move(dir);
      repeatAt = t + REPEAT_DELAY_MS;
    } else if (dir && t >= repeatAt) {
      if (!repeating) setRush((repeating = true));
      move(dir, true);
      repeatAt = t + REPEAT_MS;
    }
    if (!dir && repeating) setRush((repeating = false));
    held = dir;
    requestAnimationFrame(poll);
  };
  requestAnimationFrame(poll);
}
