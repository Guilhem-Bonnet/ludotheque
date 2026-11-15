# Image du serveur Ludothèque (ludo-server : axum + SQLite embarqué).
# Construction : docker build -t ghcr.io/guilhem-bonnet/ludo-server .
# Aucun secret dans l'image ; toute la configuration passe par des variables d'environnement.

FROM rust:1.94-bookworm AS build
WORKDIR /src
# Limite le parallélisme (la compilation de rusqlite/tokio est gourmande en mémoire).
ARG CARGO_BUILD_JOBS=4
ENV CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS} CARGO_TARGET_DIR=/src/target
COPY . .
# --locked : refuse de construire si Cargo.lock n'est pas à jour.
RUN cargo build --release --locked -p ludo-server \
 && install -D target/release/ludo-server /out/ludo-server

# Convertisseur NSZ → NSP / XCZ → XCI : nsz (Python) installé dans un venv, épinglé.
# Même base que l'image d'exécution (même Python) pour que le venv y soit utilisable tel quel.
# Base figée par digest (bookworm-slim relevé le 15/11/2026) ; à mettre à jour avec les correctifs Debian.
FROM debian:bookworm-slim@sha256:c67e92c8099ba22853ab4cfda7cfb494d253d6012ca82dee98ba8976772af018 AS nsz
RUN apt-get update \
 && apt-get install -y --no-install-recommends python3 python3-venv python3-pip \
 && rm -rf /var/lib/apt/lists/*
# nsz 5.0.0 et TOUTES ses dépendances sont figées par version et par hash (requirements-nsz.txt,
# --require-hashes) : la construction échoue si un paquet change.
COPY requirements-nsz.txt /tmp/requirements-nsz.txt
RUN python3 -m venv /opt/nsz \
 && /opt/nsz/bin/pip install --no-cache-dir --require-hashes --no-deps --only-binary=:all: -r /tmp/requirements-nsz.txt \
 && find /opt/nsz -name __pycache__ -type d -prune -exec rm -rf {} + \
 && /opt/nsz/bin/nsz --help > /dev/null

# Exécution : Debian slim + Python (nsz en a besoin), utilisateur non-root 65532, aucun shell
# d'entrée (l'ENTRYPOINT lance directement ludo-server). Système de fichiers en lecture seule
# compatible : seuls /data, /cache et /tmp sont à monter en écriture.
FROM debian:bookworm-slim@sha256:c67e92c8099ba22853ab4cfda7cfb494d253d6012ca82dee98ba8976772af018
RUN apt-get update \
 && apt-get install -y --no-install-recommends python3 \
 && rm -rf /var/lib/apt/lists/* /var/cache/apt /usr/share/doc /usr/share/man \
 && mkdir -p /data /cache && chown 65532:65532 /data /cache
COPY --from=nsz /opt/nsz /opt/nsz
COPY --from=build /out/ludo-server /usr/local/bin/ludo-server
# Dossier de données (SQLite + sauvegardes) : à monter sur un volume persistant.
# Cache des conversions (LUDO_CACHE) : volume SÉPARÉ, non sauvegardé. Clés de la console
# (LUDO_SWITCH_KEYS) : fichier monté depuis un secret, jamais dans l'image.
ENV LUDO_DATA=/data \
    LUDO_ADDR=0.0.0.0:8080 \
    LUDO_CACHE=/cache \
    LUDO_CONVERT_CMD=/opt/nsz/bin/nsz \
    PYTHONDONTWRITEBYTECODE=1
# Dossier des ROM (LUDO_ROMS) : à monter en lecture seule ; absent = catalogue vide.
VOLUME ["/data", "/cache"]
EXPOSE 8080
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/ludo-server"]
