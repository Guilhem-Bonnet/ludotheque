# Charte mouvement, son et vibration

Validée en partie par Guilhem le 15/11/2026 : la palette sonore « Feutré doux v6 » est validée, le mouvement reste à valider sur la démo.

## Principes

- Chaque événement déclenche **une animation, un son et une vibration**, sur la même image (`src/lib/feedback.ts`).
- Caractère du mouvement : **doux et joueur**.
  - Courbe signature `--ease-land` (atterrissage doux), utilisée pour 80 % des mouvements.
  - Rebond léger `--ease-pop` (environ 4 %), réservé aux petits éléments.
  - Jamais de rebond sur un écran entier.
- Trois durées : **120 / 240 / 420 ms**. Les entrées durent plus longtemps que les sorties.
- Trois couches de mouvement :
  - **principale** : l'élément qui bouge ;
  - **secondaire** : l'ombre qui s'élève, l'onglet qui suit ;
  - **ambiance** : le halo qui respire, la lueur de fond qui dérive.
- Sons (`src/lib/sound.ts`) :
  - sinus, attaque d'au moins 12 ms, passe-bas ;
  - même son sur toutes les tuiles ;
  - **jamais de son qui descend pour un moment positif.**
- Si le système demande moins d'animations, tous les déplacements et changements d'échelle deviennent instantanés. Les sons et les vibrations restent.

## Événements

| Événement | Animation | Son | Vibration |
|---|---|---|---|
| Déplacement | La tuile grandit avec un léger rebond, l'ombre s'élève, le halo respire | `move` (identique sur toutes les tuiles) | toucher léger, 14 ms |
| Bord atteint | Poussée de 5 px dans la direction | aucun | aucune |
| A (valider) | Enfoncement à 96 %, puis rebond | `ok` | 28 ms |
| B (retour) | L'écran glisse vers la droite | `back` | 18 ms |
| L1 / R1 | La grille glisse du côté de l'onglet, cellules en cascade de 25 ms | `tab` | 12 ms |
| Action impossible | — | `err` | deux petits chocs |
| Changement d'écran | View Transition : l'ancien écran part en 200 ms, le nouveau arrive en 320 ms ; la tuile choisie « s'envole » vers l'écran suivant (élément partagé) | — | — |
| Qui joue ? | Avatars en cascade ; le profil choisi grandit, l'autre s'efface ; l'accent de l'interface prend sa couleur en 420 ms | `arrive` | 50 ms |
| Écran de passage | Étapes en cascade, coche qui se dessine, barre continue | `loading` à l'arrivée, `downloaded` à la fin du téléchargement | 50 ms |
| Lancement | La tuile se soulève avec un « pop » | `launch` (montée douce de 4 notes) | 4 impulsions croissantes |
| Conflit | Les cartes montent ; la carte choisie se soulève, l'autre s'efface vers le bas | `conflict` (une note), puis `saved` | 80 ms |
| Sauvegarde restaurée / envoyée | « Pop » de la ligne | `saved` (accord doux) | 60 ms |
| Mise à jour, installation | Mini-barre, puis coche qui se dessine | `updated` / `downloaded` | 50 ms |

## À affiner (retour de Guilhem)

- Les sons `loading`, `downloaded` et `updated` sont des propositions dans l'esprit validé, pas encore écoutées.
- Les vibrations sont à essayer sur le Deck : intensités et durées.
- Démo à essayer : https://claude.ai/artifact/5noPFPFZmKzUXpSh4PU5sL

## Passe d'animation du 15/11 (skills impeccable `animate` + motion-design)

Thèse : un moment phare, le **lancement d'un jeu** ; ailleurs, uniquement de la continuité et des retours d'action.

| Élément | Ce qui a changé | Principe |
|---|---|---|
| Transition d'écran | Un seul fondu : sortie en 160 ms, entrée en 420 ms en montant de 8 px ; plus de glissement latéral ni de cascade simultanée | une seule chose à suivre |
| Focus d'une tuile | La tuile se soulève de 4 px ; son ombre descend et s'adoucit ; l'anneau se pose (de 95 % à 100 %) ; le titre suit 50 ms après, les pastilles 70 ms après | contre-mouvement, suivi décalé |
| Appui A | Enfoncement à 95 %, léger dépassement à 103,5 %, retour au repos (280 ms) | anticipation, rebond doux |
| Bord atteint | Oscillation amortie (6, −3, 1,5 px), sans son | |
| Lancement (moment phare) | Les autres tuiles s'estompent (flou de 1,5 px) ; la tuile choisie se soulève, puis vole en arc jusqu'à l'écran de passage (FLIP, 560 ms) ; le panneau arrive après l'atterrissage ; au lancement, un reflet traverse la tuile pendant la montée des 4 notes, et le panneau s'efface | mise en scène, arcs, action secondaire |
| Étapes cochées | La pastille grandit avec un dépassement, puis le trait se dessine ; reflet régulier sur la barre de progression | état de réussite |
| Bibliothèque | Indicateur d'onglet qui glisse d'un onglet à l'autre ; la grille arrive du côté de l'onglet choisi | continuité |
| Qui joue ? | Onde de la couleur du joueur depuis l'avatar choisi | action secondaire |
| Conflit | Chaque carte arrive de son côté ; pastille cochée « pop » sur la carte choisie | |
| Mises à jour | La ligne réussie s'éclaire brièvement en vert | |
| Barre d'aide | La touche dessinée s'enfonce quand le bouton est appuyé | l'interface répond |

Mode « animations réduites » : plus aucun déplacement ni changement d'échelle ; les fondus de couleur et les états restent.

## Direction de couleur C « Ambiance » (validée par Guilhem le 15/11)

Maquette : https://claude.ai/artifact/NSBRvReHdaGeZR1KapYfxh. Les couleurs ternes sont proscrites.

| Élément | Règle | Mouvement |
|---|---|---|
| Fond | Prend la couleur de la jaquette sélectionnée (`Ambiance.svelte`) ; la lumière se place au-dessus de la tuile | Fondu enchaîné de 420 ms d'un jeu à l'autre ; la même ambiance suit le jeu jusqu'à l'écran de passage et à la fiche |
| Jaquettes | Scène illustrée en dégradé vif, motif original (`CoverArt.svelte`), titre en bas à gauche, décor à droite | Au focus, parallaxe : le décor glisse de 1 px, le premier plan de 3 px avec un léger zoom |
| Pastille console | Monochrome blanche sur fond sombre translucide (comme Cocoon, ES-DE) ; jamais le rouge Switch | — |
| En-tête | Profil (avatar, nom, appareil) en pastille translucide ; à droite, nuage de synchronisation global façon Steam (`SyncStatus.svelte`) : état le plus prioritaire de toutes les sauvegardes du profil (Hors ligne > Conflit > Synchronisation > Envoi en attente > À jour) | Nouveau nuage avec petit rebond ; flèches qui tournent pendant la synchronisation ; nuage qui respire en attente d'envoi |
| Synchronisation manuelle | Le nuage de l'en-tête est un bouton (haut depuis les tuiles) ; la barre d'aide affiche « A Synchroniser » ; commande `sync` (repli démo) | Focus : la pastille se soulève, anneau d'accent ; pendant l'envoi, « Synchronisation… » : nuage immobile qui respire et arc de chargement qui tourne autour ; son `syncStart` (deux notes qui montent) puis `syncTick` (une goutte très douce, même note, toutes les 0,9 s) tant que ça charge ; à la fin, son `saved` (ou `conflict` s'il reste un conflit) |
| Images de profil | Personnage illustré sur la couleur du profil ; cette couleur est l'accent de l'interface (Guilhem #38B6FF, Claire #34E0A1) | Clignement des yeux rare (toutes les 5,5 s) |

## Options, Profil, restauration (15/11)

| Écran | Contenu | Mouvement, son |
|---|---|---|
| Options d'un jeu (X sur une tuile) | Panneau à droite : Jouer ou Installer, Détails et sauvegardes, Synchroniser ce jeu, Émulateur (Switch : Ryujinx ou Eden), Désinstaller (demande confirmation). Le focus reste dans le panneau, B le referme et rend le focus à la tuile | Le panneau glisse de 60 px en 320 ms, fond flouté ; l'option en focus se décale de 4 px ; synchronisation avec les sons `syncStart` et `syncTick` |
| Profil (pastille du profil dans l'en-tête) | Image de profil (6 personnages ; la couleur suit), nom (saisie, Entrée pour valider, Échap pour annuler), Sons, Vibrations, Changer de joueur. Enregistrement immédiat, « Enregistré » s'affiche | Nouvelle image : le grand avatar arrive avec un rebond, l'accent et l'ambiance passent en fondu ; son `arrive` ; interrupteurs avec rebond |
| Restauration d'une sauvegarde (fiche) | Fenêtre de chargement : Téléchargement de la version, Remplacement de la sauvegarde locale, Sauvegarde restaurée ; barre continue ; B est bloqué pendant l'opération | Coches avec rebond ; sons `syncStart`, puis `syncTick` toutes les 0,9 s, puis `saved` à la fin |

## Réglages et Options de la Bibliothèque (16/11)

| Écran | Contenu | Mouvement, son |
|---|---|---|
| Réglages (raccourci de l'accueil) | Son (volume 0 à 10), Serveur (adresse, Tester la connexion), Synchronisation (au démarrage, à la fin de chaque partie), Appareil (nom, espace libre), À propos. Enregistrés sur l'appareil | Volume : gauche / droite règlent (attribut `data-adjust`), aperçu sonore `move` à chaque cran, `err` en butée ; test de connexion avec `CloudIcon` (arc qui tourne autour du nuage), `syncStart`, `syncTick`, puis `saved` ou `err` |
| Bibliothèque | X sur une tuile ouvre le même menu Options qu'à l'accueil | Identique à l'accueil |
| Chargements nuage | `CloudIcon.svelte` partout : le nuage reste immobile et respire, l'arc tourne autour ; jamais le nuage entier qui tourne | — |

## Rangée de l'accueil (17/11) — mouvement par type d'entrée

Maquette interactive (2 chorégraphies, A retenue) : https://claude.ai/artifact/QCxawWGTmL8wtY12YpLqCd

Principe : la mise en page de la rangée ne bouge jamais (emplacements de 232 px + 20 px). La tuile choisie est agrandie (×272/232, ancrée à gauche) et les emplacements suivants glissent de 40 px, par transformations, sur la même durée et la même courbe : l'écart de 20 px reste constant pendant le mouvement. Le défilement est un ressort amorti critique (`src/lib/glide.ts`, ω = 20 : 90 % du trajet en 195 ms) qui garde sa vitesse quand la cible change.

| Entrée | Ce qui se passe |
|---|---|
| Flèche, croix, stick (un appui) | La tuile choisie grandit (260 ms, `--ease-land`) et se place en 2e position ; son `move` |
| Maintien (clavier, croix, stick) | 1 pas toutes les 110 ms (croix et stick : 1re répétition après 380 ms ; clavier : délai du système). Racine en `data-rush` : tuile en 150 ms, titre remplacé sans montée, ligne de l'appareil masquée, fond figé. La rangée suit avec environ une tuile de retard, sans à-coup ; au relâchement, tout se pose |
| Bord atteint | Une seule oscillation au premier appui, rien en maintenant |
| Survol à la souris | Choisit la tuile, sans défiler (seule une tuile coupée au bord est révélée). Un contenu qui défile sous un pointeur immobile ne change jamais le choix |
| Molette à crans | La rangée avance d'une tuile par cran, alignée sur les tuiles ; le choix ne bouge pas |
| Pavé tactile | Défilement horizontal natif ; un défilement vertical est converti, direct, sans ressort |
| Doigt | Glisser avec l'inertie native (le ressort se retire). 1er toucher : choisit ; 2e : ouvre. Un toucher qui arrête l'inertie (`scrollend`, sinon 200 ms) ne choisit rien |
| Clic | Ouvre la tuile cliquée (déjà choisie par le survol) |
| Retour d'écran | Le choix revient sur la tuile quittée ; la rangée est replacée sans glisser |
| Titre sous la rangée | L'ancien s'efface (100 ms), puis le nouveau monte de 8 px (240 ms) : jamais deux titres superposés |
| Fond d'ambiance | Suit 140 ms après le dernier changement : la tuile répond d'abord, le monde suit |
| Animations réduites | Tuile, décalage et défilement instantanés ; sons et vibrations restent |
