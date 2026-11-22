// Formes de cartouche par console (tuile de jeu, cf. Tile.svelte).
//
// Chaque tuile est la face d'un support de SA console, dessinée par un contour SVG vectoriel (net à
// tous les zooms, aucun filtre) : un boîtier aux couleurs de la console, un bandeau en haut (nom de la
// console, état de synchro) et une fenêtre d'étiquette dessous (image du jeu ou jaquette illustrée).
// Les signes distinctifs reprennent les traits PHYSIQUES des supports (silhouette, encoche, stries,
// coin coupé, disque), jamais un logo, une police ou un dessin de Nintendo (grille des formes et
// sources : description de la PR « cartouches par console » du 21/11/2026).
//
//   Switch    carte arrondie, coins hauts serrés, lèvre de préhension sous le bandeau
//   Switch 2  même carte + encoche dans le bord bas, à droite (trait réel des cartes Switch 2)
//   3DS       carte DS + ergot sur le flanc droit (détrompeur réel des cartes 3DS) + stries
//   DS        carte à coin haut droit coupé + stries de préhension dans le bandeau
//   GameCube  boîtier de mini-disque : coins ronds + pastille de disque (moyeu) devant le nom
//   Wii U     boîtier très arrondi + tranche verticale à gauche (dos du boîtier)
//
// Toutes les cotes sont des fractions de la taille `s` de la tuile (carrée), arrondies au demi-pixel :
// la forme reste la même de 96 px (menu) à 340 px (écran de passage).

import type { Console } from '../types';

export interface CartShape {
  /** Contour du boîtier (repère 0..s ; l'ergot de la 3DS peut déborder de quelques pixels à droite). */
  outline: string;
  /** Traits gravés dans le bandeau (stries, lèvre), dessinés à l'encre du bandeau, très atténués. */
  grooves: string;
  /** Hauteur du bandeau (px). */
  band: number;
  /** Fenêtre d'étiquette (image du jeu) : position et rayons, en px. */
  window: { top: number; left: number; right: number; bottom: number; rTop: number; rBottom: number };
  /** Retrait du texte du bandeau à gauche et à droite (coin coupé de la DS, pastille de disque). */
  padLeft: number;
  padRight: number;
  /** Pastille de disque (GameCube) : centre et rayon, dans le bandeau. */
  disc?: { cx: number; cy: number; r: number };
  /** Rayons du contour (px), pour que l'anneau de focus, l'ombre et le halo le suivent. */
  radius: { tl: number; tr: number; br: number; bl: number };
  /** Arête haute éclairée : début et fin du trait, à l'intérieur des coins (ou de la coupe). */
  edge: [number, number];
}

const h = (v: number) => Math.round(v * 2) / 2;

/** Rectangle à coins arrondis différenciés (haut / bas), avec retouches optionnelles par côté. */
function roundedRect(s: number, rt: number, rb: number, opts: { chamferTR?: number; notch?: { x: number; w: number; d: number }; tab?: { y: number; hgt: number; d: number } } = {}): string {
  const { chamferTR = 0, notch, tab } = opts;
  const p: string[] = [];
  p.push(`M${h(rt)} 0`);
  if (chamferTR > 0) {
    p.push(`H${h(s - chamferTR)}`, `L${s} ${h(chamferTR)}`);
  } else {
    p.push(`H${h(s - rt)}`, `Q${s} 0 ${s} ${h(rt)}`);
  }
  if (tab) {
    // Ergot : petite marche arrondie qui dépasse du flanc droit.
    const r = Math.min(tab.d, tab.hgt / 2);
    p.push(`V${h(tab.y)}`, `Q${h(s + tab.d)} ${h(tab.y)} ${h(s + tab.d)} ${h(tab.y + r)}`, `V${h(tab.y + tab.hgt - r)}`, `Q${h(s + tab.d)} ${h(tab.y + tab.hgt)} ${s} ${h(tab.y + tab.hgt)}`);
  }
  p.push(`V${h(s - rb)}`, `Q${s} ${s} ${h(s - rb)} ${s}`);
  if (notch) {
    // Encoche : creux à fond arrondi dans le bord bas.
    const x1 = notch.x + notch.w, x0 = notch.x, d = notch.d, r = Math.min(d, notch.w / 2);
    p.push(`H${h(x1)}`, `V${h(s - d + r)}`, `Q${h(x1)} ${h(s - d)} ${h(x1 - r)} ${h(s - d)}`, `H${h(x0 + r)}`, `Q${h(x0)} ${h(s - d)} ${h(x0)} ${h(s - d + r)}`, `V${s}`);
  }
  p.push(`H${h(rb)}`, `Q0 ${s} 0 ${h(s - rb)}`, `V${h(rt)}`, `Q0 0 ${h(rt)} 0`, 'Z');
  return p.join(' ');
}

/** Hauteur du bandeau : ≥ 26 px pour que le nom de la console tienne en 13 px (lisible sur le Deck). */
export function bandHeight(s: number): number {
  return Math.max(26, Math.round(s * 0.14));
}

export function cartShape(c: Console | string, s: number): CartShape {
  return baseShape(c, s);
}

function withFrame(sh: Omit<CartShape, 'radius' | 'edge'>, s: number, rt: number, rb: number, cutTR = 0): CartShape {
  return { ...sh, radius: { tl: rt, tr: cutTR ? Math.min(rt, cutTR / 2) : rt, br: rb, bl: rb }, edge: [h(rt + 2), h(s - (cutTR ? cutTR : rt) - 2)] };
}

function baseShape(c: Console | string, s: number): CartShape {
  const band = bandHeight(s);
  const inset = 3;
  const base = { band, padLeft: h(band * 0.42), padRight: h(band * 0.3) };
  const win = (rTop: number, rBottom: number, left = inset) => ({ top: band, left, right: inset, bottom: inset, rTop, rBottom });
  switch (c) {
    case 'switch': {
      const rt = h(s * 0.045), rb = h(s * 0.075);
      // Lèvre de préhension : trait fin sous le bandeau, comme la marche moulée en haut d'une carte.
      return withFrame({ ...base, outline: roundedRect(s, rt, rb), grooves: `M${h(band * 0.42)} ${h(band - 3.5)}H${h(s - band * 0.3)}`, window: win(3, rb - inset) }, s, rt, rb);
    }
    case 'switch2': {
      const rt = h(s * 0.045), rb = h(s * 0.075);
      const notch = { x: h(s * 0.7), w: h(Math.max(10, s * 0.09)), d: h(Math.max(4, s * 0.03)) };
      // La fenêtre s'arrête au-dessus de l'encoche pour que le creux reste lisible.
      return withFrame({ ...base, outline: roundedRect(s, rt, rb, { notch }), grooves: `M${h(band * 0.42)} ${h(band - 3.5)}H${h(s - band * 0.3)}`, window: { ...win(3, rb - inset), bottom: h(notch.d + 2) } }, s, rt, rb);
    }
    case '3ds': {
      const r = h(s * 0.05);
      const tab = { y: h(s * 0.58), hgt: h(Math.max(10, s * 0.1)), d: h(Math.max(2.5, s * 0.016)) };
      return withFrame({ ...base, outline: roundedRect(s, r, r, { tab }), grooves: '', window: win(3, r - inset) }, s, r, r);
    }
    case 'ds': {
      const r = h(s * 0.05), cut = h(Math.max(8, s * 0.07));
      return withFrame({ ...base, padRight: h(cut * 0.75 + band * 0.18), outline: roundedRect(s, r, r, { chamferTR: cut }), grooves: '', window: win(3, r - inset) }, s, r, r, cut);
    }
    case 'gc': {
      const r = h(s * 0.085), dr = h(band * 0.27);
      const disc = { cx: h(band * 0.42 + dr), cy: h(band / 2), r: dr };
      return withFrame({ ...base, padLeft: h(band * 0.42 + dr * 2 + band * 0.2), outline: roundedRect(s, r, r), grooves: '', window: win(4, r - inset), disc }, s, r, r);
    }
    case 'wiiu': {
      const r = h(s * 0.11), spine = h(Math.max(6, s * 0.035));
      // Tranche du boîtier : la fenêtre commence après elle ; un trait marque le pli.
      return withFrame({ ...base, padLeft: h(spine + band * 0.3), outline: roundedRect(s, r, r), grooves: `M${spine} ${h(r * 0.6)}V${h(s - r * 0.6)}`, window: win(4, r - inset, spine + 1.5) }, s, r, r);
    }
    default: {
      const r = h(s * 0.07);
      return withFrame({ ...base, outline: roundedRect(s, r, r), grooves: '', window: win(3, r - inset) }, s, r, r);
    }
  }
}

/** Stries de préhension (DS, 3DS) : nombre de stries posées après le nom de la console. */
export const GRIP_RIDGES: Partial<Record<string, number>> = { ds: 5, '3ds': 3 };
