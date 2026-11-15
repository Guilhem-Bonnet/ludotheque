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
| En-tête | Profil (avatar, nom, appareil) et « À jour » dans des pastilles translucides | — |
| Images de profil | Personnage illustré sur la couleur du profil ; cette couleur est l'accent de l'interface (Guilhem #38B6FF, Claire #34E0A1) | Clignement des yeux rare (toutes les 5,5 s) |
