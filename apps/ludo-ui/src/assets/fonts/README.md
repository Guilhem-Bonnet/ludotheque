# Polices des bandeaux de cartouche

Sous-ensembles (A-Z, 0-9, espace) de polices libres SIL OFL 1.1, figées à une graisse et embarquées
dans l'app : aucun chargement externe à l'exécution. Elles n'écrivent que le nom de la console dans
le bandeau de la tuile (`Tile.svelte`). Aucune police Nintendo n'est reproduite : ce sont des polices
libres choisies pour évoquer l'esprit de chaque console.

| Fichier | Console | Police d'origine | Réglage | Licence |
|---|---|---|---|---|
| cart-switch.woff2 | Switch | Nunito (paquet @fontsource-variable/nunito 5.3.0) | wght 800 | OFL-nunito.txt |
| cart-switch2.woff2 | Switch 2 | Saira (@fontsource-variable/saira 5.3.0) | wght 700, wdth 100 | OFL-saira.txt |
| cart-3ds.woff2 | 3DS | Fredoka (@fontsource-variable/fredoka 5.3.0) | wght 600 | OFL-fredoka.txt |
| cart-ds.woff2 | DS | Oxanium (@fontsource-variable/oxanium 5.3.0) | wght 700 | OFL-oxanium.txt |
| cart-gc.woff2 | GameCube | Baloo 2 (@fontsource-variable/baloo-2 5.3.0) | wght 800 | OFL-baloo-2.txt |
| cart-wiiu.woff2 | Wii U | Varela Round (@fontsource/varela-round 5.3.0) | 400 | OFL-varela-round.txt |

Régénérer : script `subset_fonts.py` (fontTools, `varLib.instancer` puis `subset`) consigné dans les
preuves du lot cartouches (`_grimoire-output/evidence/GAO-ludo-cartou-001/fonts/`).
Les polices OFL exigent que la licence accompagne le fichier : les `OFL-*.txt` restent à côté.
