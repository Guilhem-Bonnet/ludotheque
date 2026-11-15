// Accès aux données. Dans l'app Tauri, chaque appel passe par une commande Rust (`ludo-client`).
// Dans un navigateur (développement, démonstration), une source de démonstration répond à la place.

import type { Conflict, DeviceInfo, Game, Preparation, Profile, SaveRevision, UpdateItem } from './types';

export interface LudoApi {
  device(): Promise<DeviceInfo>;
  profiles(): Promise<Profile[]>;
  recent(profile: string): Promise<Game[]>;
  library(profile: string): Promise<Game[]>;
  history(profile: string, game: string): Promise<SaveRevision[]>;
  updates(): Promise<UpdateItem[]>;
  /** Synchronisation demandée à la main : envoie et récupère tout, puis renvoie la bibliothèque à jour. */
  sync(profile: string): Promise<Game[]>;
  /** Restaure une version de sauvegarde : elle devient la version actuelle (nouvelle révision). `onstep` suit l'avancement. */
  restore(profile: string, game: string, rev: number, onstep?: (step: number, progress: number) => void): Promise<void>;
  /** Temps de réponse du serveur en ms (test de connexion des Réglages) ; erreur si injoignable. */
  ping(): Promise<number>;
  /** Enregistre le profil (image, nom, préférences) sur le serveur. */
  saveProfile(profile: Profile): Promise<Profile>;
  /** Prépare le lancement : renvoie un conflit à trancher, ou la préparation en cours. */
  prepare(profile: string, game: string): Promise<{ conflict: Conflict } | { preparation: Preparation }>;
}

const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}



// ——— Source de démonstration (données illustratives, mêmes que les maquettes) ———

const PROFILES: Profile[] = [
  { id: 'guilhem', name: 'Guilhem', avatarKind: 'renard', accent: '#38b6ff', accentSoft: '#123049', accentInk: '#9ad8ff', status: 'Dernière partie : Mario Kart 8 Deluxe · Hier' },
  { id: 'claire', name: 'Claire', avatarKind: 'champi', accent: '#34e0a1', accentSoft: '#12352c', accentInk: '#8ff0c8', status: 'En cours sur Steam Deck', busyElsewhere: true },
];

const GAMES: Game[] = [
  { id: 'switch-lm3', title: "Luigi's Mansion 3", short: "Luigi's\nMansion 3", console: 'switch', tile: '#0a5a78', tileInk: '#ffffff', art: { motif: 'manoir', from: '#22e08f', to: '#0a5a78' }, emulator: 'Ryujinx 1.3.3', install: 'installe', sync: 'a_jour', playtime: '12 h 40', last: { device: 'Steam Deck', deviceKind: 'deck', when: "Aujourd'hui 20:15" }, version: '1.4.0 · 2 DLC', size: '6,8 Go' },
  { id: 'switch-mk8', title: 'Mario Kart 8 Deluxe', short: 'Mario Kart 8 Deluxe', console: 'switch', tile: '#ff2e63', tileInk: '#ffffff', art: { motif: 'course', from: '#ff9f1c', to: '#ff2e63' }, emulator: 'Ryujinx 1.3.3', install: 'installe', sync: 'envoi_en_attente', playtime: '31 h 05', last: { device: 'Steam Deck', deviceKind: 'deck', when: 'Hier 22:40' }, version: '3.0.1', size: '13,2 Go' },
  { id: 'n3ds-mm3d', title: "The Legend of Zelda: Majora's Mask 3D", short: "Majora's\nMask 3D", console: '3ds', tile: '#5b2bff', tileInk: '#ffffff', art: { motif: 'lune', from: '#ff5fa8', to: '#5b2bff', ink: '#34138f' }, emulator: 'Azahar', install: 'installe', sync: 'a_jour', playtime: '9 h 12', last: { device: 'Steam Deck', deviceKind: 'deck', when: 'Sam. 8 nov.' }, size: '1,1 Go' },
  { id: 'switch2-pokopia', title: 'Pokopia', short: 'Pokopia', console: 'switch2', tile: '#9ae6ff', tileInk: '#0b3a26', art: { motif: 'prairie', from: '#2fb8ff', to: '#9ae6ff' }, emulator: 'Lichii build-177', install: 'mise_a_jour', sync: 'a_jour', playtime: '4 h 30', size: '9,7 Go' },
  { id: 'nds-diamant', title: 'Pokémon Diamant', short: 'Pokémon\nDiamant', console: 'ds', tile: '#3d4dff', tileInk: '#ffffff', art: { motif: 'cristal', from: '#5ee0ff', to: '#3d4dff' }, emulator: 'melonDS', install: 'non_installe', sync: 'aucune', size: '65,8 Mo' },
  { id: 'n3ds-nsmb2', title: 'New Super Mario Bros. 2', short: 'New Super\nMario Bros. 2', console: '3ds', tile: '#ff5a3c', tileInk: '#ffffff', art: { motif: 'eclair', from: '#ffd23f', to: '#ff5a3c' }, emulator: 'Azahar', install: 'installe', sync: 'a_jour', playtime: '6 h 02' },
  { id: 'wiiu-wwhd', title: 'The Legend of Zelda: The Wind Waker HD', short: 'Wind\nWaker HD', console: 'wiiu', tile: '#1257d6', tileInk: '#ffffff', art: { motif: 'vagues', from: '#3fd0ff', to: '#1257d6' }, emulator: 'Cemu', install: 'non_installe', sync: 'aucune' },
  { id: 'gc-tp', title: 'The Legend of Zelda: Twilight Princess', short: 'Twilight\nPrincess', console: 'gc', tile: '#3b2a8a', tileInk: '#ffffff', art: { motif: 'lune', from: '#ff9a3c', to: '#3b2a8a', ink: '#1e1450' }, emulator: 'Dolphin', install: 'non_installe', sync: 'aucune' },
  { id: 'switch-botw', title: 'The Legend of Zelda: Breath of the Wild', short: 'Breath of\nthe Wild', console: 'switch', tile: '#1a8f8a', tileInk: '#ffffff', art: { motif: 'montagnes', from: '#7fe3c4', to: '#1a8f8a', ink: '#0b4a44' }, emulator: 'Ryujinx 1.3.3', install: 'installe', sync: 'conflit', playtime: '41 h 12' },
  { id: 'switch-epee', title: 'Pokémon Épée', short: 'Pokémon\nÉpée', console: 'switch', tile: '#3a2bd6', tileInk: '#ffffff', art: { motif: 'cristal', from: '#a98bff', to: '#3a2bd6' }, emulator: 'Ryujinx 1.3.3', install: 'installe', sync: 'a_jour' },
  { id: 'switch-letsgo', title: "Pokémon : Let's Go, Pikachu", short: "Let's Go\nPikachu", console: 'switch', tile: '#ff9f1c', tileInk: '#3a2400', art: { motif: 'eclair', from: '#ffe14d', to: '#ff9f1c' }, emulator: 'Ryujinx 1.3.3', install: 'non_installe', sync: 'aucune' },
  { id: 'switch-xc3', title: 'Xenoblade Chronicles 3', short: 'Xenoblade\nChronicles 3', console: 'switch', tile: '#3b3fd6', tileInk: '#ffffff', art: { motif: 'montagnes', from: '#5ad1ff', to: '#3b3fd6', ink: '#1b1f6e' }, emulator: 'Ryujinx 1.3.3', install: 'installe', sync: 'a_jour' },
];

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

const demoApi: LudoApi = {
  async device() {
    const params = new URLSearchParams(location.search);
    const shared = params.get('appareil') === 'pc';
    return shared
      ? { name: 'PC', kind: 'tv', server: 'connecte', freeSpace: '1,2 To libres' }
      : { name: 'Steam Deck', kind: 'deck', defaultProfile: 'claire', server: params.get('serveur') === 'hors_ligne' ? 'hors_ligne' : 'connecte', freeSpace: '214 Go libres' };
  },
  async profiles() { return PROFILES; },
  async recent() { return GAMES.slice(0, 5); },
  async library() { return GAMES; },
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
  async ping() {
    await wait(700);
    return 12;
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
 * Dans Tauri : une commande Rust absente retombe sur la démonstration, le temps que toutes les
 * commandes soient branchées. Une commande présente qui échoue :
 * - lecture (`fallback: true`) : retombe aussi sur la démonstration (serveur éteint, catalogue absent) ;
 * - écriture (restauration, profil) : l'erreur remonte, pour ne jamais afficher un faux succès.
 */
function viaTauri<K extends keyof LudoApi>(cmd: K, args: (...a: Parameters<LudoApi[K]>) => Record<string, unknown>, fallback = true): LudoApi[K] {
  return (async (...a: Parameters<LudoApi[K]>) => {
    try {
      return await invoke(cmd, args(...a));
    } catch (e) {
      const missing = /not found|unknown command|not allowed/i.test(String(e));
      if (!missing && !fallback) throw new Error(String(e));
      return (demoApi[cmd] as (...x: unknown[]) => unknown)(...a);
    }
  }) as LudoApi[K];
}

const tauriApi: LudoApi = {
  device: viaTauri('device', () => ({})),
  profiles: viaTauri('profiles', () => ({})),
  recent: viaTauri('recent', (profile) => ({ profile })),
  library: viaTauri('library', (profile) => ({ profile })),
  history: viaTauri('history', (profile, game) => ({ profile, game })),
  updates: viaTauri('updates', () => ({})),
  sync: viaTauri('sync', (profile) => ({ profile })),
  saveProfile: viaTauri('saveProfile', (profile) => ({ profile }), false),
  ping: viaTauri('ping', () => ({})),
  restore: async (profile, game, rev, onstep) => {
    try {
      await invoke('restore', { profile, game, rev });
      onstep?.(2, 1);
    } catch (e) {
      if (!/not found|unknown command|not allowed/i.test(String(e))) throw new Error(String(e));
      await demoApi.restore(profile, game, rev, onstep);
    }
  },
  prepare: viaTauri('prepare', (profile, game) => ({ profile, game })),
};

export const api: LudoApi = inTauri ? tauriApi : demoApi;
export const isDemo = !inTauri;
