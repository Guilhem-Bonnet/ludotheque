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
- [ ] P3 : app Android (Thor) — MVP : bibliothèque, lancement Eden / Azahar / melonDS, flux Moonlight

## App Android (Ayn Thor)

`scripts/apk.sh` construit l'APK arm64 (`--release` pour une version optimisée) ; `--installer [SERIE]`
l'installe par adb et dépose `config.toml` dans `/sdcard/Android/data/<paquet>/files/` (serveur, appareil
et profil de `~/.config/ludo/thor.toml`, hôte Moonlight = UUID Sunshine du PC). Outils, sans droits
administrateur : JDK 21 dans `~/.cache/jdk`, SDK Android (commandlinetools → `sdkmanager
"platforms;android-37.0" "build-tools;35.0.0" "ndk;27.3.13750724"`) dans `~/.cache/android-sdk`,
`rustup target add aarch64-linux-android`.

- Lancement : ROM trouvée dans l'arbre `Documents/Roms` accordé une fois (sélecteur Android, mémorisé)
  → émulateur Android par intent explicite ; sinon flux Moonlight (`ShortcutTrampoline`, extras `UUID`,
  `AppName`, `HDR`), l'appli Sunshine venant de l'inventaire publié par le PC (`stream_app`) ou de
  `[moonlight.applis]` dans la configuration.
- Sauvegardes : jamais touchées par l'app (Android/data illisible) ; le PC les synchronise
  (`ludo adb-watch`, service `ludo-thor-watch`).
- Code : `apps/ludo-ui/src-tauri/src/android.rs` (logique testée sur le PC) et le plugin Kotlin
  `gen/android/app/src/main/java/fr/srvdreamer/ludotheque/LudoPlugin.kt`.
