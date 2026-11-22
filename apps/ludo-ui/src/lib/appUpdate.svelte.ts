// Mise à jour de l'app elle-même (pas des jeux) : releases signées publiées par la CI sur GitHub
// (`latest.json`), vérifiées avec la clé publique de tauri.conf.json avant toute installation.
// Vérification au démarrage et sur demande (Réglages) ; l'installation remplace l'AppImage puis relance.
// Android : même état, autre canal (voir checkAndroid / installAndroid).
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { isAndroid, isDemo } from './api';

/** `indisponible` (Android) : rien à installer depuis l'app, pour une raison expliquée dans `info`
 *  (aucune APK publiée faute de clé de signature, app d'essai mise à jour par le PC). */
export type AppUpdateStatus = 'idle' | 'checking' | 'none' | 'available' | 'downloading' | 'restarting' | 'error' | 'indisponible';

export const appUpdate = $state<{
  status: AppUpdateStatus;
  /** Version proposée (`0.5.0`). */
  version: string;
  notes: string;
  /** Avancement du téléchargement, 0-100 ; `null` si la taille n'est pas connue. */
  percent: number | null;
  error: string;
  /** Raison d'un état `indisponible`, en clair. */
  info: string;
  checkedAt: Date | null;
}>({ status: 'idle', version: '', notes: '', percent: null, error: '', info: '', checkedAt: null });

let pending: Update | null = null;

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

// Android : APK signé publié avec chaque release (`android.json`), comparé à la version installée puis
// confié à l'installeur d'Android (MiseAJourApk.kt). Progression reçue par l'événement `ludo-apk`.
interface ApkVersion { installee: string; version: string; notes: string; disponible: boolean; raison?: string }
async function androidInvoke<T>(cmd: string): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd);
}

/** Version refusée par Android pour signature (par appareil) : jamais reproposée tant que l'app
 *  installée n'a pas changé. Stockage facultatif : sans lui, l'explication revient à l'essai suivant. */
const REFUSED_KEY = 'ludo.apk.refusee';
function rememberRefused(version: string): void {
  try { localStorage.setItem(REFUSED_KEY, version); } catch { /* stockage indisponible */ }
}
function refusedVersion(): string | null {
  try { return localStorage.getItem(REFUSED_KEY); } catch { return null; }
}

async function checkAndroid(): Promise<void> {
  appUpdate.status = 'checking';
  appUpdate.error = '';
  try {
    const v = await androidInvoke<ApkVersion>('apkVerifier');
    appUpdate.checkedAt = new Date();
    appUpdate.version = v.version;
    appUpdate.notes = v.notes;
    if (v.disponible && v.version && v.version === refusedVersion()) {
      appUpdate.info = APK_ETAPES.signature;
      appUpdate.status = 'indisponible';
      return;
    }
    appUpdate.info = v.raison ?? '';
    appUpdate.status = v.disponible ? 'available' : v.raison ? 'indisponible' : 'none';
  } catch (e) {
    appUpdate.checkedAt = new Date();
    appUpdate.error = message(e);
    appUpdate.status = 'error';
  }
}

const APK_ETAPES: Record<string, string> = {
  annulee: 'Installation annulée',
  signature: "Version installée signée par une autre clé que les versions publiées : désinstallation puis réinstallation depuis le PC nécessaires (une seule fois). Profil à choisir de nouveau ensuite.",
  echec: 'Installation refusée par Android',
};

let apkListening = false;
async function installAndroid(): Promise<void> {
  if (!apkListening) {
    apkListening = true;
    window.addEventListener('ludo-apk', (ev) => {
      const { etape, pourcent } = (ev as CustomEvent<{ etape: string; pourcent: number | null }>).detail;
      if (etape === 'telechargement') {
        appUpdate.status = 'downloading';
        appUpdate.percent = pourcent;
      } else if (etape === 'installation' || etape === 'confirmation' || etape === 'installee') {
        appUpdate.percent = 100;
        appUpdate.status = 'restarting';
      } else if (etape === 'signature') {
        // Refus définitif tant que l'app installée n'est pas remplacée à la main : expliqué, sans
        // bouton « Réessayer » qui échouerait à chaque appui. Version retenue : la prochaine
        // vérification (retour au premier plan) ne la repropose pas.
        rememberRefused(appUpdate.version);
        appUpdate.info = APK_ETAPES.signature;
        appUpdate.status = 'indisponible';
      } else if (APK_ETAPES[etape]) {
        appUpdate.error = APK_ETAPES[etape];
        appUpdate.status = 'error';
      }
    });
  }
  appUpdate.status = 'downloading';
  appUpdate.percent = 0;
  appUpdate.error = '';
  try {
    await androidInvoke<void>('apkInstaller');
    if (appUpdate.status === 'downloading') appUpdate.status = 'restarting';
  } catch (e) {
    appUpdate.error = message(e);
    appUpdate.status = 'error';
  }
}

/** Cherche une version plus récente. Sans effet hors de l'app (démo dans un navigateur). */
export async function checkAppUpdate(): Promise<void> {
  if (isAndroid && !isDemo && appUpdate.status !== 'checking' && appUpdate.status !== 'downloading' && appUpdate.status !== 'restarting') return checkAndroid();
  if (isDemo || isAndroid || appUpdate.status === 'checking' || appUpdate.status === 'downloading' || appUpdate.status === 'restarting') return;
  appUpdate.status = 'checking';
  appUpdate.error = '';
  try {
    pending = await check();
    appUpdate.checkedAt = new Date();
    if (pending) {
      appUpdate.version = pending.version;
      appUpdate.notes = pending.body ?? '';
      appUpdate.status = 'available';
    } else {
      appUpdate.status = 'none';
    }
  } catch (e) {
    appUpdate.checkedAt = new Date();
    appUpdate.error = message(e);
    appUpdate.status = 'error';
  }
}

/** Télécharge (signature vérifiée), installe, puis relance l'app. */
export async function installAppUpdate(): Promise<void> {
  if (isAndroid && !isDemo) {
    if (appUpdate.status === 'available' || appUpdate.status === 'error') await installAndroid();
    return;
  }
  if (!pending || appUpdate.status === 'downloading' || appUpdate.status === 'restarting') return;
  appUpdate.status = 'downloading';
  appUpdate.percent = 0;
  appUpdate.error = '';
  let total = 0;
  let done = 0;
  try {
    await pending.downloadAndInstall((ev) => {
      if (ev.event === 'Started') {
        total = ev.data.contentLength ?? 0;
        appUpdate.percent = total ? 0 : null;
      } else if (ev.event === 'Progress') {
        done += ev.data.chunkLength;
        appUpdate.percent = total ? Math.min(100, Math.round((done / total) * 100)) : null;
      } else if (ev.event === 'Finished') {
        appUpdate.percent = 100;
      }
    });
    appUpdate.status = 'restarting';
    await relaunch();
  } catch (e) {
    appUpdate.error = message(e);
    appUpdate.status = 'error';
  }
}

/** Recherche déclenchée par des événements, jamais par minuterie : au démarrage, quand la fenêtre
 *  reprend le focus (retour devant l'écran) et à l'ouverture de l'écran Mises à jour. Au focus, pas
 *  plus d'une recherche par tranche de 10 min (on ne sollicite pas GitHub à chaque alt-tab). */
const FOCUS_MIN_GAP_MS = 10 * 60 * 1000;
let listening = false;
export function startAppUpdateChecks(): void {
  void checkAppUpdate();
  if (listening || isDemo) return;
  if (isAndroid) {
    // Android : retour au premier plan (autorisation « sources inconnues » accordée dans les réglages,
    // retour d'un jeu) ; même garde de 10 min que le focus du bureau.
    listening = true;
    document.addEventListener('visibilitychange', () => {
      const recent = appUpdate.checkedAt && Date.now() - appUpdate.checkedAt.getTime() < FOCUS_MIN_GAP_MS;
      if (document.visibilityState === 'visible' && !recent && (appUpdate.status === 'idle' || appUpdate.status === 'none' || appUpdate.status === 'error' || appUpdate.status === 'indisponible')) void checkAppUpdate();
    });
    return;
  }
  listening = true;
  void import('@tauri-apps/api/window').then(({ getCurrentWindow }) =>
    getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      const recent = appUpdate.checkedAt && Date.now() - appUpdate.checkedAt.getTime() < FOCUS_MIN_GAP_MS;
      if (focused && !recent && (appUpdate.status === 'idle' || appUpdate.status === 'none' || appUpdate.status === 'error')) void checkAppUpdate();
    }),
  );
}
