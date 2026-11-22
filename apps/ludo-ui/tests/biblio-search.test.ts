// Tests de la recherche de la bibliothèque : `node --test tests/biblio-search.test.ts` (Node ≥ 22.18).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { distance, fold, match, pieces, playtimeSeconds } from '../src/lib/components/biblio/search.ts';

const TITLES = [
  'The Legend of Zelda: Majora’s Mask 3D',
  'The Legend of Zelda: Ocarina of Time 3D',
  'The Legend of Zelda: Breath of the Wild',
  'Pokémon Diamant',
  'Pokémon Lune',
  'Mario Kart 8 Deluxe',
  'Luigi’s Mansion 3',
  'Œuvre d’art',
];

function search(q: string): string[] {
  return TITLES.map((t) => ({ t, m: match(t, q) }))
    .filter((x) => x.m)
    .sort((a, b) => b.m!.score - a.m!.score)
    .map((x) => x.t);
}

test('accents et casse ignorés', () => {
  assert.equal(fold('Pokémon ÉTÉ'), 'pokemon ete');
  assert.deepEqual(search('pokemon').sort(), ['Pokémon Diamant', 'Pokémon Lune']);
  assert.deepEqual(search('POKÉ').sort(), ['Pokémon Diamant', 'Pokémon Lune']);
});

test('fautes légères : lettre oubliée, en trop, changée, inversée', () => {
  for (const q of ['majora', 'majoras', 'mjora', 'majroa', 'majpra']) {
    assert.equal(search(`zelda ${q}`)[0], TITLES[0], q);
  }
  assert.equal(search('pokmon')[0].startsWith('Pokémon'), true);
  assert.equal(search('mario kar')[0], 'Mario Kart 8 Deluxe');
});

test('mots courts : aucune faute admise (pas de bruit)', () => {
  assert.deepEqual(search('kat'), []);
  assert.deepEqual(search('3d').length, 2);
});

test('mots dans le désordre et apostrophes', () => {
  assert.equal(search('mask zelda')[0], TITLES[0]);
  assert.equal(search('luigis')[0], 'Luigi’s Mansion 3');
  assert.equal(search('luigi s')[0], 'Luigi’s Mansion 3');
});

test('requête vide = tout ; mot absent = rien', () => {
  assert.equal(search('').length, TITLES.length);
  assert.deepEqual(search('metroid'), []);
});

test('surlignage aux positions du titre d’origine (accents, ligatures)', () => {
  const t = 'Pokémon Lune';
  const m = match(t, 'pokem')!;
  assert.deepEqual(pieces(t, m.ranges), [{ text: 'Pokém', hit: true }, { text: 'on Lune', hit: false }]);
  const o = 'Œuvre d’art';
  const mo = match(o, 'oeuv')!;
  assert.equal(pieces(o, mo.ranges)[0].text, 'Œuv');
  const z = match(TITLES[0], 'majora')!;
  assert.equal(TITLES[0].slice(...z.ranges[0]), 'Majora');
});

test('distance de Damerau bornée', () => {
  assert.equal(distance('majora', 'majroa', 2), 1);
  assert.equal(distance('abc', 'abc', 1), 0);
  assert.equal(distance('abcdef', 'uvwxyz', 1), 2);
});

test('temps de jeu lisible → secondes', () => {
  assert.equal(playtimeSeconds('12 h 40'), 12 * 3600 + 40 * 60);
  assert.equal(playtimeSeconds('0 h 05'), 300);
  assert.equal(playtimeSeconds(undefined), 0);
});

test('300 titres filtrés en moins de 30 ms', () => {
  const many = Array.from({ length: 300 }, (_, i) => `${TITLES[i % TITLES.length]} ${i}`);
  const t0 = performance.now();
  for (const q of ['z', 'ze', 'zel', 'zeld', 'zelda', 'zelda m', 'zelda ma', 'zelda maj']) many.forEach((t) => match(t, q));
  const per = (performance.now() - t0) / 8;
  assert.ok(per < 30, `${per.toFixed(2)} ms par frappe`);
});
