#!/usr/bin/env bash
# Shizuku sur la Thor (ou tout appareil Android adb) : « Fermer le jeu » de la Ludothèque arrête alors
# l'émulateur sans aucune page d'Android (ArretDirect.kt).
#
# Usage : scripts/shizuku-thor.sh [SERIE]             installe ou met à jour Shizuku (fork thedjchi, version
#                                                    épinglée, SHA-256 vérifié)
#         scripts/shizuku-thor.sh --retablir [SERIE]  arrête le serveur, retire les accès à Shizuku
#                                                    (shizuku.json gardé en .ludo-old-<date>), port adb persistant
#                                                    remis à vide. Shizuku reste installé
#                                                    (`adb uninstall moe.shizuku.privileged.api` pour l'enlever).
#
# La préparation (accès des apps Ludothèque dans shizuku.json, droits d'arrière-plan, port adb TCP
# persistant, serveur démarré) est refaite à chaque connexion de la Thor par `ludo adb-sync`, ou à la
# demande par `ludo --config ~/.config/ludo/thor.toml shizuku`.
#
# Geste unique sur la Thor, impossible par adb (Android 13 refuse au shell de changer un composant d'une
# autre app) : ouvrir Shizuku une fois serveur démarré, puis Réglages › « Start on boot ». Shizuku se
# relance alors seul après un redémarrage, en mode TCP (port 5555 persistant), sans le PC ; à la toute
# première fois, Android demande d'autoriser le débogage pour la clé de Shizuku : « Toujours autoriser ».
set -euo pipefail

# Fork thedjchi (Apache-2.0) : « Start on boot » sans root, mode TCP, relance après plantage.
# Certificat de signature : CN=Alex Chi, O=thedjchi, SHA-256
# 94:7E:81:ED:49:D6:CD:E4:00:6B:D6:4B:94:AB:69:E6:3F:50:07:4C:E0:14:51:D1:BF:30:AC:B1:4E:59:1F:37
VERSION="v13.7.0-thedjchi"
URL="https://github.com/thedjchi/Shizuku/releases/download/$VERSION/shizuku-$VERSION.apk"
SHA256="6ea6dee65d5ddc626b6b75b2c2f67f8cc547fa47d7b437e6892639c37eaffe43"
PAQUET="moe.shizuku.privileged.api"
CONFIG="/data/user_de/0/com.android.shell/shizuku.json"

retablir=""
serial="${LUDO_ADB_SERIAL:-}"
while [ $# -gt 0 ]; do
  case "$1" in
    --retablir) retablir=1 ;;
    -h|--help) sed -n '2,18p' "$0"; exit 0 ;;
    -*) echo "Option inconnue : $1" >&2; exit 2 ;;
    *) serial="$1" ;;
  esac
  shift
done
adb_dev=(adb)
[ -n "$serial" ] && adb_dev=(adb -s "$serial")

if [ -n "$retablir" ]; then
  # Un seul argument entre apostrophes : le script s'exécute tel quel sur l'appareil.
  "${adb_dev[@]}" shell '
    p="$(pidof shizuku_server)"; [ -z "$p" ] || kill -9 $p
    [ ! -f '"$CONFIG"' ] || mv '"$CONFIG"' '"$CONFIG"'.ludo-old-$(date +%Y%m%d-%H%M%S)
    setprop persist.adb.tcp.port ""
    echo "Shizuku arrêté, accès retirés, port adb persistant : \"$(getprop persist.adb.tcp.port)\""'
  exit 0
fi

cache="${XDG_CACHE_HOME:-$HOME/.cache}/ludotheque/shizuku"
mkdir -p "$cache"
apk="$cache/shizuku-$VERSION.apk"
if ! echo "$SHA256  $apk" | sha256sum -c --status 2>/dev/null; then
  curl -fsSL -o "$apk.part" "$URL"
  echo "$SHA256  $apk.part" | sha256sum -c --status || { echo "SHA-256 inattendu pour $URL : rien installé" >&2; exit 1; }
  mv "$apk.part" "$apk"
fi
installee="$("${adb_dev[@]}" shell dumpsys package "$PAQUET" | sed -n 's/^ *versionName=//p' | head -n 1 | tr -d '\r')"
if [ "$installee" = "${VERSION#v}" ]; then
  echo "Shizuku $installee déjà installé"
else
  "${adb_dev[@]}" install -r "$apk"
  echo "Shizuku ${VERSION#v} installé (avant : ${installee:-absent})"
fi
echo "Préparation : ludo --config ~/.config/ludo/thor.toml shizuku (refaite à chaque connexion par ludo adb-sync)"
