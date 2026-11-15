# Ludothèque

Bibliothèque de jeux maison : ROM sur le serveur, profils façon Switch, sauvegardes synchronisées façon Steam Cloud entre le PC, le Steam Deck et l'Ayn Thor. Usage local (LAN) seulement.

Conception complète : `Terraform-HouseServer/docs/ludotheque-conception.md`.

## Contenu

| Crate | Rôle |
|---|---|
| `ludo-proto` | Types du protocole partagés par le serveur et les clients |
| `ludo-core` | Décision de synchronisation (pure), empreinte et archive neutre des sauvegardes, fiches d'émulateur |
| `ludo-server` | API HTTP : sauvegardes versionnées (compare-and-swap), historique, verrous de partie |

`fiches/` : une fiche TOML par émulateur (lancement, sauvegardes, profils, chemins par appareil).

## Conversion NSZ → NSP (Eden)

Eden ne lit pas les Switch compressés. Le client envoie les formats de son émulateur (`?formats=nsp,xci`) ; si le fichier stocké est un NSZ / XCZ non accepté, le serveur le décompresse avec `nsz -D` dans un cache à plafond (éviction LRU) et répond `202` + `Retry-After` en attendant. Variables : `LUDO_SWITCH_KEYS` (prod.keys, secret monté), `LUDO_CACHE` (volume non sauvegardé), `LUDO_CACHE_MAX_GB` (80), `LUDO_CONVERT_CMD` (`nsz`). Sans clés : `503 Conversion indisponible : clés manquantes`. L'image embarque nsz 5.0.0 (Python, Debian slim).

## Développement

Les artefacts de compilation vont dans `~/.cache/cargo-target/ludotheque` (`.cargo/config.toml`), jamais sur `/mnt/Travail`.

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
LUDO_DATA=./data cargo run -p ludo-server   # écoute sur 0.0.0.0:8080
```

## État

- [x] P1a : cœur de synchronisation, archive neutre, fiches, serveur (sauvegardes + verrous)
- [ ] P1b : démon `ludod` + `ludo run` (Linux), adaptateurs Ryujinx/Eden, écran de passage
- [ ] P1c : catalogue des ROM, livraison au bon format, paquets système, déploiement K3s
- [ ] P2 : app Bibliothèque (Linux) + catalogue web
- [ ] P3 : app Android (Thor)
