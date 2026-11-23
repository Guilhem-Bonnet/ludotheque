// Ayn Thor, deux écrans : l'app complète tourne sur l'écran du BAS (tactile) et pousse vers l'écran du
// HAUT (EcranHaut.svelte, seconde WebView créée par DoubleEcran.kt) l'état du jeu choisi. Tout part
// d'événements : focus d'un élément (`focusin`), changement d'écran ou de profil (runes), retour au
// premier plan (`visibilitychange`), activation du second écran (`ludo-ecran`, posé par Kotlin).
// Hors Android (PC, Deck, démo) : rien n'est installé.
import { api, isAndroid, isDemo } from './api';
import { router } from './router.svelte';
import { coverBase } from './avatars';
import type { Game, Profile } from './types';

/** Jeu tel que l'écran du haut l'affiche (sous-ensemble de `Game`). */
export type JeuHaut = Pick<Game, 'id' | 'title' | 'console' | 'tile' | 'tileInk' | 'art' | 'cover' | 'background' | 'emulator' | 'install' | 'sync' | 'playtime' | 'last' | 'stream'>;

export interface EtatHaut {
  /** Écran affiché en bas (`accueil`, `fiche`, `passage`…). */
  ecran: string;
  profil?: Pick<Profile, 'name' | 'accent' | 'accentSoft' | 'accentInk' | 'avatar' | 'avatarKind'>;
  jeu?: JeuHaut;
  /** Partie en cours de lancement (écran de passage). */
  lancement?: boolean;
  /** Adresse du serveur des images de profil (la WebView du haut n'appelle pas `api.device()`). */
  images?: string;
}

let library: Game[] = [];
let recent: Game[] = [];
let loadedFor = '';
let focusedId: string | null = null;
let last = '';

function jeuHaut(g: Game): JeuHaut {
  const { id, title, console, tile, tileInk, art, cover, background, emulator, install, sync, playtime, last, stream } = g;
  return { id, title, console, tile, tileInk, art, cover, background, emulator, install, sync, playtime, last, stream };
}

/** Jeu désigné par l'élément focalisé : `data-game` (tuiles, bibliothèque), sinon rang dans la rangée
 *  de l'accueil (`data-slot`, même ordre que `api.recent`). */
function gameOf(target: EventTarget | null): string | null {
  if (!(target instanceof Element)) return null;
  const tagged = target.closest<HTMLElement>('[data-game]');
  if (tagged?.dataset.game) return tagged.dataset.game;
  const slot = target.closest<HTMLElement>('[data-slot]');
  if (!slot) return null;
  const i = [...document.querySelectorAll('[data-slot]')].indexOf(slot);
  return recent[i]?.id ?? null;
}

function current(): { jeu?: Game; lancement?: boolean } {
  const s = router.screen;
  if (s.name === 'fiche') return { jeu: s.game };
  if (s.name === 'passage') return { jeu: s.preparation.game, lancement: true };
  if (s.name === 'conflit') return { jeu: s.conflict.game };
  if (!focusedId) return {};
  return { jeu: library.find((g) => g.id === focusedId) ?? recent.find((g) => g.id === focusedId) };
}

function etat(): EtatHaut {
  const p = router.profile;
  const { jeu, lancement } = current();
  return {
    ecran: router.screen.name,
    profil: p ? { name: p.name, accent: p.accent, accentSoft: p.accentSoft, accentInk: p.accentInk, avatar: p.avatar, avatarKind: p.avatarKind } : undefined,
    jeu: jeu ? jeuHaut(jeu) : undefined,
    lancement,
    images: coverBase() || undefined,
  };
}

/** Envois en file : chaque état part après le précédent (l'écran du haut n'affiche jamais un état
 *  plus ancien que le dernier calculé). */
let queue: Promise<void> = Promise.resolve();
function publish(force = false): Promise<void> {
  const e = etat();
  const json = JSON.stringify(e);
  if (!force && json === last) return queue;
  last = json;
  queue = queue.then(async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke<boolean>('ecranHaut', { etat: e });
    } catch {
      // Pont absent (ancienne coque) : l'écran du haut garde son dernier état ; renvoi au prochain événement.
      if (last === json) last = '';
    }
  });
  return queue;
}

/** Bibliothèque et récents du profil (temps de jeu, appareil de la dernière partie, état des sauvegardes). */
async function load(profile: string) {
  try {
    const [r, l] = await Promise.all([api.recent(profile), api.library(profile)]);
    if (router.profile?.id !== profile) return;
    recent = r;
    library = l;
    loadedFor = profile;
    // Focus posé avant l'arrivée des listes (tuile mise en avant au démarrage) : relu maintenant.
    focusedId = gameOf(document.activeElement) ?? focusedId;
    void publish();
  } catch {
    // Hors ligne : l'écran du haut montre ce qu'il sait (profil, jeu de la fiche).
  }
}

let started = false;
export function startEcranHaut(): void {
  if (started || !isAndroid || isDemo) return;
  started = true;
  document.addEventListener('focusin', (e) => {
    const id = gameOf(e.target);
    // Un bouton hors rangée (raccourci, profil) garde le dernier jeu à l'écran : le haut ne clignote pas.
    if (id) focusedId = id;
    void publish();
  });
  // Second écran (re)devenu actif : l'état est renvoyé tel quel.
  window.addEventListener('ludo-ecran', () => void publish(true));
  // Retour d'un émulateur ou de Moonlight : temps de jeu et sauvegardes ont pu changer.
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible' && router.profile) void load(router.profile.id);
  });
  $effect.root(() => {
    $effect(() => {
      const p = router.profile?.id;
      if (p && p !== loadedFor) {
        focusedId = null;
        void load(p);
      }
    });
    $effect(() => {
      // Dépendances lues : écran et profil courants.
      void router.screen;
      void router.profile;
      void publish();
    });
  });
}
