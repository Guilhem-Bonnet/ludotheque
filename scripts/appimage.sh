#!/usr/bin/env bash
# Construit l'AppImage de l'app Ludothèque (release), avec GStreamer embarqué : sans lui, le son
# de l'interface (Web Audio → GStreamer « autoaudiosink ») ne sort pas sur les systèmes qui n'ont pas
# les bons plugins (vu sur Fedora ; le Steam Deck en profite aussi).
# Prérequis : patchelf dans le PATH (`pipx install patchelf`), plugins GStreamer installés sur la machine.
# Mémoire : build dans un scope systemd plafonné (systemd-oomd tue sinon la session).
set -euo pipefail
cd "$(dirname "$0")/../apps/ludo-ui"
PLUGINS=${GSTREAMER_PLUGINS_DIR:-$( [ -d /usr/lib64/gstreamer-1.0 ] && echo /usr/lib64/gstreamer-1.0 || echo /usr/lib/x86_64-linux-gnu/gstreamer-1.0 )}
HELPERS=${GSTREAMER_HELPERS_DIR:-$( [ -d /usr/libexec/gstreamer-1.0 ] && echo /usr/libexec/gstreamer-1.0 || echo /usr/lib/x86_64-linux-gnu/gstreamer1.0/gstreamer-1.0 )}
exec systemd-run --user --scope --quiet --slice=agents.slice -p MemoryHigh=6G -p MemoryMax=10G \
  env PATH="$HOME/.local/bin:$PATH" CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}" NO_STRIP=true \
  GSTREAMER_PLUGINS_DIR="$PLUGINS" GSTREAMER_HELPERS_DIR="$HELPERS" \
  npx tauri build --bundles appimage "$@"
