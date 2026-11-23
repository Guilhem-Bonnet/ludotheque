// Gabarits d'écran : une mise en page par rapport largeur/hauteur (lot « ecrans », 21/11/2026).
//
// L'interface est dessinée pour une vue logique d'environ 1280×800 (Steam Deck). Le zoom natif de la vue
// (App.svelte) agrandit cette vue sur les grands écrans ; ce module décide :
//   - du zoom (piloté par la hauteur, plafonné pour garder textes et jaquettes nets et montrer plus de contenu) ;
//   - du gabarit (`data-format` sur <html>) et des colonnes latérales (`data-side`) ;
//   - des variables CSS de mise en page que tous les écrans peuvent lire (voir app.css).
//
// Variables exposées sur <html> (px logiques) :
//   --page-w, --page-h     taille de la vue logique
//   --stage-max            largeur maximale d'un écran de lecture (listes, réglages) au centre
//   --side-w               largeur d'une colonne latérale (0 sans colonne)
//   --gutter               marge latérale de référence (48 px, 32 px en portrait)
// Attributs : data-format = portrait | compact | large | ultra | super ; data-side = none | right | both | bottom.
//
// Aucun calcul périodique : tout part de l'événement `resize` (App.svelte).

import { writable } from 'svelte/store';

export type Format = 'portrait' | 'compact' | 'large' | 'ultra' | 'super';
export type Side = 'none' | 'right' | 'both' | 'bottom';

export interface Layout {
  format: Format;
  /** Rapport largeur / hauteur de la vue. */
  ratio: number;
  /** Taille de la vue logique (px CSS vus par la page). */
  w: number;
  h: number;
  /** Colonnes latérales disponibles pour les écrans qui en ont (accueil, fiche). */
  side: Side;
  sideW: number;
  /** Hauteur du panneau du bas en portrait (0 sinon). */
  bottomH: number;
  stageMax: number;
  gutter: number;
  /** `bas` : écran du bas de la Thor en double écran (vitrine du jeu sur l'écran du haut). */
  ecran?: EcranMode;
}

/** Hauteur logique de référence : 800 (Deck, 1080p), 900 dès que l'écran dépasse 1100 px de haut (1440p, 4K). */
export const BASE_W = 1280;
export function baseHeight(physH: number): number {
  return physH > 1100 ? 900 : 800;
}
/** Zoom maximal : au-delà, les jaquettes (images ~512 px) s'agrandiraient au point de flouter. */
export const ZOOM_MAX = 2.25;
/** Zoom minimal (petite fenêtre de bureau) : en dessous, l'interface ne tient plus. */
export const ZOOM_MIN = 0.75;
/** Plus petit corps de texte admis, en px réels (à zoom ≥ 1, un texte de 13 px logiques suffit). */
export const TEXT_MIN_PX = 13;

/**
 * Zoom natif pour une fenêtre de `physW`×`physH` px « 100 % » : la vue logique garde au moins 1280×800
 * (ou 1280×900 sur les grands écrans), le reste de la largeur sert aux colonnes et aux tuiles en plus.
 */
export function computeZoom(physW: number, physH: number): number {
  if (!(physW > 0 && physH > 0)) return 1;
  const z = Math.min(physW / BASE_W, physH / baseHeight(physH));
  return Math.round(Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, z)) * 1000) / 1000;
}

/**
 * Taille physique d'un px CSS du Steam Deck, en dp Android : 1280 px sur la largeur d'un écran 7" 1280×800
 * (≈ 215 ppp) = 0,118 mm ; 1 dp = 1/160 po = 0,159 mm → 0,744 dp. L'interface est dessinée et réglée pour le
 * Deck (« 13 px minimum », cibles de 44 px) : à cette échelle, ces règles gardent le même sens physique.
 */
export const DECK_PX_DP = 0.744;

/** Mode d'affichage annoncé par la coque Android (DoubleEcran.kt) : `bas` = écran du bas de la Thor en double écran. */
export type EcranMode = 'bas' | 'seul' | undefined;

/** État annoncé par DoubleEcran.kt (`window.__ludoEcran`, commande `ecranEtat`) : mode et taille de la vue en dp. */
export interface EcranInfo {
  mode?: EcranMode;
  vueLargeurDp?: number;
  vueHauteurDp?: number;
}

/**
 * Largeur logique de la page sur Android. Taille de la vue annoncée par la coque (dp) d'abord : elle ne dépend
 * pas de la balise viewport. `window.outerWidth`, lui, la suit sur la WebView Android : relu après la pose de la
 * balise, il donnait un point fixe sur une mauvaise largeur (écran du bas petit à CHAQUE lancement, 22/11 soir).
 */
export function androidPageWidth(info: EcranInfo | undefined, outerW: number, outerH: number): number {
  const mode = info?.mode;
  const w = info?.vueLargeurDp ?? 0;
  const h = info?.vueHauteurDp ?? 0;
  if (w > 0 && h > 0) return androidViewportWidth(w, h, mode);
  return androidViewportWidth(outerW, outerH, mode);
}

/**
 * Largeur logique (balise viewport) de la WebView Android pour une fenêtre de `outerW`×`outerH` dp.
 * - Écran du bas de la Thor (`bas`, 538×469 dp en paysage) : 1 px CSS = 1 px du Deck (≈ 723×630 px logiques).
 *   Mesuré le 22/11 avec l'ancienne règle (1280 px imposés) : 0,97 px physique par px CSS pour une densité de
 *   2,31, soit une interface 2,4× plus petite qu'une appli Android et ~56 % de sa taille sur le Deck.
 * - Un seul écran (Thor écran du haut, autres appareils) : règle d'origine, au moins 1280×800 visibles.
 */
export function androidViewportWidth(outerW: number, outerH: number, mode: EcranMode): number {
  const ratio = outerW > 0 && outerH > 0 ? outerW / outerH : 16 / 10;
  if (mode === 'bas' && outerW > 0) return Math.round(outerW / DECK_PX_DP);
  return Math.ceil(Math.max(BASE_W, 800 * ratio));
}

/** Gabarit d'après le rapport largeur/hauteur : 16:10 et 3:2 → compact, 16:9 → large, 21:9 → ultra, 32:9 → super. */
export function formatFor(ratio: number): Format {
  if (ratio < 0.9) return 'portrait';
  if (ratio < 1.7) return 'compact';
  if (ratio < 2.05) return 'large';
  if (ratio < 2.9) return 'ultra';
  return 'super';
}

/** Mise en page pour une vue logique de `w`×`h` px. */
export function computeLayout(w: number, h: number, mode?: EcranMode): Layout {
  const ratio = w > 0 && h > 0 ? w / h : 16 / 10;
  const format = formatFor(ratio);
  // Écran du bas de la Thor (≈ 723 px logiques) : marge de 24 px, la largeur va aux tuiles.
  const gutter = mode === 'bas' ? 24 : format === 'portrait' ? 32 : 48;
  // Colonne latérale : 452 px de contenu + sa marge ; l'écran garde toujours au moins 1280 px.
  const sideW = 452;
  let side: Side = 'none';
  if (format === 'super' && w - 2 * (sideW + gutter) >= BASE_W) side = 'both';
  else if ((format === 'ultra' || format === 'super') && w - (sideW + gutter) >= BASE_W) side = 'right';
  // Portrait : l'écran garde sa hauteur de référence (800 à 1280 de large), le panneau prend le reste.
  let bottomH = 0;
  if (format === 'portrait') {
    const stageH = Math.max(800, Math.min(1000, Math.round(h * 0.56)));
    if (h - stageH >= 360) {
      side = 'bottom';
      bottomH = h - stageH;
    }
  }
  // Écrans de lecture (listes, réglages, conflit) : centrés, jamais plus larges que 1760 px.
  const stageMax = 1760;
  return { format, ratio, w, h, side, sideW: side === 'none' || side === 'bottom' ? 0 : sideW, bottomH, stageMax, gutter, ecran: mode };
}

/** Écrans qui reçoivent les colonnes latérales (les autres sont centrés ou pleine largeur). */
export function screenHasSide(screen: string): boolean {
  return screen === 'accueil' || screen === 'fiche';
}

/** Écrans pleine largeur sur ultrawide : plus de jaquettes par rangée plutôt qu'un écran centré. */
export function screenIsWide(screen: string): boolean {
  return screen === 'bibliotheque';
}

const initial = computeLayout(BASE_W, 800);
/** Mise en page courante (lecture : `$layout` dans un composant). */
export const layout = writable<Layout>(initial);

/** Applique la mise en page : attributs et variables CSS sur <html>, puis la diffuse aux composants. */
export function applyLayout(l: Layout, root: HTMLElement = document.documentElement): void {
  root.dataset.format = l.format;
  root.dataset.side = l.side;
  const s = root.style;
  s.setProperty('--page-w', `${l.w}px`);
  s.setProperty('--page-h', `${l.h}px`);
  s.setProperty('--side-w', `${l.sideW}px`);
  s.setProperty('--bottom-h', `${l.bottomH}px`);
  s.setProperty('--stage-max', `${l.stageMax}px`);
  s.setProperty('--gutter', `${l.gutter}px`);
  layout.set(l);
}
