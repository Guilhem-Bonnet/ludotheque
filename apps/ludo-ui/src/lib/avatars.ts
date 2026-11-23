// Images de profil : personnages dessinés pour la Ludothèque (dessins originaux) et galerie d'images reprises
// telles quelles de bibliothèques publiques (icônes de profil Nintendo Switch Online, sprites animés PokeAPI,
// icônes SteamGridDB), listées dans `avatars-galerie.json` (généré par `scripts/avatars.py`) et servies par le
// serveur (`/api/v1/avatars/{slug}`, version fixe `/fixe`). Aucune image n'est découpée ni redessinée.
//
// Couleur d'accent : tirée de l'image (« automatique », défaut) ou choisie dans une palette ; un accent choisi
// reste en place quand l'image change (`accentAuto: false`).

import galerie from './avatars-galerie.json' with { type: 'json' };
import type { AvatarKind, Profile } from './types';

export const AVATARS: { kind: AvatarKind; label: string; accent: string; soft: string; ink: string }[] = [
  { kind: 'renard', label: 'Renard', accent: '#38b6ff', soft: '#123049', ink: '#9ad8ff' },
  { kind: 'champi', label: 'Champignon', accent: '#34e0a1', soft: '#12352c', ink: '#8ff0c8' },
  { kind: 'chat', label: 'Chat', accent: '#ffc83d', soft: '#3a2f12', ink: '#ffe08f' },
  { kind: 'etoile', label: 'Étoile', accent: '#9b85ff', soft: '#262046', ink: '#c9bcff' },
  { kind: 'planete', label: 'Planète', accent: '#ff6fae', soft: '#3d1a2c', ink: '#ffb3d3' },
  { kind: 'poisson', label: 'Poisson', accent: '#22d3c5', soft: '#0f3533', ink: '#8fece4' },
];

/** Image de la galerie (une entrée de `avatars-galerie.json`). */
export interface GalleryImage {
  slug: string;
  label: string;
  famille: string;
  /** GIF animé : joué seulement au focus et sur la carte du profil, version fixe ailleurs. */
  anime: boolean;
  accent: string;
  soft: string;
  ink: string;
  source: string;
}

export const GALLERY: GalleryImage[] = galerie.images;

/** Ordre des familles dans le choix d'image ; les personnages maison viennent en dernier. */
export const FAMILLES: string[] = galerie.familles;

/** Préfixe d'une image de la galerie dans `avatarKind` (« perso:pokemon-pikachu »). */
export const GALLERY_PREFIX = 'perso:';

/**
 * Anciens portraits découpés dans les jaquettes (avant le 22/11/2026) → image équivalente de la galerie.
 * Un profil qui en a un garde un choix lisible sans rien réécrire ; le prochain choix enregistre le nouveau nom.
 */
const ANCIENS: Record<string, string> = {
  mario: 'mario-mario', luigi: 'mario-luigi', yoshi: 'mario-yoshi', toad: 'mario-toad', link: 'zelda-link',
  pikachu: 'pokemon-pikachu', dracaufeu: 'pokemon-anime-dracaufeu', florizarre: 'pokemon-anime-florizarre',
  solgaleo: 'pokemon-anime-solgaleo', marie: 'animal-crossing-marie', kirby: 'kirby-kirby', samus: 'metroid-samus',
  pikmin: 'autres-heros-pikmin', jibanyan: 'autres-heros-whisper', layton: 'autres-heros-professeur-layton',
  alear: 'autres-heros-alear', pit: 'autres-heros-pit', shovel: 'autres-heros-shovel-knight',
};

const bySlug = new Map(GALLERY.map((g) => [g.slug, g]));

export function galleryOf(kind: AvatarKind | undefined): GalleryImage | undefined {
  if (!kind?.startsWith(GALLERY_PREFIX)) return undefined;
  const slug = kind.slice(GALLERY_PREFIX.length);
  return bySlug.get(slug) ?? bySlug.get(ANCIENS[slug] ?? '');
}

/** Nom d'image à jour (ancien portrait → image de la galerie) ; sert à reconnaître le choix actuel. */
export function currentKind(kind: AvatarKind | undefined): AvatarKind | undefined {
  const g = galleryOf(kind);
  return g ? (`${GALLERY_PREFIX}${g.slug}` as AvatarKind) : kind;
}

/** Adresse du serveur pour les images (renseignée par `api.device()` ; vide = pas d'image). */
let imageBase = '';
export function setCoverBase(url: string | undefined) {
  imageBase = (url ?? '').replace(/\/+$/, '');
}

/** Adresse du serveur des images (transmise à l'écran du haut de la Thor, qui n'appelle pas `api.device()`). */
export function coverBase(): string {
  return imageBase;
}

/** Adresse de l'image : animée (`live`) ou fixe ; `undefined` sans serveur connu (disque à la couleur d'accent). */
export function galleryImage(g: GalleryImage, live: boolean): string | undefined {
  if (!imageBase) return undefined;
  const base = `${imageBase}/api/v1/avatars/${encodeURIComponent(g.slug)}`;
  return g.anime && !live ? `${base}/fixe` : base;
}

/** Nom affiché de l'image du profil. */
export function avatarLabel(kind: AvatarKind | undefined): string {
  return galleryOf(kind)?.label ?? AVATARS.find((a) => a.kind === kind)?.label ?? 'Initiale';
}

/** Couleurs propres à une image (accent automatique). */
function imageColors(kind: AvatarKind): { kind: AvatarKind; accent: string; soft: string; ink: string } {
  const g = galleryOf(kind);
  if (g) return { kind: `${GALLERY_PREFIX}${g.slug}` as AvatarKind, accent: g.accent, soft: g.soft, ink: g.ink };
  const a = AVATARS.find((x) => x.kind === kind) ?? AVATARS[0];
  return { kind: a.kind, accent: a.accent, soft: a.soft, ink: a.ink };
}

/** Le profil avec cette image ; l'accent suit l'image sauf s'il a été choisi à la main. */
export function withAvatar(p: Profile, kind: AvatarKind): Profile {
  const c = imageColors(kind);
  if (p.accentAuto === false) return { ...p, avatarKind: c.kind };
  return { ...p, avatarKind: c.kind, accent: c.accent, accentSoft: c.soft, accentInk: c.ink, accentAuto: true };
}

/**
 * Palette de l'accent choisi : 12 teintes vives qui tiennent le contraste sur le fond sombre de l'interface
 * (texte sombre #0b0f14 posé sur l'accent : rapport ≥ 7:1, vérifié par `contrastRatio` dans les tests),
 * chacune avec son fond doux et son encre claire.
 */
export const ACCENTS: { name: string; accent: string; soft: string; ink: string }[] = [
  { name: 'Rouge', accent: '#ff7a7a', soft: '#3d1616', ink: '#ffb3b3' },
  { name: 'Corail', accent: '#ff8c5a', soft: '#3d2014', ink: '#ffc4a8' },
  { name: 'Orange', accent: '#ffab3d', soft: '#3d2a10', ink: '#ffd39a' },
  { name: 'Jaune', accent: '#ffd83d', soft: '#3a3210', ink: '#ffeb9a' },
  { name: 'Vert pomme', accent: '#a6e34a', soft: '#26350f', ink: '#d4f2a3' },
  { name: 'Vert', accent: '#34e0a1', soft: '#12352c', ink: '#8ff0c8' },
  { name: 'Turquoise', accent: '#22d3c5', soft: '#0f3533', ink: '#8fece4' },
  { name: 'Bleu ciel', accent: '#38b6ff', soft: '#123049', ink: '#9ad8ff' },
  { name: 'Bleu', accent: '#7a9cff', soft: '#1a2448', ink: '#bccbff' },
  { name: 'Violet', accent: '#b08cff', soft: '#2a2048', ink: '#d8c6ff' },
  { name: 'Rose', accent: '#ff6fae', soft: '#3d1a2c', ink: '#ffb3d3' },
  { name: 'Mauve', accent: '#e08cff', soft: '#361f42', ink: '#f0c6ff' },
];

/** Le profil avec cet accent choisi à la main, ou `null` : accent automatique d'après l'image. */
export function withAccent(p: Profile, accent: string | null): Profile {
  if (accent === null) {
    const c = imageColors(p.avatarKind ?? AVATARS[0].kind);
    return { ...p, accent: c.accent, accentSoft: c.soft, accentInk: c.ink, accentAuto: true };
  }
  const a = ACCENTS.find((x) => x.accent === accent) ?? ACCENTS[0];
  return { ...p, accent: a.accent, accentSoft: a.soft, accentInk: a.ink, accentAuto: false };
}

/** Rapport de contraste WCAG entre deux couleurs #rrggbb. */
export function contrastRatio(a: string, b: string): number {
  const lum = (hex: string) => {
    const [r, g, bl] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
    return 0.2126 * r + 0.7152 * g + 0.0722 * bl;
  };
  const [x, y] = [lum(a), lum(b)].sort((m, n) => n - m);
  return (x + 0.05) / (y + 0.05);
}
