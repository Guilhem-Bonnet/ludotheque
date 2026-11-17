// Défilement doux piloté par un ressort amorti critique (sans rebond), à la place de
// `scrollTo({ behavior: 'smooth' })`.
//
// Pourquoi : le défilement doux natif repart de zéro à chaque nouvel appel. Flèche maintenue
// (répétition clavier ou manette) = une suite de démarrages arrêtés, donc une saccade et un
// retard qui s'accumule. Ici, changer de cible garde la vitesse acquise : le mouvement s'infléchit
// au lieu de redémarrer, et le retard sur une cible qui avance à vitesse constante reste borné
// (2·v/ω, soit environ une tuile à 9 pas par seconde).
//
// - Solution exacte du ressort à chaque image (pas d'intégration pas à pas) : même trajectoire à
//   60 Hz, 120 Hz ou avec des images perdues.
// - Animé par requestAnimationFrame (rythme d'animation, ne lit aucun état) ; s'arrête de lui-même
//   une fois posé (moins de 0,5 px et 5 px/s).
// - La main de l'utilisateur gagne toujours : molette, toucher ou défilement extérieur annulent le
//   ressort, qui repartira de la position réelle au prochain appel.

import { reducedMotion } from './motion';

/** Raideur : 90 % du trajet en ≈ 195 ms, posé (< 0,5 px sur une tuile) en ≈ 430 ms. */
export const OMEGA = 20;

/** Pas du ressort amorti critique, exact pour une durée `dt` (s). `e` = écart à la cible, `v` = vitesse. */
export function springStep(e: number, v: number, dt: number, omega = OMEGA): [number, number] {
  const k = Math.exp(-omega * dt);
  const c = v + omega * e;
  return [(e + c * dt) * k, (v - omega * c * dt) * k];
}

type Axis = 'x' | 'y';

class Glide {
  private pos = 0;
  private vel = 0;
  private target = 0;
  private raf = 0;
  private last = 0;
  /** Dernière valeur écrite : un défilement qui s'en écarte vient d'ailleurs (doigt, molette, barre). */
  private written = NaN;

  constructor(private el: HTMLElement, private axis: Axis) {
    const stop = () => this.cancel();
    // Le doigt ou la molette native prennent la main (passifs : on n'empêche jamais le geste).
    el.addEventListener('touchstart', stop, { passive: true });
    el.addEventListener('pointerdown', (e) => { if (e.pointerType !== 'mouse') stop(); }, { passive: true });
    el.addEventListener('scroll', () => {
      if (this.raf && Math.abs(this.read() - this.written) > 2) this.cancel();
    }, { passive: true });
  }

  private read() { return this.axis === 'x' ? this.el.scrollLeft : this.el.scrollTop; }
  private max() {
    return this.axis === 'x' ? this.el.scrollWidth - this.el.clientWidth : this.el.scrollHeight - this.el.clientHeight;
  }
  private write(v: number) {
    // Position entière : un décalage fractionnaire oblige le moteur à redessiner tout le contenu au
    // lieu de décaler l'image déjà dessinée (mesuré dans WebKitGTK sans composition).
    v = Math.round(v);
    this.written = v;
    if (this.axis === 'x') this.el.scrollLeft = v;
    else this.el.scrollTop = v;
  }
  private clamp(v: number) { return Math.max(0, Math.min(v, Math.max(0, this.max()))); }

  get moving() { return this.raf !== 0; }
  /** Cible courante (ou position réelle au repos) : base des déplacements relatifs (molette). */
  get goal() { return this.raf ? this.target : this.read(); }
  /** Le défilement actuel vient-il du ressort (et non d'un doigt, d'un pavé ou d'une barre) ? */
  get own() { return this.raf !== 0 || Math.abs(this.read() - this.written) <= 2; }
  /** Position réelle du contenu, à cet instant. */
  get now() { return this.read(); }

  /** Va vers `to` (bornée au contenu). `instant` : saut direct (retour d'écran, animations réduites). */
  to(to: number, instant = false) {
    this.target = this.clamp(to);
    if (instant || reducedMotion()) {
      this.cancel();
      this.write(this.target);
      return;
    }
    if (!this.raf) {
      this.pos = this.read();
      this.vel = 0;
      this.last = 0;
      if (Math.abs(this.target - this.pos) < 0.5) return;
      this.raf = requestAnimationFrame(this.frame);
    }
  }

  /** Déplacement relatif (molette à crans) : s'ajoute à la cible en cours. */
  by(delta: number) { this.to(this.goal + delta); }

  cancel() {
    if (this.raf) cancelAnimationFrame(this.raf);
    this.raf = 0;
    this.vel = 0;
  }

  private frame = (t: number) => {
    // Première image : pas de durée connue, on prend une image à 60 Hz. Au-delà de 64 ms
    // (onglet masqué, gel), on borne pour ne pas « sauter » d'un coup.
    const dt = this.last ? Math.min((t - this.last) / 1000, 0.064) : 1 / 60;
    this.last = t;
    const [e, v] = springStep(this.pos - this.target, this.vel, dt);
    this.pos = this.target + e;
    this.vel = v;
    if (Math.abs(e) < 0.5 && Math.abs(v) < 5) {
      this.write(this.target);
      this.raf = 0;
      this.vel = 0;
      return;
    }
    this.write(this.pos);
    this.raf = requestAnimationFrame(this.frame);
  };
}

const glides = new WeakMap<HTMLElement, Partial<Record<Axis, Glide>>>();

/** Ressort de défilement d'un conteneur sur un axe (un seul par conteneur et par axe). */
export function glide(el: HTMLElement, axis: Axis = 'x'): Glide {
  let g = glides.get(el);
  if (!g) glides.set(el, (g = {}));
  return (g[axis] ??= new Glide(el, axis));
}

function scrollParent(el: HTMLElement, axis: Axis): HTMLElement | null {
  for (let p = el.parentElement; p; p = p.parentElement) {
    const s = getComputedStyle(p);
    const ov = axis === 'x' ? s.overflowX : s.overflowY;
    const room = axis === 'x' ? p.scrollWidth > p.clientWidth : p.scrollHeight > p.clientHeight;
    if (room && (ov === 'auto' || ov === 'scroll')) return p;
  }
  return null;
}

/** Équivalent doux de `scrollIntoView({ block: 'nearest', inline: 'nearest' })`, avec une marge. */
export function glideIntoView(el: HTMLElement, margin = 24) {
  const r = el.getBoundingClientRect();
  for (const axis of ['x', 'y'] as const) {
    const box = scrollParent(el, axis);
    if (!box) continue;
    const b = box.getBoundingClientRect();
    const [lo, hi, blo, bhi] = axis === 'x' ? [r.left, r.right, b.left, b.right] : [r.top, r.bottom, b.top, b.bottom];
    const g = glide(box, axis);
    // Position de l'élément une fois le défilement en cours arrivé : on ne corrige que si besoin.
    const ahead = g.goal - g.now;
    if (lo - ahead < blo + margin) g.to(g.goal - (blo + margin - (lo - ahead)));
    else if (hi - ahead > bhi - margin) g.to(g.goal + (hi - ahead - (bhi - margin)));
  }
}
