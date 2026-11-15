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
  /** Prépare le lancement : renvoie un conflit à trancher, ou la préparation en cours. */
  prepare(profile: string, game: string): Promise<{ conflict: Conflict } | { preparation: Preparation }>;
}

const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

const tauriApi: LudoApi = {
  device: () => invoke('device'),
  profiles: () => invoke('profiles'),
  recent: (profile) => invoke('recent', { profile }),
  library: (profile) => invoke('library', { profile }),
  history: (profile, game) => invoke('history', { profile, game }),
  updates: () => invoke('updates'),
  prepare: (profile, game) => invoke('prepare', { profile, game }),
};

// ——— Source de démonstration (données illustratives, mêmes que les maquettes) ———

const PROFILES: Profile[] = [
  { id: 'guilhem', name: 'Guilhem', accent: '#4cc3ff', accentSoft: '#1c3546', accentInk: '#8ad8ff', status: 'Dernière partie : Mario Kart 8 Deluxe · Hier' },
  { id: 'claire', name: 'Claire', accent: '#ff8a5c', accentSoft: '#3a2a26', accentInk: '#ffb08f', status: 'En cours sur Steam Deck', busyElsewhere: true },
];

const GAMES: Game[] = [
  { id: 'switch-lm3', title: "Luigi's Mansion 3", short: "Luigi's\nMansion 3", console: 'switch', tile: '#1e3b33', tileInk: '#cff5e4', emulator: 'Ryujinx 1.3.3', install: 'installe', sync: 'a_jour', playtime: '12 h 40', last: { device: 'Steam Deck', deviceKind: 'deck', when: "Aujourd'hui 20:15" }, version: '1.4.0 · 2 DLC', size: '6,8 Go' },
  { id: 'switch-mk8', title: 'Mario Kart 8 Deluxe', short: 'Mario Kart 8 Deluxe', console: 'switch', tile: '#2b2346', tileInk: '#e3dcff', emulator: 'Ryujinx 1.3.3', install: 'installe', sync: 'envoi_en_attente', playtime: '31 h 05', last: { device: 'Steam Deck', deviceKind: 'deck', when: 'Hier 22:40' }, version: '3.0.1', size: '13,2 Go' },
  { id: 'n3ds-mm3d', title: "The Legend of Zelda: Majora's Mask 3D", short: "Majora's\nMask 3D", console: '3ds', tile: '#3a2e1b', tileInk: '#f6e3bf', emulator: 'Azahar', install: 'installe', sync: 'a_jour', playtime: '9 h 12', last: { device: 'Steam Deck', deviceKind: 'deck', when: 'Sam. 8 nov.' }, size: '1,1 Go' },
  { id: 'switch2-pokopia', title: 'Pokopia', short: 'Pokopia', console: 'switch2', tile: '#23344a', tileInk: '#d4e7ff', emulator: 'Lichii build-177', install: 'mise_a_jour', sync: 'a_jour', playtime: '4 h 30', size: '9,7 Go' },
  { id: 'nds-diamant', title: 'Pokémon Diamant', short: 'Pokémon\nDiamant', console: 'ds', tile: '#22262f', tileInk: '#c9cdd6', emulator: 'melonDS', install: 'non_installe', sync: 'aucune', size: '65,8 Mo' },
  { id: 'n3ds-nsmb2', title: 'New Super Mario Bros. 2', short: 'New Super\nMario Bros. 2', console: '3ds', tile: '#3a2422', tileInk: '#ffd6cf', emulator: 'Azahar', install: 'installe', sync: 'a_jour', playtime: '6 h 02' },
  { id: 'wiiu-wwhd', title: 'The Legend of Zelda: The Wind Waker HD', short: 'Wind\nWaker HD', console: 'wiiu', tile: '#173847', tileInk: '#c8ecfa', emulator: 'Cemu', install: 'non_installe', sync: 'aucune' },
  { id: 'gc-tp', title: 'The Legend of Zelda: Twilight Princess', short: 'Twilight\nPrincess', console: 'gc', tile: '#2a2540', tileInk: '#dcd6ff', emulator: 'Dolphin', install: 'non_installe', sync: 'aucune' },
  { id: 'switch-botw', title: 'The Legend of Zelda: Breath of the Wild', short: 'Breath of\nthe Wild', console: 'switch', tile: '#24331f', tileInk: '#ddf0cf', emulator: 'Ryujinx 1.3.3', install: 'installe', sync: 'conflit', playtime: '41 h 12' },
  { id: 'switch-epee', title: 'Pokémon Épée', short: 'Pokémon\nÉpée', console: 'switch', tile: '#1f2a44', tileInk: '#d6e1ff', emulator: 'Ryujinx 1.3.3', install: 'installe', sync: 'a_jour' },
  { id: 'switch-letsgo', title: "Pokémon : Let's Go, Pikachu", short: "Let's Go\nPikachu", console: 'switch', tile: '#3a3418', tileInk: '#fff1b8', emulator: 'Ryujinx 1.3.3', install: 'non_installe', sync: 'aucune' },
  { id: 'switch-xc3', title: 'Xenoblade Chronicles 3', short: 'Xenoblade\nChronicles 3', console: 'switch', tile: '#1b2b33', tileInk: '#cde6f0', emulator: 'Ryujinx 1.3.3', install: 'installe', sync: 'a_jour' },
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
      { id: 'u1', group: 'jeux', title: 'Pokopia', detail: 'Mise à jour 1.1.0 · 412 Mo', console: 'switch2', tile: '#23344a', action: 'Mettre à jour' },
      { id: 'u2', group: 'jeux', title: 'Mario Kart 8 Deluxe', detail: 'DLC disponible : Pass circuits additionnels · 640 Mo', console: 'switch', tile: '#2b2346', action: 'Installer' },
      { id: 'u3', group: 'nouveaux', title: 'The Legend of Zelda: Twilight Princess', detail: 'Ajouté le 14/11 · 889 Mo', console: 'gc', tile: '#2a2540', action: 'Installer' },
      { id: 'u4', group: 'emulateurs', title: 'Ryujinx', detail: '1.3.3 → 1.3.4 · version de référence du foyer', action: 'Mettre à jour' },
    ];
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

export const api: LudoApi = inTauri ? tauriApi : demoApi;
export const isDemo = !inTauri;
