// Outils d'animation partagés.

export const reducedMotion = () => window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;

/** Rejoue une classe d'animation CSS sur un élément (retire, force un reflow, remet). */
export function replay(el: Element | null | undefined, cls: string, ms = 400) {
  if (!el) return;
  el.classList.remove(cls);
  void (el as HTMLElement).offsetWidth;
  el.classList.add(cls);
  setTimeout(() => el.classList.remove(cls), ms);
}

type VT = { startViewTransition?: (cb: () => Promise<void> | void) => { finished: Promise<void> } };

/** Change d'écran dans une View Transition si le moteur la connaît (WebKitGTK récent, Chromium). */
export function withTransition(dir: 'forward' | 'back', update: () => Promise<void> | void) {
  const doc = document as unknown as VT;
  document.documentElement.dataset.dir = dir;
  if (!doc.startViewTransition || reducedMotion()) {
    void update();
    return;
  }
  doc.startViewTransition(update);
}
