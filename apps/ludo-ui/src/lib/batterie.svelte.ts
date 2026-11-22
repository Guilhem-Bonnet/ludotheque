// Batterie de l'appareil pour l'indicateur d'angle (Batterie.svelte) et le repos (Repos.svelte).
// Événements seulement, jamais de scrutation :
// - Android (Ayn Thor) : `navigator.getBattery()` de la WebView Chromium, événements `levelchange` et
//   `chargingchange` (essai réel sur la Thor le 22/11 : niveau 1, en charge = `dumpsys battery`).
// - Bureau (Steam Deck, portable) : commande `batterie_etat` au démarrage, puis événement Tauri
//   `ludo://batterie` (UPower, energie.rs) ; relecture au retour du focus si UPower manque.
// - PC fixe : aucune batterie → `connue` reste faux, rien ne s'affiche.
// - Démonstration (navigateur) : `?batterie=<niveau>` ou `?batterie=<niveau>c` (en charge), sinon rien.
import { isAndroid, isDemo } from './api';

export const SEUIL_FAIBLE = 15;

export const batterie = $state({ connue: false, niveau: 100, enCharge: false });

type Etat = { niveau: number; enCharge: boolean } | null;

function poser(e: Etat) {
  if (!e) {
    batterie.connue = false;
    return;
  }
  batterie.niveau = Math.max(0, Math.min(100, Math.round(e.niveau)));
  batterie.enCharge = e.enCharge;
  batterie.connue = true;
}

interface BatteryManagerLike extends EventTarget {
  level: number;
  charging: boolean;
}

let suivie = false;

/** Démarre le suivi (une seule fois, quel que soit le nombre d'indicateurs affichés). */
export function suivreBatterie() {
  if (suivie || typeof window === 'undefined') return;
  suivie = true;
  if (isDemo) {
    const q = new URLSearchParams(location.search).get('batterie');
    const m = q ? /^(\d{1,3})(c?)$/.exec(q) : null;
    if (m) poser({ niveau: Number(m[1]), enCharge: m[2] === 'c' });
    return;
  }
  const nav = navigator as Navigator & { getBattery?: () => Promise<BatteryManagerLike> };
  if (isAndroid) {
    if (typeof nav.getBattery !== 'function') return;
    nav
      .getBattery()
      .then((b) => {
        const maj = () => poser({ niveau: b.level * 100, enCharge: b.charging });
        maj();
        b.addEventListener('levelchange', maj);
        b.addEventListener('chargingchange', maj);
      })
      .catch(() => {});
    return;
  }
  void (async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const { listen } = await import('@tauri-apps/api/event');
      const lire = () => invoke<Etat>('batterie_etat').then(poser).catch(() => {});
      await lire();
      if (!batterie.connue) return; // PC fixe : rien à suivre
      await listen<Etat>('ludo://batterie', ({ payload }) => poser(payload));
      window.addEventListener('focus', () => void lire());
    } catch {
      /* commande absente (ancienne coque) : pas d'indicateur */
    }
  })();
}
