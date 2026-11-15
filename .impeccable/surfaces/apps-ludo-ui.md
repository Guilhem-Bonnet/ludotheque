---
version: 1
slug: "apps-ludo-ui"
primary_target: "apps/ludo-ui"
related_targets: []
---

Scope : interface de la Ludothèque (Tauri 2) — accueil, Qui joue ?, écran de passage, conflit, Bibliothèque, fiche jeu, mises à jour. Mode : Operate. Usage à la manette sur Steam Deck (1280×800), PC/TV via Moonlight (1920×1080), Ayn Thor.

## Direction contract

THESIS : un écran d'accueil de console où « qui joue » passe en premier et où chaque tuile sait sur quel appareil elle a été jouée en dernier ; refuse l'étagère de jaquettes à la Steam avec grande bannière.

OWN-WORLD : ardoise sombre bleutée (#15171C, surfaces #1F222A / #2A2E38), texte #EEF0F4 ; l'accent est celui du profil actif (Guilhem cyan #4CC3FF, Claire corail #FF8A5C) et teinte focus, sélection et barre ; états : vert, bleu, ambre, violet (jamais rouge), gris ; pastille couleur par console ; Rubik 400/500/700, chiffres tabulaires ; tuiles 16 px de rayon.

STORY : choisir son avatar, voir ses jeux récents, la tuile en focus dit appareil + état de sauvegarde, A pour Jouer ; le reste (bibliothèque, mises à jour, profils) en ronds de raccourci.

FIRST VIEWPORT : barre haute (avatar actif + état serveur + heure + batterie), rangée horizontale de grandes tuiles (la première en focus, agrandie, anneau d'accent), sous elle titre + onglet d'appareil, rangée de ronds de raccourci, barre d'aide des boutons.

SIGNATURE : l'onglet d'appareil — sous la tuile en focus glisse une languette « PC · Hier 22:40 » avec icône d'appareil et glyphe de synchro.

FORM : accueil façon Switch, candidat 1 (choix « pick »), seed:ae8f756f6b13db9c. Relève du challenger « catalogue de personnages » (compétitif) : reflow par cellules entières et un seul rebond de focus.
