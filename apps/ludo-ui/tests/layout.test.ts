// Tests des gabarits d'écran (lib/layout.ts) : `node --test tests/layout.test.ts` (Node ≥ 22.18, types retirés nativement).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { computeLayout, computeZoom, formatFor, TEXT_MIN_PX, ZOOM_MAX } from '../src/lib/layout.ts';

// Formats cibles du brief : [largeur, hauteur, zoom attendu, format, colonnes].
const CASES: [number, number, number, string, string][] = [
  [1280, 800, 1, 'compact', 'none'], // Steam Deck
  [1920, 1080, 1.35, 'large', 'none'], // 1080p, flux Moonlight
  [2560, 1440, 1.6, 'large', 'none'], // 1440p
  [3440, 1440, 1.6, 'ultra', 'right'], // ultrawide du PC
  [2560, 1080, 1.35, 'ultra', 'right'], // ultrawide 1080
  [5120, 1440, 1.6, 'super', 'both'], // super-ultrawide
  [3840, 2160, ZOOM_MAX, 'large', 'none'], // 4K : zoom plafonné
];

for (const [w, h, z, format, side] of CASES) {
  test(`${w}×${h} → zoom ${z}, ${format}, colonnes ${side}`, () => {
    const zoom = computeZoom(w, h);
    assert.equal(zoom, z);
    const l = computeLayout(w / zoom, h / zoom);
    assert.equal(l.format, format);
    assert.equal(l.side, side);
    // L'écran garde toujours au moins la largeur de référence à côté des colonnes.
    const stage = l.w - (l.side === 'both' ? 2 : l.side === 'right' ? 1 : 0) * (l.sideW + l.gutter);
    assert.ok(stage >= 1280 - 0.5, `écran ${stage} px < 1280`);
    // Texte de 13 px logiques ≥ 13 px réels à tous les formats cibles.
    assert.ok(TEXT_MIN_PX * zoom >= 13);
  });
}

test('Ayn Thor : écran principal 1080×1920 et second écran 1080×1240 en portrait, panneau du bas', () => {
  for (const [w, h] of [[1080, 1920], [1080, 1240]]) {
    // Android : largeur logique imposée par la balise viewport (App.svelte) = max(1280, 800 × ratio).
    const lw = Math.max(1280, 800 * (w / h));
    const l = computeLayout(lw, lw * (h / w));
    assert.equal(l.format, 'portrait');
    assert.equal(l.side, 'bottom');
    assert.ok(l.h - l.bottomH >= 800, 'écran au moins 800 px de haut');
  }
});

test('bornes des formats', () => {
  assert.equal(formatFor(16 / 10), 'compact');
  assert.equal(formatFor(16 / 9), 'large');
  assert.equal(formatFor(21 / 9), 'ultra');
  assert.equal(formatFor(32 / 9), 'super');
  assert.equal(formatFor(9 / 16), 'portrait');
});

test('petite fenêtre : zoom plancher, entrée invalide → 1', () => {
  assert.equal(computeZoom(800, 500), 0.75);
  assert.equal(computeZoom(0, 0), 1);
});
