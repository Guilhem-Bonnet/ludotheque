import { test } from 'node:test';
import assert from 'node:assert/strict';
import { raisonSansArretDirect } from '../src/lib/arretDirect.ts';

test('arrêt direct prêt ou ancienne coque : aucune raison affichée', () => {
  assert.equal(raisonSansArretDirect('pret'), null);
  assert.equal(raisonSansArretDirect(undefined), null);
});

test('raisons lisibles, sans pronom ni tutoiement', () => {
  for (const etat of ['absent', 'arrete', 'refuse', 'ancien', 'inconnu']) {
    const t = raisonSansArretDirect(etat);
    assert.ok(t && t.startsWith('Arrêt direct indisponible'), etat);
    assert.doesNotMatch(t!, /\b(tu|te|ton|ta|tes|vous|votre|vos)\b/i, etat);
  }
  assert.match(raisonSansArretDirect('arrete')!, /Brancher l’appareil au PC/);
});
