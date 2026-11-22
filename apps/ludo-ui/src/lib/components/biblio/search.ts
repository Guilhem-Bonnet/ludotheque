// Recherche de la bibliothèque : tolérante aux accents, à la casse, à la ponctuation et aux fautes
// légères (une lettre oubliée, en trop, changée ou deux lettres inversées).
//
// Principe : le titre et la requête sont ramenés à des mots simples (« Pokémon : Lune » → pokemon,
// lune). Chaque mot de la requête doit retrouver un mot du titre : début exact (le plus fort),
// contenu, puis début approché (distance d'édition ≤ 1, ≤ 2 à partir de 7 lettres). Le score
// classe les résultats ; `ranges` dit quelles lettres du titre surligner.

/** Minuscule, sans accents ni ligatures, apostrophes et ponctuation ramenées à des espaces. */
export function fold(s: string): string {
  return s
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')
    .replace(/œ/gi, 'oe')
    .replace(/æ/gi, 'ae')
    .replace(/ß/g, 'ss')
    .toLowerCase();
}

/**
 * Mots d'un texte replié. `at` et `map` renvoient au texte d'origine : `map[i]` = position, dans
 * l'original, de la lettre i du texte replié (une ligature « œ » donne deux lettres, un accent
 * décomposé aucune).
 */
export function words(s: string): { w: string; at: number; map: number[] }[] {
  let f = '';
  const map: number[] = [];
  for (let i = 0; i < s.length; i++) {
    const part = fold(s[i]);
    for (let k = 0; k < part.length; k++) map.push(i);
    f += part;
  }
  map.push(s.length);
  const out: { w: string; at: number; map: number[] }[] = [];
  const re = /[a-z0-9]+/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(f))) out.push({ w: m[0], at: m.index, map });
  return out;
}

/** Distance de Damerau-Levenshtein restreinte (transposition de deux lettres voisines = 1), bornée par `max`. */
export function distance(a: string, b: string, max: number): number {
  if (Math.abs(a.length - b.length) > max) return max + 1;
  const n = b.length;
  let prev2 = new Array<number>(n + 1).fill(0);
  let prev = Array.from({ length: n + 1 }, (_, j) => j);
  for (let i = 1; i <= a.length; i++) {
    const cur = new Array<number>(n + 1);
    cur[0] = i;
    let rowMin = cur[0];
    for (let j = 1; j <= n; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      let v = Math.min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + cost);
      if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) v = Math.min(v, prev2[j - 2] + 1);
      cur[j] = v;
      if (v < rowMin) rowMin = v;
    }
    if (rowMin > max) return max + 1;
    prev2 = prev;
    prev = cur;
  }
  return prev[n];
}

/** Fautes admises pour un mot tapé : aucune sous 4 lettres, 1 jusqu'à 6, 2 au-delà. */
function tolerance(len: number): number {
  return len < 4 ? 0 : len < 7 ? 1 : 2;
}

export interface Match {
  score: number;
  /** Plages [début, fin[ du titre d'origine à surligner. */
  ranges: [number, number][];
}

/**
 * Le titre correspond-il à la requête ? `null` sinon. Score plus haut = meilleur résultat.
 * Requête vide : tout correspond (score 0).
 */
export function match(title: string, query: string): Match | null {
  const q = words(query);
  if (!q.length) return { score: 0, ranges: [] };
  const t = words(title);
  if (!t.length) return null;
  let score = 0;
  const ranges: [number, number][] = [];
  const used = new Set<number>();
  for (const { w } of q) {
    let best = -1;
    let bestScore = 0;
    let bestLen = 0;
    t.forEach((tw, i) => {
      let s = 0;
      let len = 0;
      if (tw.w === w) { s = 100; len = w.length; }
      else if (tw.w.startsWith(w)) { s = 80; len = w.length; }
      else if (tw.w.includes(w) && w.length >= 3) { s = 50; len = 0; }
      else {
        const tol = tolerance(w.length);
        if (tol) {
          // Début approché : le mot tapé contre le début du mot du titre de même longueur (±1).
          let d = tol + 1;
          for (let l = Math.max(1, w.length - 1); l <= Math.min(tw.w.length, w.length + 1); l++) {
            d = Math.min(d, distance(w, tw.w.slice(0, l), tol));
          }
          if (d <= tol) { s = 40 - 10 * d; len = Math.min(w.length, tw.w.length); }
        }
      }
      // Un mot déjà retenu pour un autre mot de la requête compte moins (« zelda zelda »).
      if (s && used.has(i)) s = Math.floor(s / 4);
      if (s > bestScore) { bestScore = s; best = i; bestLen = len; }
    });
    if (best < 0) return null;
    used.add(best);
    score += bestScore;
    const tw = t[best];
    const a = bestLen ? tw.at : tw.at + tw.w.indexOf(w);
    const b = a + (bestLen || w.length);
    // Retour aux positions du titre d'origine (la dernière lettre surlignée est incluse en entier).
    ranges.push([tw.map[a], tw.map[b - 1] + 1]);
  }
  // Ordre des mots respecté et titre court : léger bonus (le titre « exact » remonte).
  score += Math.max(0, 20 - t.length);
  return { score, ranges: merge(ranges) };
}

function merge(r: [number, number][]): [number, number][] {
  const s = [...r].sort((a, b) => a[0] - b[0]);
  const out: [number, number][] = [];
  for (const x of s) {
    const last = out[out.length - 1];
    if (last && x[0] <= last[1]) last[1] = Math.max(last[1], x[1]);
    else out.push([x[0], x[1]]);
  }
  return out;
}

/** Découpe un titre en morceaux surlignés ou non, pour l'affichage. */
export function pieces(title: string, ranges: [number, number][]): { text: string; hit: boolean }[] {
  const out: { text: string; hit: boolean }[] = [];
  let at = 0;
  for (const [a, b] of ranges) {
    if (a > at) out.push({ text: title.slice(at, a), hit: false });
    out.push({ text: title.slice(a, b), hit: true });
    at = b;
  }
  if (at < title.length) out.push({ text: title.slice(at), hit: false });
  return out;
}

/** « 12 h 40 » → secondes (tri par temps de jeu) ; absent ou illisible = 0. */
export function playtimeSeconds(label: string | undefined): number {
  const m = label?.match(/(\d+)\s*h\s*(\d+)?/);
  return m ? Number(m[1]) * 3600 + Number(m[2] ?? 0) * 60 : 0;
}
