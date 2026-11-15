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
