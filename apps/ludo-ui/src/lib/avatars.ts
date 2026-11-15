// Personnages des images de profil et la couleur d'accent que chacun donne à l'interface.
// Accent, fond doux (accentSoft) et encre (accentInk) : contraste vérifié à l'œil sur fond sombre.

import type { AvatarKind, Profile } from './types';

export const AVATARS: { kind: AvatarKind; label: string; accent: string; soft: string; ink: string }[] = [
  { kind: 'renard', label: 'Renard', accent: '#38b6ff', soft: '#123049', ink: '#9ad8ff' },
  { kind: 'champi', label: 'Champignon', accent: '#34e0a1', soft: '#12352c', ink: '#8ff0c8' },
  { kind: 'chat', label: 'Chat', accent: '#ffc83d', soft: '#3a2f12', ink: '#ffe08f' },
  { kind: 'etoile', label: 'Étoile', accent: '#9b85ff', soft: '#262046', ink: '#c9bcff' },
  { kind: 'planete', label: 'Planète', accent: '#ff6fae', soft: '#3d1a2c', ink: '#ffb3d3' },
  { kind: 'poisson', label: 'Poisson', accent: '#22d3c5', soft: '#0f3533', ink: '#8fece4' },
];

/** Le profil avec ce personnage et la couleur d'accent qui va avec. */
export function withAvatar(p: Profile, kind: AvatarKind): Profile {
  const a = AVATARS.find((x) => x.kind === kind) ?? AVATARS[0];
  return { ...p, avatarKind: kind, accent: a.accent, accentSoft: a.soft, accentInk: a.ink };
}
