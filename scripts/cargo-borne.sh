#!/usr/bin/env bash
# Lance cargo dans son propre scope systemd, plafonné en mémoire, à côté de la session Claude
# (et non dedans). Sous pression mémoire, systemd-oomd tue alors la compilation, pas la session.
# Usage : scripts/cargo-borne.sh test --workspace
set -euo pipefail
exec systemd-run --user --scope --quiet --slice=agents.slice \
  -p MemoryHigh="${CARGO_MEM_HIGH:-6G}" -p MemoryMax="${CARGO_MEM_MAX:-10G}" \
  -- env CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}" cargo "$@"
