// Accès aux données. Dans l'app Tauri, chaque appel passe par une commande Rust (`ludo-client`).
// Dans un navigateur (développement, démonstration), une source de démonstration répond à la place.

import { setCoverBase } from './avatars';
import type { Conflict, ConflictSide, DeviceInfo, Emulator, Game, LaunchMode, RemoteDevice, Preparation, Profile, SaveRevision, UpdateItem } from './types';

/** Étapes d'un lancement réel, dans l'ordre (événement `ludo://passage`). */
export type LaunchStep = 'profil' | 'telechargement' | 'sauvegarde' | 'lancement' | 'en_jeu' | 'envoi' | 'termine';

export interface LaunchEvent {
  step: LaunchStep;
  state: 'en_cours' | 'fait';
  detail?: string;
}

/** Fin d'un lancement : partie jouée et sauvegarde envoyée, ou conflit à trancher (rien n'a été lancé sauf si `played`). */
export type LaunchResult =
  | { result: 'termine'; playtime: string; detail?: string; emulatorError?: boolean }
  | { result: 'conflit'; local: ConflictSide; server: ConflictSide; played: boolean; rev: number }
  /** Android : jeu ouvert dans l'émulateur ou dans Moonlight (`detail` : nom de l'émulateur ou de l'appli Sunshine). */
  | { result: 'lance'; via: 'emulateur' | 'flux'; detail: string };

/** Suite d'un choix de conflit : appliqué, ou conflit à retrancher (le serveur a changé depuis l'affichage). */
export type ResolveResult = { result: 'fait' } | { result: 'conflit'; local: ConflictSide; server: ConflictSide; played: boolean; rev: number };

export interface LudoApi {
  device(): Promise<DeviceInfo>;
  profiles(): Promise<Profile[]>;
  recent(profile: string): Promise<Game[]>;
  library(profile: string): Promise<Game[]>;
  history(profile: string, game: string): Promise<SaveRevision[]>;
  /** Mises à jour et DLC à installer ; `profile` = profil en cours (appareil partagé). */
  updates(profile?: string): Promise<UpdateItem[]>;
  /** Synchronisation demandée à la main : envoie et récupère tout, puis renvoie la bibliothèque à jour. */
  sync(profile: string): Promise<Game[]>;
  /** Restaure une version de sauvegarde : elle devient la version actuelle (nouvelle révision). `onstep` suit l'avancement. */
  restore(profile: string, game: string, rev: number, onstep?: (step: number, progress: number) => void): Promise<void>;
  /** Temps de réponse du serveur en ms (test de connexion des Réglages) ; erreur si injoignable. */
  ping(): Promise<number>;
  /** Recherche les émulateurs sur l'appareil (à chaque appel, sans cache). */
  emulators(): Promise<Emulator[]>;
  /** Autres appareils du foyer et leurs jeux (inventaires du serveur). */
  devices(): Promise<RemoteDevice[]>;
  /** Installe une mise à jour ou un DLC sur l'appareil ; `onprogress` reçoit 0 à 1 (et `preparing` tant que le serveur convertit le fichier). */
  installContent(game: string, content: number, onprogress?: OnProgress): Promise<void>;
  /** Installe un jeu sur l'appareil (jeu de base et mises à jour) ; `onprogress` reçoit 0 à 1. Erreur : message en français. */
  installGame(game: string, onprogress?: OnProgress): Promise<void>;
  /** Annule l'installation en cours d'un jeu (l'appel à `installGame` échoue alors avec « Installation annulée »). */
  cancelInstall(game: string): Promise<void>;
  /** Désinstalle un jeu de l'appareil : fichiers du jeu seulement, jamais les sauvegardes. */
  uninstallGame(game: string): Promise<void>;
  /** Enregistre le profil (image, nom, préférences) sur le serveur. */
  saveProfile(profile: Profile): Promise<Profile>;
  /**
   * Lance le jeu pour de vrai : verrou de partie, installation si besoin, sauvegarde, émulateur, envoi à la fermeture.
   * `onstep` suit les étapes, `onprogress` (0 à 1) le téléchargement. Erreur : message en français.
   */
  launchGame(profile: string, game: string, onstep?: (e: LaunchEvent) => void, onprogress?: OnProgress, mode?: LaunchMode): Promise<LaunchResult>;
  /** Tranche un conflit : « local » garde la sauvegarde de cet appareil, « serveur » prend celle du serveur ; l'autre reste dans l'historique. */
  resolveConflict(profile: string, game: string, choice: 'local' | 'serveur', rev: number): Promise<ResolveResult>;
  /** Renvoie la sauvegarde d'une partie déjà jouée dont l'envoi a échoué, sans relancer le jeu. */
  retrySend(profile: string, game: string): Promise<LaunchResult>;
  /** Appelé quand la fermeture de la fenêtre est refusée parce qu'une partie est en cours ; renvoie la fonction d'arrêt de l'écoute. */
  onCloseRefused(cb: () => void): Promise<() => void>;
  /** Prépare le lancement : renvoie un conflit à trancher, ou la préparation en cours. */
  prepare(profile: string, game: string): Promise<{ conflict: Conflict } | { preparation: Preparation }>;
  /** Retient le profil choisi sur cet appareil (ouvert directement au prochain démarrage). */
  rememberProfile(profile: string): Promise<void>;
  /** Lancement au démarrage de la session (bureau seulement) : état actuel, puis changement. */
  autostart(): Promise<boolean>;
  setAutostart(on: boolean): Promise<boolean>;
}

/** Le serveur prépare le fichier (conversion NSZ → NSP pour Eden) : « Préparation sur le serveur… ». */
export interface Preparing {
  /** Avancement annoncé par le convertisseur (0 à 100), s'il en donne un. */
  percent?: number;
  /** En attente d'une autre conversion. */
  queued: boolean;
}

/** Libellé de l'étape de préparation sur le serveur. */
export function prepLabel(p: Preparing): string {
  if (p.queued) return "File d'attente sur le serveur…";
  return p.percent !== undefined ? `Préparation sur le serveur… ${p.percent} %` : 'Préparation sur le serveur…';
}

/** Rappel de progression : `p` de 0 à 1 ; `preparing` n'est présent que pendant la préparation sur le serveur. */
export type OnProgress = (p: number, preparing?: Preparing) => void;

const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** App Android (Ayn Thor) : lancement par intent, sauvegardes synchronisées par le PC. */
export const isAndroid = typeof navigator !== 'undefined' && /Android/i.test(navigator.userAgent);

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}


/** Événement de progression émis par l'app Rust pendant une installation (cumul de tous les fichiers). */
const EVENT_PROGRESSION = 'ludo://progression';

/** Exécute `run` en relayant la progression du jeu `game` à `onprogress` (0 à 1). */
async function withProgress<T>(game: string, onprogress: OnProgress | undefined, run: () => Promise<T>): Promise<T> {
  let unlisten: (() => void) | undefined;
  if (onprogress) {
    try {
      const { listen } = await import('@tauri-apps/api/event');
      unlisten = await listen<{ game: string; file: number; done: number; total: number; prepare?: { percent?: number; queued: boolean } }>(EVENT_PROGRESSION, (ev) => {
        const { game: g, done, total, prepare } = ev.payload;
        if (g !== game) return;
        const p = total > 0 ? Math.min(1, done / total) : 0;
        if (prepare) onprogress(p, { percent: prepare.percent, queued: prepare.queued });
        else if (total > 0) onprogress(p);
      });
    } catch {
      // Pas d'écoute possible : l'installation se fait quand même, sans progression.
    }
  }
  try {
    return await run();
  } finally {
    unlisten?.();
  }
}

const EVENT_PASSAGE = 'ludo://passage';

/** Exécute `run` en relayant les étapes du lancement du jeu `game` à `onstep`. */
async function withSteps<T>(game: string, onstep: ((e: LaunchEvent) => void) | undefined, run: () => Promise<T>): Promise<T> {
  let unlisten: (() => void) | undefined;
  if (onstep) {
    try {
      const { listen } = await import('@tauri-apps/api/event');
      unlisten = await listen<{ game: string; step: LaunchStep; state: 'en_cours' | 'fait'; detail?: string }>(EVENT_PASSAGE, (ev) => {
        const { game: g, step, state, detail } = ev.payload;
        if (g === game) onstep({ step, state, detail });
      });
    } catch {
      // Pas d'écoute possible : le lancement se fait quand même, sans étapes.
    }
  }
  try {
    return await run();
  } finally {
    unlisten?.();
  }
}

// ——— Source de démonstration (données illustratives, mêmes que les maquettes) ———

const PROFILES: Profile[] = [
  { id: 'guilhem', name: 'Guilhem', avatarKind: 'renard', accent: '#38b6ff', accentSoft: '#123049', accentInk: '#9ad8ff', status: 'Dernière partie : Mario Kart 8 Deluxe · Hier' },
  { id: 'claire', name: 'Claire', avatarKind: 'champi', accent: '#34e0a1', accentSoft: '#12352c', accentInk: '#8ff0c8', status: 'En cours sur Steam Deck', busyElsewhere: true },
];

const GAMES: Game[] = [
  { id: 'switch-lm3', title: "Luigi's Mansion 3", short: "Luigi's\nMansion 3", console: 'switch', tile: '#0a5a78', tileInk: '#ffffff', art: { motif: 'manoir', from: '#22e08f', to: '#0a5a78' }, emulator: 'Ryujinx 1.3.3', install: 'installe', sync: 'a_jour', playtime: '12 h 40', last: { device: 'Steam Deck', deviceKind: 'deck', when: "Aujourd'hui 20:15" }, version: '1.4.0 · 2 DLC', size: '6,8 Go', content: [{ id: 101, role: 'maj', name: 'Mise à jour', size: '412 Mo', installed: true }, { id: 102, role: 'dlc', name: 'Pack multijoueur 1', size: '1,2 Go', installed: true }, { id: 103, role: 'dlc', name: 'Pack multijoueur 2', size: '1,1 Go', installed: false }] },
  { id: 'switch-mk8', title: 'Mario Kart 8 Deluxe', short: 'Mario Kart 8 Deluxe', console: 'switch', tile: '#ff2e63', tileInk: '#ffffff', art: { motif: 'course', from: '#ff9f1c', to: '#ff2e63' }, emulator: 'Ryujinx 1.3.3', install: 'installe', sync: 'envoi_en_attente', playtime: '31 h 05', last: { device: 'Steam Deck', deviceKind: 'deck', when: 'Hier 22:40' }, version: '3.0.1', size: '13,2 Go', content: [{ id: 201, role: 'maj', name: 'Mise à jour', size: '640 Mo', installed: true }, { id: 202, role: 'dlc', name: 'Pass circuits additionnels', size: '2,4 Go', installed: false }] },
  { id: 'n3ds-mm3d', title: "The Legend of Zelda: Majora's Mask 3D", short: "Majora's\nMask 3D", console: '3ds', tile: '#5b2bff', tileInk: '#ffffff', art: { motif: 'lune', from: '#ff5fa8', to: '#5b2bff', ink: '#34138f' }, emulator: 'Azahar', install: 'installe', sync: 'a_jour', playtime: '9 h 12', last: { device: 'Steam Deck', deviceKind: 'deck', when: 'Sam. 8 nov.' }, size: '1,1 Go' },
  { id: 'switch2-pokopia', title: 'Pokopia', short: 'Pokopia', console: 'switch2', tile: '#9ae6ff', tileInk: '#0b3a26', art: { motif: 'prairie', from: '#2fb8ff', to: '#9ae6ff' }, emulator: 'Lichii build-177', install: 'mise_a_jour', sync: 'a_jour', playtime: '4 h 30', size: '9,7 Go' },
  { id: 'nds-diamant', title: 'Pokémon Diamant', short: 'Pokémon\nDiamant', console: 'ds', tile: '#3d4dff', tileInk: '#ffffff', art: { motif: 'cristal', from: '#5ee0ff', to: '#3d4dff' }, emulator: 'melonDS', install: 'non_installe', sync: 'aucune', size: '65,8 Mo' },
  { id: 'n3ds-nsmb2', title: 'New Super Mario Bros. 2', short: 'New Super\nMario Bros. 2', console: '3ds', tile: '#ff5a3c', tileInk: '#ffffff', art: { motif: 'eclair', from: '#ffd23f', to: '#ff5a3c' }, emulator: 'Azahar', install: 'installe', sync: 'a_jour', playtime: '6 h 02' },
  { id: 'wiiu-wwhd', title: 'The Legend of Zelda: The Wind Waker HD', short: 'Wind\nWaker HD', console: 'wiiu', tile: '#1257d6', tileInk: '#ffffff', art: { motif: 'vagues', from: '#3fd0ff', to: '#1257d6' }, emulator: 'Cemu', install: 'non_installe', sync: 'aucune' },
  { id: 'gc-tp', title: 'The Legend of Zelda: Twilight Princess', short: 'Twilight\nPrincess', console: 'gc', tile: '#3b2a8a', tileInk: '#ffffff', art: { motif: 'lune', from: '#ff9a3c', to: '#3b2a8a', ink: '#1e1450' }, emulator: 'Dolphin', install: 'non_installe', sync: 'aucune' },
  { id: 'switch-botw', title: 'The Legend of Zelda: Breath of the Wild', short: 'Breath of\nthe Wild', console: 'switch', tile: '#1a8f8a', tileInk: '#ffffff', art: { motif: 'montagnes', from: '#7fe3c4', to: '#1a8f8a', ink: '#0b4a44' }, emulator: 'Ryujinx 1.3.3', install: 'installe', sync: 'conflit', playtime: '41 h 12', content: [{ id: 301, role: 'maj', name: 'Mise à jour', size: '1,1 Go', installed: true }, { id: 302, role: 'dlc', name: "Pass d'extension", size: '1,9 Go', installed: true }] },
  { id: 'switch-epee', title: 'Pokémon Épée', short: 'Pokémon\nÉpée', console: 'switch', tile: '#3a2bd6', tileInk: '#ffffff', art: { motif: 'cristal', from: '#a98bff', to: '#3a2bd6' }, emulator: 'Ryujinx 1.3.3', install: 'installe', sync: 'a_recuperer', content: [{ id: 401, role: 'dlc', name: "L'Île solitaire de l'Armure", size: '1,4 Go', installed: false }, { id: 402, role: 'dlc', name: 'Les Terres Enneigées de la Couronne', size: '1,6 Go', installed: false }] },
  { id: 'switch-letsgo', title: "Pokémon : Let's Go, Pikachu", short: "Let's Go\nPikachu", console: 'switch', tile: '#ff9f1c', tileInk: '#3a2400', art: { motif: 'eclair', from: '#ffe14d', to: '#ff9f1c' }, emulator: 'Ryujinx 1.3.3', install: 'non_installe', sync: 'aucune' },
  { id: 'switch-xc3', title: 'Xenoblade Chronicles 3', short: 'Xenoblade\nChronicles 3', console: 'switch', tile: '#3b3fd6', tileInk: '#ffffff', art: { motif: 'montagnes', from: '#5ad1ff', to: '#3b3fd6', ink: '#1b1f6e' }, emulator: 'Ryujinx 1.3.3', install: 'installe', sync: 'a_jour' },
];

/** Message de l'erreur d'une installation annulée (identique côté Rust). */
export const CANCELLED = 'Installation annulée';
const demoCancelled = new Set<string>();

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Démo seulement : dernier profil retenu dans le navigateur (l'app le retient côté Rust). */
function demoLast(): string | undefined {
  try { return localStorage.getItem('ludo.demo.dernierProfil') ?? undefined; } catch { return undefined; }
}
let demoAutostart = false;

const demoApi: LudoApi = {
  async device() {
    const params = new URLSearchParams(location.search);
    const shared = params.get('appareil') === 'pc';
    // Démo : images des jeux servies par `?images=<adresse>` (portraits des profils), sinon disques unis.
    setCoverBase(params.get('images') ?? undefined);
    return shared
      ? { name: 'PC', kind: 'tv', server: 'connecte', freeSpace: '1,2 To libres', defaultProfile: demoLast() }
      : { name: 'Steam Deck', kind: 'deck', defaultProfile: 'claire', server: params.get('serveur') === 'hors_ligne' ? 'hors_ligne' : 'connecte', freeSpace: '214 Go libres' };
  },
  async profiles() { return PROFILES; },
  async recent() { return GAMES.slice(0, 5); },
  // `?jeux=300` : bibliothèque gonflée de copies (essai de performance de la bibliothèque).
  async library() {
    const n = Number(new URLSearchParams(location.search).get('jeux'));
    if (!(n > GAMES.length)) return GAMES;
    return Array.from({ length: n }, (_, i) => {
      const g = GAMES[i % GAMES.length];
      return i < GAMES.length ? g : { ...g, id: `${g.id}-${i}`, title: `${g.title} ${i}`, last: undefined };
    });
  },
  async history() {
    return [
      { rev: 9, device: 'Steam Deck', deviceKind: 'deck', when: "Aujourd'hui 20:15", playtime: '12 h 40', current: true },
      { rev: 8, device: 'Steam Deck', deviceKind: 'deck', when: 'Hier 21:58', playtime: '11 h 52' },
      { rev: 7, device: 'Steam Deck', deviceKind: 'deck', when: 'Mer. 12 nov. 19:30', playtime: '10 h 05', discarded: true },
      { rev: 6, device: 'Steam Deck', deviceKind: 'deck', when: 'Sam. 8 nov. 16:12', playtime: '7 h 48' },
      { rev: 5, device: 'Steam Deck', deviceKind: 'deck', when: 'Sam. 8 nov. 11:40', playtime: '6 h 02' },
    ];
  },
  async updates() {
    return [
      { id: 'u1', group: 'jeux', title: 'Pokopia', detail: 'Mise à jour 1.1.0 · 412 Mo', console: 'switch2', tile: 'linear-gradient(160deg, #2fb8ff, #45cf78)', action: 'Mettre à jour' },
      { id: 'u2', group: 'jeux', title: 'Mario Kart 8 Deluxe', detail: 'DLC disponible : Pass circuits additionnels · 640 Mo', console: 'switch', tile: 'linear-gradient(135deg, #ff9f1c, #ff2e63)', action: 'Installer' },
      { id: 'u3', group: 'nouveaux', title: 'The Legend of Zelda: Twilight Princess', detail: 'Ajouté le 14/11 · 889 Mo', console: 'gc', tile: 'linear-gradient(160deg, #ff9a3c, #3b2a8a)', action: 'Installer' },
      { id: 'u4', group: 'emulateurs', title: 'Ryujinx', detail: '1.3.3 → 1.3.4 · version de référence du foyer', action: 'Mettre à jour' },
    ];
  },
  async restore(_p, _g, _r, onstep) {
    // Démo : téléchargement de la version (≈1,2 s), puis remplacement local (≈0,6 s).
    for (let i = 1; i <= 12; i++) { await wait(100); onstep?.(0, i / 12); }
    // `?echec=restauration` : simule un conflit (la sauvegarde a changé ailleurs) pour voir l'écran d'échec.
    if (new URLSearchParams(location.search).get('echec') === 'restauration') throw new Error('conflit');
    onstep?.(1, 0);
    await wait(600);
    onstep?.(2, 1);
  },
  async devices() {
    await wait(300);
    return [] as RemoteDevice[];
  },
  async emulators() {
    await wait(700);
    return [
      { id: 'eden', name: 'Eden', consoles: ['switch'], defaultFor: [], found: true, source: 'Flatpak dev.eden_emu.eden', bin: 'flatpak run dev.eden_emu.eden', data: '~/.var/app/dev.eden_emu.eden/data/eden' },
      { id: 'ryujinx', name: 'Ryujinx', consoles: ['switch'], defaultFor: ['switch'], found: true, source: 'Flatpak io.github.ryubing.Ryujinx', bin: 'flatpak run io.github.ryubing.Ryujinx', data: '~/.var/app/io.github.ryubing.Ryujinx/config/Ryujinx' },
    ] satisfies Emulator[];
  },
  async ping() {
    await wait(700);
    return 12;
  },
  async installContent(_g, _c, onprogress) {
    for (let i = 1; i <= 20; i++) { await wait(90); onprogress?.(i / 20); }
  },
  async installGame(game, onprogress) {
    demoCancelled.delete(game);
    for (let i = 1; i <= 24; i++) {
      await wait(90);
      if (demoCancelled.delete(game)) throw new Error(CANCELLED);
      onprogress?.(i / 24);
    }
    const g = GAMES.find((x) => x.id === game);
    if (g) g.install = 'installe';
  },
  async cancelInstall(game) {
    demoCancelled.add(game);
  },
  async uninstallGame(game) {
    await wait(300);
    const g = GAMES.find((x) => x.id === game);
    if (g) g.install = 'non_installe';
  },
  async saveProfile(profile) {
    await wait(120);
    const i = PROFILES.findIndex((p) => p.id === profile.id);
    if (i >= 0) PROFILES[i] = { ...profile };
    return { ...profile };
  },
  async sync() {
    await wait(1800);
    // Démo : les envois en attente partent ; un conflit reste à trancher (il demande un choix).
    for (const g of GAMES) if (g.sync === 'envoi_en_attente' || g.sync === 'synchronisation') g.sync = 'a_jour';
    return GAMES.map((g) => ({ ...g }));
  },
  async launchGame(_profile, _game, onstep, onprogress) {
    const step = async (s: LaunchStep, ms: number, detail?: string) => {
      onstep?.({ step: s, state: 'en_cours' });
      await wait(ms);
      onstep?.({ step: s, state: 'fait', detail });
    };
    await step('profil', 200);
    onstep?.({ step: 'telechargement', state: 'en_cours' });
    for (let i = 1; i <= 10; i++) { await wait(60); onprogress?.(i / 10); }
    onstep?.({ step: 'telechargement', state: 'fait' });
    await step('sauvegarde', 500);
    await step('lancement', 600);
    await step('en_jeu', 1500);
    await step('envoi', 600);
    onstep?.({ step: 'termine', state: 'fait' });
    return { result: 'termine', playtime: '12 h 41' };
  },
  async resolveConflict() {
    await wait(150);
    return { result: 'fait' };
  },
  async retrySend() {
    await wait(400);
    return { result: 'termine', playtime: '12 h 41' };
  },
  async onCloseRefused() {
    return () => {};
  },
  async rememberProfile(profile) {
    try { localStorage.setItem('ludo.demo.dernierProfil', profile); } catch { /* stockage indisponible */ }
  },
  async autostart() { return demoAutostart; },
  async setAutostart(on) { demoAutostart = on; return on; },
  async prepare(profile, gameId) {
    await wait(150);
    const game = GAMES.find((g) => g.id === gameId) ?? GAMES[0];
    if (game.sync === 'conflit') {
      return {
        conflict: {
          game, profile,
          local: { device: 'Ayn Thor', deviceKind: 'thor', when: 'Hier, 23:10', playtime: '41 h 12', delta: '+1 h 05 depuis la base', ahead: true },
          server: { device: 'PC', deviceKind: 'pc', when: "Aujourd'hui, 14:02", playtime: '40 h 30', delta: '+0 h 23 depuis la base' },
        },
      };
    }
    const needsDownload = game.install === 'non_installe';
    return {
      preparation: {
        game, profile,
        steps: [
          { label: `Profil : ${PROFILES.find((p) => p.id === profile)?.name ?? profile}`, state: 'fait' },
          needsDownload
            ? { label: 'Téléchargement…', state: 'en_cours', detail: '42,1 Mo sur 65,8 Mo · 12,4 Mo/s', progress: 64 }
            : { label: 'Jeu installé', state: 'fait' },
          { label: 'Sauvegarde : récupération', state: needsDownload ? 'a_venir' : 'en_cours' },
          { label: 'Lancement', state: 'a_venir' },
        ],
      },
    };
  },
};

/**
 * App réelle (Tauri) : chaque appel passe par une commande Rust, et une erreur remonte telle quelle
 * (message en français) pour que l'écran l'affiche. Aucune donnée de démonstration dans l'app.
 */
/**
 * Commande absente de l'app Rust : messages exacts de Tauri 2 (« Command xyz not found », refus
 * d'autorisation), pour ne jamais confondre avec une erreur réseau comme « host not found ».
 */
const commandMissing = (e: unknown) => /^Command \S+ not found$| not allowed( on |\. (Plugin|Command) not found)/.test(String(e));

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw new Error(commandMissing(e) ? `Fonction indisponible dans cette version (${cmd})` : String(e));
  }
}

const tauriApi: LudoApi = {
  device: async () => {
    const d = await call<DeviceInfo>('device');
    setCoverBase(d.serverUrl);
    return d;
  },
  profiles: () => call('profiles'),
  recent: (profile) => call('recent', { profile }),
  library: (profile) => call('library', { profile }),
  history: (profile, game) => call('history', { profile, game }),
  updates: (profile) => call('updates', { profile: profile ?? null }),
  sync: (profile) => call('sync', { profile }),
  saveProfile: (profile) => call('saveProfile', { profile }),
  ping: () => call('ping'),
  emulators: () => call('emulators'),
  devices: () => call('devices'),
  installContent: async (game, content, onprogress) => {
    await withProgress(game, onprogress, () => call('installContent', { game, content }));
    onprogress?.(1);
  },
  installGame: async (game, onprogress) => {
    await withProgress(game, onprogress, () => call('installGame', { game }));
    onprogress?.(1);
  },
  cancelInstall: (game) => call('cancelInstall', { game }),
  uninstallGame: (game) => call('uninstallGame', { game }),
  restore: async (profile, game, rev, onstep) => {
    await call('restore', { profile, game, rev });
    onstep?.(2, 1);
  },
  launchGame: (profile, game, onstep, onprogress, mode) =>
    withSteps(game, onstep, () => withProgress(game, onprogress, () => call<LaunchResult>('launchGame', { profile, game, mode: mode ?? null }))),
  resolveConflict: (profile, game, choice, rev) => call('resolveConflict', { profile, game, choice, rev }),
  retrySend: (profile, game) => call<LaunchResult>('retrySend', { profile, game }),
  onCloseRefused: async (cb) => {
    try {
      const { listen } = await import('@tauri-apps/api/event');
      return await listen('ludo://fermeture-refusee', () => cb());
    } catch {
      return () => {};
    }
  },
  // Préparation réelle : construite par play.ts (realPreparation) puis menée par launchGame.
  prepare: () => Promise.reject(new Error('Fonction indisponible dans cette version (prepare)')),
  rememberProfile: (profile) => call('rememberProfile', { profile }),
  autostart: () => call('autostart'),
  setAutostart: (on) => call('setAutostart', { on }),
};

export const api: LudoApi = inTauri ? tauriApi : demoApi;
export const isDemo = !inTauri;
