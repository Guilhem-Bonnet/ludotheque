#!/usr/bin/env bash
# Construit l'APK Android de la Ludothèque (arm64, Ayn Thor) et, avec --installer, l'installe par adb
# puis dépose la configuration de l'appareil dans Android/data/<paquet>/files/config.toml et accorde à
# l'app l'accès « tous les fichiers » (lancement des jeux sans aucun sélecteur de dossier).
#
# Usage : scripts/apk.sh [--debug] [--installer [SERIE]]
#         scripts/apk.sh [--debug] --apk FICHIER.apk [SERIE]   installe un APK déjà construit (artefact de la CI)
#         scripts/apk.sh [--debug] --config [SERIE]            configuration + accès seulement (ni build ni install)
#
# Paquet visé : la release `fr.srvdreamer.ludotheque` par défaut ; `--debug` : build de développement et
# paquet `fr.srvdreamer.ludotheque.debug`. (`--release`, ancienne option, est accepté et ne change rien.)
#
# Outils attendus (installés sans droits administrateur, voir README.md, « App Android ») :
#   JDK 17/21 (JAVA_HOME, défaut ~/.cache/jdk/jdk-21*), SDK Android (ANDROID_HOME, défaut ~/.cache/android-sdk)
#   avec platforms;android-37.0, build-tools, NDK (NDK_HOME, défaut le plus récent de $ANDROID_HOME/ndk),
#   cible Rust aarch64-linux-android.
#
# Verrou du cache cargo tenu par une autre session (« Blocking waiting for file lock on package cache ») :
# CARGO_HOME=<dossier avec liens vers ~/.cargo/registry et ~/.cargo/git> scripts/apk.sh
#
# Compilation dans un scope systemd plafonné (comme cargo-borne.sh) ; artefacts Rust et Gradle hors du
# dépôt (~/.cache) : le disque du dépôt a déjà été saturé par un target/.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
profile=release
install=""
apk=""
config_only=""
serial="${LUDO_ADB_SERIAL:-}"
while [ $# -gt 0 ]; do
  case "$1" in
    --release) profile=release ;;
    --debug) profile=debug ;;
    --installer) install=1; if [ $# -gt 1 ] && [ "${2#-}" = "$2" ]; then serial="$2"; shift; fi ;;
    --apk) [ $# -gt 1 ] || { echo "--apk : fichier attendu" >&2; exit 2; }; apk="$2"; install=1; shift
      if [ $# -gt 1 ] && [ "${2#-}" = "$2" ]; then serial="$2"; shift; fi ;;
    --config) config_only=1; install=1; if [ $# -gt 1 ] && [ "${2#-}" = "$2" ]; then serial="$2"; shift; fi ;;
    -h|--help) sed -n '2,21p' "$0"; exit 0 ;;
    *) echo "Option inconnue : $1" >&2; exit 2 ;;
  esac
  shift
done
[ -z "$apk" ] || [ -f "$apk" ] || { echo "APK introuvable : $apk" >&2; exit 1; }

adb_dev=(adb)
[ -n "$serial" ] && adb_dev=(adb -s "$serial")
package=fr.srvdreamer.ludotheque
[ "$profile" = debug ] && package="$package.debug"

# Configuration de l'appareil et accès aux fichiers (fonction appelée après l'installation éventuelle).
deposer_config() {
  # Configuration : serveur, appareil et profil de ~/.config/ludo/thor.toml, hôte Moonlight = UUID de ce PC
  # dans Sunshine. Un fichier déjà présent sur l'appareil est gardé en .ludo-old-<date>.
  local conf_src="${LUDO_THOR_CONFIG:-$HOME/.config/ludo/thor.toml}" uuid tmp dest
  uuid="$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["root"]["uniqueid"])' "$HOME/.config/sunshine/sunshine_state.json" 2>/dev/null || true)"
  tmp="$(mktemp)"
  {
    echo "# Ludothèque — $(date +%d/%m/%Y) : déposée par scripts/apk.sh depuis $(hostname)."
    grep -E '^(serveur|appareil|profil_defaut) *=' "$conf_src"
    echo
    echo "[moonlight]"
    [ -n "$uuid" ] && echo "hote = \"$uuid\""
    echo "hdr = false"
  } > "$tmp"
  dest="/sdcard/Android/data/$package/files"
  "${adb_dev[@]}" shell "mkdir -p $dest && if [ -f $dest/config.toml ]; then mv $dest/config.toml $dest/config.toml.ludo-old-$(date +%Y%m%d-%H%M%S); fi"
  "${adb_dev[@]}" push "$tmp" "$dest/config.toml" >/dev/null
  rm -f "$tmp"
  echo "Configuration déposée : $dest/config.toml"
  # Accès « tous les fichiers » : ROM lues par chemin, confiées aux émulateurs sans arbre (Dolphin) ;
  # l'app ne le demande jamais elle-même (aucun écran de réglage, aucun sélecteur).
  if "${adb_dev[@]}" shell appops set "$package" MANAGE_EXTERNAL_STORAGE allow; then
    echo "Accès aux fichiers : $("${adb_dev[@]}" shell appops get "$package" MANAGE_EXTERNAL_STORAGE | head -1)"
  else
    echo "Accès aux fichiers non accordé (appops) : à accorder dans Réglages › Applications › Ludothèque" >&2
  fi
}

if [ -n "$config_only" ]; then
  deposer_config
  exit 0
fi
if [ -n "$apk" ]; then
  "${adb_dev[@]}" install -r "$apk"
  deposer_config
  exit 0
fi

export ANDROID_HOME="${ANDROID_HOME:-$HOME/.cache/android-sdk}"
if [ -z "${JAVA_HOME:-}" ]; then
  JAVA_HOME="$(ls -d "$HOME"/.cache/jdk/jdk-21* 2>/dev/null | sort | tail -1)"
  export JAVA_HOME
fi
if [ -z "${NDK_HOME:-}" ]; then
  NDK_HOME="$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)"
  export NDK_HOME
fi
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/cargo-target/ludotheque-android}"
for v in JAVA_HOME ANDROID_HOME NDK_HOME; do
  [ -d "${!v}" ] || { echo "$v introuvable (${!v:-vide})" >&2; exit 1; }
done

# Dossiers de construction Gradle : liens vers ~/.cache (ignorés par git), UN cache par checkout : le
# cache de configuration de Gradle retient le chemin absolu du projet (un cache partagé entre deux
# worktrees relance la construction dans l'autre worktree, puis échoue).
gen="$root/apps/ludo-ui/src-tauri/gen/android"
cache="$HOME/.cache/gradle-build/ludotheque-$(basename "$root")"
for d in app/build buildSrc/build build .gradle; do
  if [ ! -e "$gen/$d" ]; then
    mkdir -p "$cache/$d" "$(dirname "$gen/$d")"
    ln -s "$cache/$d" "$gen/$d"
  fi
done

cd "$root/apps/ludo-ui"
[ -d node_modules ] || npm ci --no-audit --no-fund
flags=(--apk --target aarch64)
[ "$profile" = debug ] && flags+=(--debug)
systemd-run --user --scope --quiet --slice=agents.slice \
  -p MemoryHigh="${CARGO_MEM_HIGH:-6G}" -p MemoryMax="${CARGO_MEM_MAX:-10G}" \
  -- env CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}" npx tauri android build "${flags[@]}"

apk="$(find -L "$gen/app/build/outputs/apk" -name '*.apk' -path "*$profile*" -newer "$gen/app/build.gradle.kts" | sort | tail -1)"
[ -n "$apk" ] || { echo "APK introuvable sous $gen/app/build/outputs/apk" >&2; exit 1; }
echo "APK : $apk"
[ -n "$install" ] || exit 0

"${adb_dev[@]}" install -r "$apk"
deposer_config
