// Images de profil : personnages dessinés pour la Ludothèque (dessins originaux) et portraits de
// personnages recadrés dans les images des jeux de la collection (stockées sur le serveur, jamais redessinées).
// Chaque image donne sa couleur d'accent à l'interface : accent, fond doux (accentSoft), encre (accentInk).

import type { AvatarKind, Profile } from './types';

export const AVATARS: { kind: AvatarKind; label: string; accent: string; soft: string; ink: string }[] = [
  { kind: 'renard', label: 'Renard', accent: '#38b6ff', soft: '#123049', ink: '#9ad8ff' },
  { kind: 'champi', label: 'Champignon', accent: '#34e0a1', soft: '#12352c', ink: '#8ff0c8' },
  { kind: 'chat', label: 'Chat', accent: '#ffc83d', soft: '#3a2f12', ink: '#ffe08f' },
  { kind: 'etoile', label: 'Étoile', accent: '#9b85ff', soft: '#262046', ink: '#c9bcff' },
  { kind: 'planete', label: 'Planète', accent: '#ff6fae', soft: '#3d1a2c', ink: '#ffb3d3' },
  { kind: 'poisson', label: 'Poisson', accent: '#22d3c5', soft: '#0f3533', ink: '#8fece4' },
];

/**
 * Portrait recadré dans l'image d'un jeu (`/api/v1/games/{game}/cover`).
 * `x`, `y` : centre du personnage (fractions de la largeur et de la hauteur de l'image) ;
 * `d` : diamètre du cercle (fraction de la largeur) ; `ratio` : hauteur / largeur de l'image.
 * Recadrages choisis à l'œil sur les images du serveur ; couleurs calculées sur le disque recadré
 * (teinte vive dominante, script `accents.py` du dossier de preuve GAO-ludo-profil-001, 21/11/2026).
 */
export interface Portrait {
  slug: string;
  label: string;
  licence: string;
  game: string;
  x: number;
  y: number;
  d: number;
  ratio: number;
  accent: string;
  soft: string;
  ink: string;
}

export const PORTRAITS: Portrait[] = [
  { slug: 'mario', label: 'Mario', licence: 'Mario', game: 'switch-010036b0034e4000', x: 0.6, y: 0.51, d: 0.36, ratio: 1.0, accent: '#ff9c5c', soft: '#391f0e', ink: '#ffc199' },
  { slug: 'luigi', label: 'Luigi', licence: 'Mario', game: 'switch-0100dca0064a6000', x: 0.19, y: 0.5, d: 0.4, ratio: 1.0, accent: '#efbd6b', soft: '#39290e', ink: '#f5d6a3' },
  { slug: 'yoshi', label: 'Yoshi', licence: 'Mario', game: '3ds-3ds0650-yoshis-new-island', x: 0.27, y: 0.46, d: 0.3, ratio: 0.8906, accent: '#f8ea62', soft: '#39350e', ink: '#fbf29d' },
  { slug: 'toad', label: 'Toad', licence: 'Mario', game: '3ds-3ds1824-captain-toad-treasure-tracker', x: 0.56, y: 0.31, d: 0.3, ratio: 0.8848, accent: '#f0bb6b', soft: '#39280e', ink: '#f5d4a3' },
  { slug: 'link', label: 'Link', licence: 'Zelda', game: 'switch-01007ef00011e000', x: 0.875, y: 0.4, d: 0.14, ratio: 1.0, accent: '#2e9cea', soft: '#0e2739', ink: '#a2d3f6' },
  { slug: 'pikachu', label: 'Pikachu', licence: 'Pokémon', game: 'switch-010003f003a34000', x: 0.37, y: 0.4, d: 0.44, ratio: 1.0, accent: '#ffe75c', soft: '#39330e', ink: '#fff099' },
  { slug: 'dracaufeu', label: 'Dracaufeu', licence: 'Pokémon', game: 'switch-01004b3023412000', x: 0.72, y: 0.52, d: 0.26, ratio: 1.0, accent: '#d48255', soft: '#391d0e', ink: '#ebc3ad' },
  { slug: 'florizarre', label: 'Florizarre', licence: 'Pokémon', game: 'switch-010087c02342e000', x: 0.45, y: 0.72, d: 0.42, ratio: 1.0, accent: '#51c3d3', soft: '#0e3439', ink: '#ade3eb' },
  { slug: 'solgaleo', label: 'Solgaleo', licence: 'Pokémon', game: '3ds-3ds1606-pokemon-sun', x: 0.62, y: 0.27, d: 0.3, ratio: 0.8969, accent: '#dec77c', soft: '#392f0e', ink: '#ebdcad' },
  { slug: 'marie', label: 'Marie', licence: 'Animal Crossing', game: '3ds-3ds1626-animal-crossing-new-leaf-welcome-amiibo', x: 0.78, y: 0.8, d: 0.24, ratio: 0.8945, accent: '#d1a447', soft: '#392b0e', ink: '#ebd7ad' },
  { slug: 'kirby', label: 'Kirby', licence: 'Autres héros', game: '3ds-3ds0930-kirby-triple-deluxe', x: 0.37, y: 0.31, d: 0.32, ratio: 0.8906, accent: '#fe5d91', soft: '#390e1c', ink: '#fe9abb' },
  { slug: 'samus', label: 'Samus', licence: 'Autres héros', game: '3ds-3ds1752-metroid-samus-returns', x: 0.42, y: 0.45, d: 0.3, ratio: 0.8867, accent: '#ed412c', soft: '#39130e', ink: '#f7aba1' },
  { slug: 'pikmin', label: 'Pikmin', licence: 'Autres héros', game: '3ds-3ds1740-hey-pikmin', x: 0.45, y: 0.4, d: 0.3, ratio: 0.8945, accent: '#d1474e', soft: '#390e10', ink: '#ebadb0' },
  { slug: 'jibanyan', label: 'Jibanyan', licence: 'Autres héros', game: '3ds-3ds1487-yo-kai-watch', x: 0.14, y: 0.76, d: 0.24, ratio: 0.8906, accent: '#ec4d2c', soft: '#39160e', ink: '#f7b0a1' },
  { slug: 'layton', label: 'Professeur Layton', licence: 'Autres héros', game: '3ds-3ds0217-professeur-layton-et-le-masque-des-miracles', x: 0.4, y: 0.52, d: 0.22, ratio: 0.8906, accent: '#e9bb71', soft: '#39290e', ink: '#f1d5a7' },
  { slug: 'alear', label: 'Alear', licence: 'Autres héros', game: 'switch-0100a6301214e000', x: 0.33, y: 0.33, d: 0.26, ratio: 1.0, accent: '#2d79eb', soft: '#0e1f39', ink: '#a2c3f6' },
  { slug: 'pit', label: 'Pit', licence: 'Autres héros', game: '3ds-3ds0134-kid-icarus-uprising', x: 0.5, y: 0.62, d: 0.3, ratio: 0.8789, accent: '#7c9fde', soft: '#0e1d39', ink: '#adc3eb' },
  { slug: 'shovel', label: 'Shovel Knight', licence: 'Autres héros', game: '3ds-3ds1372-shovel-knight', x: 0.5, y: 0.55, d: 0.3, ratio: 0.8809, accent: '#3e9dda', soft: '#0e2839', ink: '#a9d3ef' },
];

/** Ordre d'affichage des familles dans le choix d'image ; les personnages maison viennent en dernier. */
export const LICENCES = ['Mario', 'Zelda', 'Pokémon', 'Animal Crossing', 'Autres héros'];

/** Préfixe d'un portrait dans `avatarKind` (« perso:pikachu »). */
export const PORTRAIT_PREFIX = 'perso:';

export function portraitOf(kind: AvatarKind | undefined): Portrait | undefined {
  if (!kind?.startsWith(PORTRAIT_PREFIX)) return undefined;
  const slug = kind.slice(PORTRAIT_PREFIX.length);
  return PORTRAITS.find((p) => p.slug === slug);
}

/** Adresse du serveur pour les images des jeux (renseignée par `api.device()` ; vide = pas d'image). */
let coverBase = '';
export function setCoverBase(url: string | undefined) {
  coverBase = (url ?? '').replace(/\/+$/, '');
}

/** Image du jeu d'un portrait ; `undefined` sans serveur connu (le disque reste à la couleur d'accent). */
export function portraitImage(p: Portrait): string | undefined {
  return coverBase ? `${coverBase}/api/v1/games/${encodeURIComponent(p.game)}/cover` : undefined;
}

/** Position de l'image dans un disque carré (pourcentages du disque) pour centrer le personnage. */
export function portraitStyle(p: Portrait): string {
  const w = 100 / p.d;
  return `width:${w}%;left:${50 - p.x * w}%;top:${50 - p.y * p.ratio * w}%`;
}

/** Le profil avec cette image et la couleur d'accent qui va avec. */
export function withAvatar(p: Profile, kind: AvatarKind): Profile {
  const portrait = portraitOf(kind);
  if (portrait) return { ...p, avatarKind: kind, accent: portrait.accent, accentSoft: portrait.soft, accentInk: portrait.ink };
  const a = AVATARS.find((x) => x.kind === kind) ?? AVATARS[0];
  return { ...p, avatarKind: a.kind, accent: a.accent, accentSoft: a.soft, accentInk: a.ink };
}
