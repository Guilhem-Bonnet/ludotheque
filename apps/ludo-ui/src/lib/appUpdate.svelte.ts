// Mise à jour de l'app elle-même (pas des jeux) : releases signées publiées par la CI sur GitHub
// (`latest.json`), vérifiées avec la clé publique de tauri.conf.json avant toute installation.
// Vérification au démarrage et sur demande (Réglages) ; l'installation remplace l'AppImage puis relance.
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { isAndroid, isDemo } from './api';

export type AppUpdateStatus = 'idle' | 'checking' | 'none' | 'available' | 'downloading' | 'restarting' | 'error';

export const appUpdate = $state<{
  status: AppUpdateStatus;
  /** Version proposée (`0.5.0`). */
  version: string;
  notes: string;
  /** Avancement du téléchargement, 0-100 ; `null` si la taille n'est pas connue. */
  percent: number | null;
  error: string;
  checkedAt: Date | null;
}>({ status: 'idle', version: '', notes: '', percent: null, error: '', checkedAt: null });

let pending: Update | null = null;

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** Cherche une version plus récente. Sans effet hors de l'app (démo dans un navigateur). */
export async function checkAppUpdate(): Promise<void> {
  // Android : pas de mise à jour intégrée (l'APK est installé par le PC).
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
  if (listening || isDemo || isAndroid) return;
  listening = true;
  void import('@tauri-apps/api/window').then(({ getCurrentWindow }) =>
    getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      const recent = appUpdate.checkedAt && Date.now() - appUpdate.checkedAt.getTime() < FOCUS_MIN_GAP_MS;
      if (focused && !recent && (appUpdate.status === 'idle' || appUpdate.status === 'none' || appUpdate.status === 'error')) void checkAppUpdate();
    }),
  );
}
