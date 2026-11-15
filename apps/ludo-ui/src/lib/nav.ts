// Navigation à la manette et au clavier.
//
// - Croix / stick gauche / flèches : focus spatial vers l'élément `.focusable` le plus proche.
// - A / Entrée : clic natif sur l'élément en focus.
// - B / Échap / Retour arrière : événement `ludo:back`.
// - X, Y : événement `ludo:action` (detail = 'x' | 'y').
// - L1 / R1 (Q / E au clavier) : événement `ludo:tab` (detail = -1 | 1).

type Dir = 'up' | 'down' | 'left' | 'right';

function focusables(): HTMLElement[] {
  return [...document.querySelectorAll<HTMLElement>('.focusable')].filter((el) => {
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
    const x = r.left + r.width / 2;
    const y = r.top + r.height / 2;
    const dx = x - cx;
    const dy = y - cy;
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

export function move(dir: Dir) {
  const all = focusables();
  const active = document.activeElement as HTMLElement | null;
  if (!active || !all.includes(active)) {
    all[0]?.focus();
    return;
  }
  const from = active.getBoundingClientRect();
  const next = neighbour(from, all.filter((el) => el !== active).map((el) => ({ el, r: el.getBoundingClientRect() })), dir);
  if (next) {
    next.focus();
    next.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'smooth' });
  }
}

/** Met le focus sur `[data-autofocus]`, sinon sur le premier élément navigable. */
export function focusFirst() {
  requestAnimationFrame(() => {
    const el = document.querySelector<HTMLElement>('[data-autofocus]') ?? focusables()[0];
    el?.focus();
  });
}

function emit(name: string, detail?: unknown) {
  window.dispatchEvent(new CustomEvent(name, { detail }));
}

const KEYS: Record<string, () => void> = {
  ArrowUp: () => move('up'),
  ArrowDown: () => move('down'),
  ArrowLeft: () => move('left'),
  ArrowRight: () => move('right'),
  Escape: () => emit('ludo:back'),
  Backspace: () => emit('ludo:back'),
  x: () => emit('ludo:action', 'x'),
  y: () => emit('ludo:action', 'y'),
  q: () => emit('ludo:tab', -1),
  e: () => emit('ludo:tab', 1),
};

// Boutons de la manette standard (Gamepad API) : 0 A, 1 B, 2 X, 3 Y, 4 L1, 5 R1, 12-15 croix.
const PAD: Record<number, () => void> = {
  0: () => (document.activeElement as HTMLElement | null)?.click(),
  1: () => emit('ludo:back'),
  2: () => emit('ludo:action', 'x'),
  3: () => emit('ludo:action', 'y'),
  4: () => emit('ludo:tab', -1),
  5: () => emit('ludo:tab', 1),
  12: () => move('up'),
  13: () => move('down'),
  14: () => move('left'),
  15: () => move('right'),
};

export function installNav() {
  window.addEventListener('keydown', (ev) => {
    const handler = KEYS[ev.key];
    if (handler) {
      ev.preventDefault();
      handler();
    }
  });

  const pressed = new Set<string>();
  let stickHeld: Dir | null = null;
  let stickRepeatAt = 0;
  const poll = (t: number) => {
    for (const pad of navigator.getGamepads?.() ?? []) {
      if (!pad) continue;
      pad.buttons.forEach((b, i) => {
        const id = `${pad.index}:${i}`;
        if (b.pressed && !pressed.has(id)) {
          pressed.add(id);
          PAD[i]?.();
        } else if (!b.pressed) pressed.delete(id);
      });
      const [ax, ay] = pad.axes;
      const dir: Dir | null = ax > 0.6 ? 'right' : ax < -0.6 ? 'left' : ay > 0.6 ? 'down' : ay < -0.6 ? 'up' : null;
      if (dir && (dir !== stickHeld || t > stickRepeatAt)) {
        move(dir);
        stickRepeatAt = t + (dir === stickHeld ? 120 : 380);
      }
      stickHeld = dir;
    }
    requestAnimationFrame(poll);
  };
  requestAnimationFrame(poll);
}
