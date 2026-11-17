# Ludothèque — règles de développement

## Systèmes événementiels : jamais de scrutation par minuterie

Un changement d'état se **reçoit**, il ne se **cherche** pas à intervalle régulier. Interdits pour
détecter un changement : `setInterval`, `loop { sleep(…); vérifier }`, `recv_timeout` court pour
relire un drapeau, timer systemd « toutes les X min », relance HTTP en boucle.

| Besoin | Source d'événement utilisée dans le dépôt |
|---|---|
| Fichiers modifiés (PC) | inotify (crate `notify`), anti-rebond sur le **dernier** événement |
| Fichiers modifiés (Android, sans root) | `adb shell inotifyd - <dossier>:wmn` (toybox : `w` écrit, `m` renommé vers, `n` créé) |
| Appareil branché / débranché | `adb track-devices` (flux poussé, USB et Wi-Fi) |
| Fin d'un processus Android | `adb shell logcat -b events -s am_proc_died` |
| Fin d'un processus local | `Child::wait()` dans un fil + canal |
| Signal (SIGTERM, Ctrl+C) | `signal-hook` (itérateur dans un fil + canal) |
| Arrêt / annulation | `launch::Stop`, `install::Cancel` (Condvar + réveils) |
| Progression côté serveur | requête longue `?attendre=1` (canal `watch` du convertisseur) |
| Nouvelle version de l'app | démarrage, focus de la fenêtre, ouverture de l'écran Mises à jour |

**Exceptions admises, justifiées en commentaire au point d'usage** :
- renouvellement d'un **bail à durée de vie** (protocole : TTL 120 s, renouvelé toutes les 40 s) ;
- **délai de garde unique** calculé (`recv_timeout(reste)`, timeout réseau, SIGKILL après grâce) ;
- **rythme** d'animation ou de son (`sound::syncRhythm`), qui ne lit aucun état ;
- une **seule** attente de repli quand la source d'événement elle-même tombe (ex. serveur adb relancé).

## Autres règles

- Compilation : toujours `scripts/cargo-borne.sh` (scope systemd plafonné, sinon systemd-oomd tue la session).
- Textes d'interface : sans pronom, style Steam (« Prêt », « Jouer »), jamais de tutoiement.
- Une sauvegarde locale modifiée n'est jamais écrasée : passer par `ludo_core::sync::decide` (conflit sinon).
- Rien n'est supprimé hors du dossier de la Ludothèque (collection existante, appareils) : on renomme
  en `.ludo-old-<date>`.
