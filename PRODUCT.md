# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

Tauri 2 (décision de Guilhem du 15/11/2026) : interface web (HTML/CSS/TS) dans une app Rust native. Le même code d'interface sert au Steam Deck, au PC (y compris en Moonlight), à l'Ayn Thor (Tauri Android) et au catalogue web de la phase 2. Le cœur métier reste en Rust : crates `ludo-core`, `ludo-client` et `ludo-server`.

## Users

Deux personnes d'un même foyer, qui jouent à la manette :
- **Guilhem** : PC (souvent streamé par Moonlight vers la Thor ou une TV), Ayn Thor (console Android à double écran).
- **Claire** : Steam Deck.

Ils lancent un jeu Nintendo émulé et veulent reprendre leur partie là où ils l'ont laissée, sur n'importe quel appareil, sans rien régler.

## Product Purpose

Une ludothèque maison : les ROM (DS, 3DS, Switch, Switch 2, Wii U, GameCube) sont rangées sur le serveur, chaque personne a son profil comme sur la Switch (sans mot de passe), et les sauvegardes se synchronisent comme avec Steam Cloud entre le PC, le Steam Deck et l'Ayn Thor. Réussite : ne plus jamais copier de fichier à la main, ne jamais perdre de progression, ne jamais se tromper de profil.

## Positioning

Un service fait sur mesure pour ce foyer, au lieu d'un produit générique comme RomM. Il connaît chaque émulateur réellement installé (Ryujinx, Eden, Lichii, Azahar, melonDS, Cemu, Dolphin) et traduit une même sauvegarde d'un émulateur à l'autre. Sur chaque appareil, il se fond dans le lanceur natif (bibliothèque Steam, Cocoon), au lieu d'imposer une app de plus pour lancer un jeu.

## Operating Context

- **Usage à la manette**, sans clavier ni souris : Steam en mode Jeu sur le Deck (1280×800, 7 pouces), Steam Big Picture sur le PC, flux Moonlight compressé vers une TV ou la Thor (1080p), Cocoon sur la Thor (écran principal en haut, second écran en bas).
- **Local seulement** : un serveur sur le LAN (K3s), pas d'Internet.
- Le jeu doit toujours démarrer, même serveur éteint (« Hors ligne – sauvegarde locale utilisée »).
- Surfaces : un écran de passage avant et après chaque jeu (choix du profil, téléchargement, conflit, envoi) ; une app Bibliothèque (jeux, état sur l'appareil, mises à jour, profils, historique des sauvegardes) ; des indicateurs discrets propres à chaque plateforme (collections Steam, notification Android).

## Capabilities and Constraints

- Une sauvegarde par jeu et par profil, versionnée, avec un historique restaurable. Un conflit se tranche en un seul choix, et la version écartée reste dans l'historique.
- Cinq états de synchronisation : À jour, Synchronisation…, Envoi en attente, Conflit de sauvegarde, Hors ligne.
- Jeux et émulateurs décrits par des fiches. Ryujinx est l'émulateur Switch par défaut, Eden le second (seul choix sur la Thor).
- Pas de pastille possible dans la grille de Steam ou de Cocoon : l'état vit dans l'écran de passage, l'app Bibliothèque et les surfaces natives.
- Non décidé : jaquettes HD issues d'un service externe (pour l'instant, icônes et noms lus dans les ROM elles-mêmes).

## Brand Commitments

- Nom : **Ludothèque**.
- Textes **sans pronom ni phrase**, à la manière de Steam : « Prêt », « Jouer », « Télécharger », « À jour », « Hors ligne », « Conflit de sauvegarde ». Jamais de tutoiement ni de ton chaleureux.
- Profils avec **avatar en image choisie** par chaque personne.
- Identité visuelle propre à la Ludothèque, inspirée de Steam Big Picture (décision du 15/11/2026), distincte du design system existant de Guilhem.

## Evidence on Hand

- Conception complète : `Terraform-HouseServer/docs/ludotheque-conception.md` (sections 3 et 10 : parcours, langage visuel, textes).
- Aucun avatar, logo ni jaquette fourni pour l'instant : ne rien inventer comme définitif.

## Product Principles

1. Rien quand tout va bien : l'interface ne parle que s'il y a quelque chose à faire.
2. Le jeu démarre toujours ; la synchronisation ne bloque jamais plus de quelques secondes.
3. Aucune progression perdue : rien n'est écrasé sans choix explicite, et tout reste dans l'historique.
4. Natif d'abord : s'intégrer à Steam et à Cocoon plutôt que les remplacer.
5. Tout se fait à la manette, lisible de loin et sur petit écran.

## Accessibility & Inclusion

Navigation complète à la manette (focus toujours visible, pas d'action réservée au tactile ou au pointeur). États lisibles sans la couleur (forme + libellé), y compris dans un flux vidéo compressé.
