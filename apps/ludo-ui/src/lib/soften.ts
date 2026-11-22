// Fond d'ambiance flouté à petite taille, au lieu d'un `filter: blur()` CSS sur l'image plein écran.
//
// Pourquoi : un flou CSS de 18 px sur une image plein écran se recalcule à chaque repeinture de la page.
// Mesuré le 21/11 (WebKitGTK 2.54, 1280×800, evidence GAO-ludo-peaufi-001/mesures/cout-flou-variantes-1280.txt) :
// sous une repeinture plein écran, flou CSS = 50 images en 6 s (un cœur saturé) ; image réduite floutée à
// petite taille puis agrandie = 352 images en 6 s, le même coût qu'une image sans flou (356).
//
// Méthode : l'image est recadrée (« cover ») et réduite d'un facteur `scale` dans un petit canvas (réductions
// successives par moitié : chacune moyenne des blocs de 2×2 pixels) ; le flou CSS restant ne porte que sur
// ce petit canvas (≈ 1,5 px), puis un `transform: scale()` l'étire à la taille du fond. WebKitGTK n'a pas
// `ctx.filter`, et les jaquettes viennent du serveur (autre origine : `getImageData` interdit) : seul
// `drawImage` sert. Un agrandissement par paliers dans le canvas a été essayé : il laisse des blocs
// (captures/essai-flou-variantes.png), d'où le flou final en CSS sur la petite surface.

/** Flou CSS appliqué sur la petite surface (px) : assez pour effacer les pixels de la réduction. */
export const SMALL_BLUR = 1.5;
/** Marge (px de la petite surface) au-delà du bord visible : le flou y fond vers le transparent, hors champ. */
export const MARGIN = 3;

function canvas(w: number, h: number) {
  const c = document.createElement('canvas');
  c.width = Math.max(1, Math.round(w));
  c.height = Math.max(1, Math.round(h));
  return c;
}

function ctx2d(c: HTMLCanvasElement) {
  const x = c.getContext('2d')!;
  x.imageSmoothingEnabled = true;
  x.imageSmoothingQuality = 'high';
  return x;
}

/** Géométrie (pure, testable) : facteur d'agrandissement et taille de la petite surface, marges comprises. */
export function plan(w: number, h: number, blur: number) {
  const scale = Math.max(1, blur / SMALL_BLUR);
  return { scale, w: Math.max(4, Math.round(w / scale) + 2 * MARGIN), h: Math.max(4, Math.round(h / scale) + 2 * MARGIN) };
}

/**
 * Dessine dans `out` l'image `img` recadrée pour un fond de `w`×`h` px CSS, réduite pour un flou final
 * d'environ `blur` px. Renvoie le facteur à appliquer en `transform: scale()` et le décalage (px CSS,
 * négatif) du coin haut gauche, marges comprises. `out` reçoit sa taille CSS (petite).
 */
export function soften(img: HTMLImageElement, out: HTMLCanvasElement, w: number, h: number, blur: number) {
  const iw = img.naturalWidth;
  const ih = img.naturalHeight;
  if (!iw || !ih || w < 1 || h < 1) return null;
  const p = plan(w, h, blur);
  // Recadrage « cover » à la proportion de la petite surface (marges comprises).
  const ar = p.w / p.h;
  let sw = iw, sh = ih;
  if (iw / ih > ar) sw = ih * ar;
  else sh = iw / ar;
  let src: CanvasImageSource = img;
  let rect = [(iw - sw) / 2, (ih - sh) / 2, sw, sh];
  let cw = sw, ch = sh;
  while (cw / 2 > p.w) {
    cw /= 2;
    ch /= 2;
    const c = canvas(cw, ch);
    ctx2d(c).drawImage(src, rect[0], rect[1], rect[2], rect[3], 0, 0, c.width, c.height);
    src = c;
    rect = [0, 0, c.width, c.height];
  }
  out.width = p.w;
  out.height = p.h;
  ctx2d(out).drawImage(src, rect[0], rect[1], rect[2], rect[3], 0, 0, p.w, p.h);
  out.style.width = `${p.w}px`;
  out.style.height = `${p.h}px`;
  return { scale: p.scale, offset: -MARGIN * p.scale };
}
