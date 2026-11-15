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
 && install -D target/release/ludo-server /out/ludo-server \
 && mkdir -p /out/data && chown 65532:65532 /out/data

# Exécution : distroless « cc » (glibc + libgcc, pas de shell), utilisateur non-root 65532.
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /out/ludo-server /usr/local/bin/ludo-server
# Dossier de données (SQLite + sauvegardes) : à monter sur un volume persistant.
COPY --from=build --chown=65532:65532 /out/data /data
ENV LUDO_DATA=/data \
    LUDO_ADDR=0.0.0.0:8080
# Dossier des ROM (LUDO_ROMS) : à monter en lecture seule ; absent = catalogue vide.
VOLUME ["/data"]
EXPOSE 8080
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/ludo-server"]
