// Tests de la galerie d'images de profil (lib/avatars.ts) : `node --test tests/avatars.test.ts`.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ACCENTS, AVATARS, contrastRatio, currentKind, GALLERY, galleryImage, galleryOf, setCoverBase, withAccent, withAvatar } from '../src/lib/avatars.ts';

const base = { id: 'claire', name: 'Claire', avatarKind: 'champi' as const, accent: '#34e0a1', accentSoft: '#12352c', accentInk: '#8ff0c8' };

test('galerie : slugs uniques et acceptés par le serveur (perso:<slug>, 1 à 40 caractères a-z 0-9 -)', () => {
  const slugs = GALLERY.map((g) => g.slug);
  assert.equal(new Set(slugs).size, slugs.length);
  for (const s of slugs) assert.match(s, /^[a-z0-9-]{1,40}$/);
  for (const g of GALLERY) for (const c of [g.accent, g.soft, g.ink]) assert.match(c, /^#[0-9a-f]{6}$/);
});

test('anciens portraits découpés : tous retrouvent une image de la galerie', () => {
  for (const old of ['mario', 'luigi', 'yoshi', 'toad', 'link', 'pikachu', 'dracaufeu', 'florizarre', 'solgaleo', 'marie', 'kirby', 'samus', 'pikmin', 'jibanyan', 'layton', 'alear', 'pit', 'shovel']) {
    const g = galleryOf(`perso:${old}`);
    assert.ok(g, old);
    assert.equal(currentKind(`perso:${old}`), `perso:${g!.slug}`);
  }
  assert.equal(galleryOf('perso:inconnu'), undefined);
  assert.equal(currentKind('renard'), 'renard');
});

test('accent automatique : suit l\'image ; accent choisi : reste quand l\'image change', () => {
  const pika = GALLERY.find((g) => g.slug === 'pokemon-pikachu')!;
  const auto = withAvatar(base, 'perso:pokemon-pikachu');
  assert.equal(auto.accent, pika.accent);
  assert.equal(auto.accentAuto, true);
  const choisi = withAccent(auto, ACCENTS[9].accent);
  assert.equal(choisi.accentAuto, false);
  const ensuite = withAvatar(choisi, 'renard');
  assert.equal(ensuite.avatarKind, 'renard');
  assert.equal(ensuite.accent, ACCENTS[9].accent);
  const retour = withAccent(ensuite, null);
  assert.equal(retour.accent, AVATARS[0].accent);
  assert.equal(retour.accentAuto, true);
});

test('palette : 12 teintes lisibles (texte sombre sur l\'accent ≥ 7:1, accent sur fond sombre ≥ 4.5:1)', () => {
  assert.equal(ACCENTS.length, 12);
  for (const a of ACCENTS) {
    assert.ok(contrastRatio(a.accent, '#0b0f14') >= 7, `${a.name} ${contrastRatio(a.accent, '#0b0f14').toFixed(2)}`);
    assert.ok(contrastRatio(a.accent, '#161a22') >= 4.5, a.name);
  }
});

test('adresse des images : fixe hors focus pour une image animée, rien sans serveur', () => {
  const anime = GALLERY.find((g) => g.anime)!;
  const fixe = GALLERY.find((g) => !g.anime)!;
  setCoverBase(undefined);
  assert.equal(galleryImage(anime, true), undefined);
  setCoverBase('https://serveur.exemple/');
  assert.equal(galleryImage(anime, false), `https://serveur.exemple/api/v1/avatars/${anime.slug}/fixe`);
  assert.equal(galleryImage(anime, true), `https://serveur.exemple/api/v1/avatars/${anime.slug}`);
  assert.equal(galleryImage(fixe, false), `https://serveur.exemple/api/v1/avatars/${fixe.slug}`);
});
