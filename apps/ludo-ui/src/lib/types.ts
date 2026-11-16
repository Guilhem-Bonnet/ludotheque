// Modèle de données de l'interface. Côté Tauri, ces objets viennent du client Rust `ludo-client`.

export type Console = 'switch' | 'switch2' | '3ds' | 'ds' | 'wiiu' | 'gc';

/** Les cinq états de synchronisation (conception 10.4), plus l'absence de sauvegarde. */
export type SyncState = 'a_jour' | 'synchronisation' | 'envoi_en_attente' | 'conflit' | 'hors_ligne' | 'aucune'
  /** Sauvegarde sur le serveur jamais récupérée sur cet appareil (téléchargée d'elle-même au lancement). */
  | 'a_recuperer';

/** Motif d'une jaquette illustrée (dessins originaux, cf. CoverArt.svelte). */
export type Motif = 'manoir' | 'course' | 'lune' | 'prairie' | 'cristal' | 'vagues' | 'montagnes' | 'eclair';

/** Jaquette illustrée : dégradé vif `from` → `to`, motif, couleur des silhouettes. */
export interface Art { motif: Motif; from: string; to: string; ink?: string }

export type AvatarKind = 'renard' | 'champi' | 'chat' | 'etoile' | 'planete' | 'poisson';

export type DeviceKind = 'pc' | 'deck' | 'thor' | 'tv';

export interface Profile {
  id: string;
  name: string;
  /** URL de l'avatar choisi ; absent = initiale sur la couleur du profil. */
  avatar?: string;
  /** Personnage illustré choisi (en attendant les images personnelles). */
  avatarKind?: AvatarKind;
  accent: string;
  accentSoft: string;
  accentInk: string;
  /** Texte secondaire sous l'avatar (« Dernière partie : … », « En cours sur Steam Deck »). */
  status?: string;
  busyElsewhere?: boolean;
  /** Préférences du profil (suivent la personne sur tous ses appareils). */
  prefs?: { sons: boolean; vibrations: boolean };
}

export interface LastSession {
  device: string;
  deviceKind: DeviceKind;
  /** Date lisible (« Aujourd'hui 20:15 »). */
  when: string;
}

export type InstallState = 'installe' | 'non_installe' | 'telechargement' | 'mise_a_jour';

/** Mise à jour ou DLC d'un jeu (vue « Contenu additionnel »). */
export interface GameContent {
  id: number;
  role: 'maj' | 'dlc';
  name: string;
  size: string;
  installed: boolean;
}

export interface Game {
  id: string;
  title: string;
  /** Titre court pour la tuile (retours à la ligne autorisés avec \n). */
  short: string;
  console: Console;
  tile: string;
  tileInk: string;
  art?: Art;
  /** URL absolue de l'image du jeu (absente tant que le serveur n'en a pas). */
  cover?: string;
  emulator: string;
  install: InstallState;
  progress?: number;
  sync: SyncState;
  playtime?: string;
  last?: LastSession;
  /** Échec de la dernière synchronisation manuelle de ce jeu (« En cours sur Steam Deck »…). */
  syncError?: string;
  content?: GameContent[];
  version?: string;
  size?: string;
}

export interface SaveRevision {
  rev: number;
  device: string;
  deviceKind: DeviceKind;
  when: string;
  playtime: string;
  current?: boolean;
  discarded?: boolean;
}

export interface ConflictSide {
  device: string;
  deviceKind: DeviceKind;
  when: string;
  playtime: string;
  delta: string;
  /** Vrai seulement si mesurable (temps de jeu supérieur). */
  ahead?: boolean;
}

export interface Conflict {
  game: Game;
  profile: string;
  local: ConflictSide;
  server: ConflictSide;
  /** Conflit apparu à l'envoi après une partie jouée : le jeu ne se relance pas après le choix. */
  played?: boolean;
  /** Révision du serveur affichée : le choix ne s'applique que si la tête n'a pas changé depuis. */
  rev?: number;
}

export interface UpdateItem {
  id: string;
  group: 'jeux' | 'nouveaux' | 'emulateurs';
  title: string;
  detail: string;
  console?: Console;
  tile?: string;
  action: 'Mettre à jour' | 'Installer';
}

export interface DeviceInfo {
  name: string;
  kind: DeviceKind;
  /** Profil mémorisé (appareil personnel) ; absent = appareil partagé → « Qui joue ? ». */
  defaultProfile?: string;
  server: 'connecte' | 'hors_ligne';
  freeSpace: string;
  /** Adresse du serveur (Réglages ; absente dans la démo). */
  serverUrl?: string;
  /** Version de l'app (absente dans la démo). */
  version?: string;
}

export interface Preparation {
  game: Game;
  profile: string;
  steps: { label: string; state: 'fait' | 'en_cours' | 'a_venir'; detail?: string; progress?: number }[];
}

/** Émulateur connu (fiche) et ce que la détection a trouvé sur l'appareil (`UiEmulator` côté Rust). */
export interface Emulator {
  id: string;
  name: string;
  consoles: Console[];
  /** Consoles dont c'est l'émulateur par défaut sur cet appareil. */
  defaultFor: Console[];
  found: boolean;
  /** « Flatpak io.github… », « AppImage … », « Réglage de l'appareil »… ; vide si absent. */
  source: string;
  bin: string;
  data: string;
}
