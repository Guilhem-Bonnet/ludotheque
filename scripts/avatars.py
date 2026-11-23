#!/usr/bin/env python3
"""Galerie des images de profil de la Ludothèque, depuis des bibliothèques publiques d'images.

Aucune image n'est dessinée ni découpée ici : le script reprend des images existantes, telles quelles
(redimensionnées à 256 px au plus) :
- icônes de profil officielles de la Nintendo Switch Online (pièces « personnage » détourées, fond
  transparent, 256 px), archivées par le projet lhub-data (GitLab), sélection figée dans LHUB_CHOIX ;
- sprites animés des Pokémon (PokeAPI/sprites, dossier « showdown », GIF) ;
- icônes SteamGridDB de personnages pour les licences absentes des icônes Switch (Layton, Kid Icarus,
  Yo-kai Watch…), choisies à l'œil dans SGDB_CHOIX. Clé : `~/.config/ludotheque/steamgriddb.key` (jamais affichée).

À lancer sur le PC (le serveur n'a pas d'accès Internet). Essai à blanc par défaut : télécharge dans le
cache local, écrit une planche-contact et le tableau de la sélection, n'écrit rien d'autre.
    scripts/avatars.py                          # essai à blanc (cache + planche-contact)
    scripts/avatars.py --galerie apps/ludo-ui/src/lib/avatars-galerie.json   # écrit le catalogue de l'app
    scripts/avatars.py --envoyer                # envoie les images absentes du serveur (PUT /api/v1/avatars/…)

Idempotent : chaque image est téléchargée une fois (cache), l'identifiant d'une image (slug) dérive de
sa source et de son nom, et une image déjà présente sur le serveur n'est pas renvoyée (sauf `--forcer`).
Requêtes une par une avec une pause, User-Agent générique, rien de personnel n'est transmis.
Une image animée part avec sa version fixe (première image, PNG) : l'app n'anime qu'au focus.
"""
import argparse
import colorsys
import json
import os
import re
import sys
import time
import unicodedata
import urllib.error
import urllib.parse
import urllib.request

try:
    from PIL import Image, ImageSequence
except ImportError:  # pragma: no cover
    sys.exit("Pillow est nécessaire (python3-pillow).")

UA = "LudothequeAvatars/1.0"
PAUSE_S = 0.35
TAILLE = 256
CLE_SGDB = os.path.expanduser("~/.config/ludotheque/steamgriddb.key")
SGDB = "https://www.steamgriddb.com/api/v2"
POKEAPI = "https://raw.githubusercontent.com/PokeAPI/sprites/master/sprites/pokemon/other/showdown"

# Icônes de profil officielles de la Nintendo Switch (Nintendo Switch Online), pièces « personnage »
# détourées (256 px, fond transparent), archivées par le projet lhub-data (GitLab). Sélection faite à l'œil
# sur des planches-contact le 22/11/2026 : un personnage bien visible, pas d'objet ni de décor.
# Clé : catégorie lhub (jeu) ; valeur : (numéro de l'icône, nom affiché, famille).
LHUB = "https://gitlab.fancy.org.uk/api/v4/projects/samuel%2Flhub-data/repository/files/"
LHUB_CHOIX = {
    "super-mario-bros-wonder": [(5248, "Mario", "Mario"), (5249, "Peach", "Mario"), (5276, "Luigi", "Mario"),
                                (5278, "Yoshi", "Mario"), (5250, "Toad", "Mario"), (5277, "Toadette", "Mario"),
                                (5301, "Daisy", "Mario"), (5303, "Carottin", "Mario"), (5281, "Bowser", "Mario"),
                                (5273, "Mario éléphant", "Mario")],
    "princess-peach": [(5848, "Peach", "Mario"), (5850, "Peach épéiste", "Mario"), (5886, "Peach détective", "Mario"),
                       (5849, "Stella", "Mario")],
    "luigis-mansion-2-hd": [(6163, "Luigi", "Mario"), (6164, "Luigi et sa lampe", "Mario"), (6202, "Boo", "Mario"),
                            (6188, "Professeur K. Tastroff", "Mario")],
    "donkey-kong-country-returns-hd": [(6880, "Donkey Kong", "Donkey Kong"), (6881, "Diddy Kong", "Donkey Kong"),
                                       (6913, "Donkey et Diddy", "Donkey Kong"), (6892, "Cranky Kong", "Donkey Kong"),
                                       (6903, "Rambi", "Donkey Kong")],
    "the-legend-of-zelda-tears-of-the-kingdom": [(4802, "Zelda", "Zelda"), (4335, "Link", "Zelda"),
                                                 (4313, "Link et l'Emprise", "Zelda"), (4804, "Pru'ha", "Zelda"),
                                                 (4806, "Robelo", "Zelda"), (4821, "Tulin", "Zelda"),
                                                 (4836, "Prince Sidon", "Zelda"), (4838, "Yona", "Zelda")],
    "the-legend-of-zelda-echoes-of-wisdom": [(6499, "Zelda", "Zelda"), (6497, "Zelda et le bâton", "Zelda"),
                                             (6500, "Link", "Zelda"), (6498, "Tri", "Zelda"), (6537, "Octorok", "Zelda")],
    "pokemon-scarlet-and-violet": [(3479, "Pikachu", "Pokémon"), (3480, "Évoli", "Pokémon"),
                                   (3247, "Poussacha", "Pokémon"), (3246, "Chochodile", "Pokémon"),
                                   (3248, "Coiffeton", "Pokémon"), (6869, "Koraidon", "Pokémon"),
                                   (6870, "Miraidon", "Pokémon")],
    "detective-pikachu-returns": [(4953, "Détective Pikachu", "Pokémon"), (4954, "Détective Pikachu à la loupe", "Pokémon")],
    "kirby-and-the-forgotten-land": [(1599, "Kirby", "Kirby"), (1602, "Kirby épée", "Kirby"),
                                     (1600, "Bandana Waddle Dee", "Kirby"), (1620, "Meta Knight", "Kirby"),
                                     (1640, "Roi DaDiDou", "Kirby"), (1660, "Elfilin", "Kirby")],
    "kirbys-return-to-dream-land-deluxe": [(3868, "Kirby rêveur", "Kirby"), (3870, "Magolor", "Kirby")],
    "animal-crossing-new-horizons": [(4022, "Marie", "Animal Crossing"), (3975, "Tom Nook", "Animal Crossing"),
                                     (3977, "Méli et Mélo", "Animal Crossing"), (3978, "Kéké Laglisse", "Animal Crossing"),
                                     (4245, "Thibou", "Animal Crossing"), (4246, "Céleste", "Animal Crossing"),
                                     (4286, "Porcelette", "Animal Crossing"), (4259, "Rounard", "Animal Crossing"),
                                     (4271, "Résetti", "Animal Crossing"), (4256, "Pascal", "Animal Crossing"),
                                     (4260, "Kapp'n", "Animal Crossing"), (2851, "Raymond", "Animal Crossing"),
                                     (2396, "Marshal", "Animal Crossing"), (814, "Judy", "Animal Crossing"),
                                     (3319, "Sherb", "Animal Crossing"), (2372, "Ankha", "Animal Crossing")],
    "splatoon-3": [(2763, "Inkling", "Splatoon"), (2802, "Inkling souriante", "Splatoon"), (2764, "Octaling", "Splatoon"),
                   (2803, "Octaling en garde", "Splatoon"), (2587, "Calamar", "Splatoon"), (2588, "Poulpe", "Splatoon"),
                   (2836, "Judd", "Splatoon")],
    "metroid-dread": [(6038, "Samus", "Metroid"), (6074, "Samus en action", "Metroid")],
    "metroid-prime-remastered": [(4003, "Samus Varia", "Metroid"), (3987, "Metroid", "Metroid")],
    "fire-emblem-engage": [(3665, "Alear", "Autres héros"), (3649, "Marth", "Autres héros"), (3644, "Roy", "Autres héros"),
                           (3656, "Ike", "Autres héros"), (3683, "Sommie", "Autres héros")],
    "xenoblade3": [(2143, "Noah", "Autres héros"), (2252, "Mio", "Autres héros"), (2260, "Manana", "Autres héros")],
    "pikmin-series": [(4783, "Pikmin", "Autres héros"), (4770, "Olimar", "Autres héros"), (4748, "Otchin", "Autres héros")],
}

# Pokémon des jeux de la collection, sprites animés PokeAPI (numéro national, nom français).
POKEMON_ANIMES = [
    (25, "Pikachu"), (133, "Évoli"), (1, "Bulbizarre"), (4, "Salamèche"), (7, "Carapuce"), (6, "Dracaufeu"),
    (3, "Florizarre"), (9, "Tortank"), (150, "Mewtwo"), (151, "Mew"), (143, "Ronflex"), (39, "Rondoudou"),
    (94, "Ectoplasma"), (448, "Lucario"), (658, "Amphinobi"), (722, "Brindibou"), (725, "Flamiaou"),
    (728, "Otaquin"), (791, "Solgaleo"), (792, "Lunala"), (716, "Xerneas"), (717, "Yveltal"),
    (382, "Kyogre"), (383, "Groudon"), (483, "Dialga"), (54, "Psykokwak"),
]

# Licences absentes des icônes Switch : icônes SteamGridDB de personnages, choisies à l'œil
# (identifiant de l'icône, nom affiché, famille). Les logos et titres sont écartés.
SGDB_CHOIX = [
    ("The Legend of Zelda: Breath of the Wild", 71878, "Link de Breath of the Wild", "Zelda"),
    ("The Legend of Zelda: A Link Between Worlds", 430, "Link de A Link Between Worlds", "Zelda"),
    ("Captain Toad: Treasure Tracker", 2400, "Captain Toad", "Mario"),
    ("Kid Icarus: Uprising", 4474, "Pit", "Autres héros"),
    ("Yo-kai Watch", 4493, "Whisper", "Autres héros"),
    ("Shovel Knight", 48166, "Shovel Knight", "Autres héros"),
    ("Professor Layton and the Miracle Mask", 118891, "Professeur Layton", "Autres héros"),
]

# Ordre des familles dans l'écran de choix.
FAMILLES = ["Mario", "Donkey Kong", "Zelda", "Pokémon", "Kirby", "Animal Crossing", "Splatoon", "Metroid", "Autres héros"]


def log(msg):
    print(msg, file=sys.stderr, flush=True)


def requete(url, methode="GET", data=None, entetes=None, delai=60):
    req = urllib.request.Request(url, data=data, method=methode, headers={"User-Agent": UA, **(entetes or {})})
    return urllib.request.urlopen(req, timeout=delai)


def lire_json(url, entetes=None):
    try:
        with requete(url, entetes=entetes) as r:
            return json.load(r)
    finally:
        time.sleep(PAUSE_S)


def slugifier(s):
    s = unicodedata.normalize("NFKD", s).encode("ascii", "ignore").decode().lower()
    return re.sub(r"[^a-z0-9]+", "-", s).strip("-")


def slug_unique(base, pris):
    base = base[:40].strip("-") or "image"
    s, n = base, 2
    while s in pris:
        suffixe = f"-{n}"
        s, n = base[: 40 - len(suffixe)] + suffixe, n + 1
    pris.add(s)
    return s


# --- Couleur d'accent ------------------------------------------------------------------------

def hex_rgb(r, g, b):
    return "#{:02x}{:02x}{:02x}".format(int(r), int(g), int(b))


def couleurs(img):
    """Accent (teinte vive dominante de l'image, pixels opaques), fond doux et encre, comme les
    couleurs des personnages maison : l'accent tient sur fond sombre, le fond doux est très sombre."""
    petite = img.convert("RGBA").resize((48, 48))
    seaux = {}
    pixels = petite.get_flattened_data() if hasattr(petite, "get_flattened_data") else petite.getdata()
    for r, g, b, a in pixels:
        if a < 160:
            continue
        h, s, v = colorsys.rgb_to_hsv(r / 255, g / 255, b / 255)
        if s < 0.35 or v < 0.35:
            continue
        cle = int(h * 24) % 24
        poids = s * v
        t = seaux.setdefault(cle, [0.0, 0.0, 0.0, 0.0])
        t[0] += poids
        t[1] += h * poids
        t[2] += s * poids
        t[3] += v * poids
    if not seaux:
        h, s, v = 0.58, 0.55, 0.95  # image sans couleur vive : bleu doux par défaut
    else:
        p, sh, ss, sv = max(seaux.values(), key=lambda t: t[0])
        h, s, v = sh / p, ss / p, sv / p
    s, v = min(max(s, 0.55), 0.85), max(v, 0.88)
    accent = colorsys.hsv_to_rgb(h, s, v)
    soft = colorsys.hsv_to_rgb(h, 0.75, 0.22)
    ink = colorsys.hsv_to_rgb(h, 0.38, 1.0)
    return tuple(hex_rgb(*(c * 255 for c in x)) for x in (accent, soft, ink))


# --- Téléchargement et préparation ----------------------------------------------------------

def telecharger(url, dest, entetes=None):
    if os.path.exists(dest) and os.path.getsize(dest) > 0:
        return True
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    try:
        with requete(url, entetes=entetes) as r:
            data = r.read()
    except (urllib.error.URLError, TimeoutError, OSError) as e:
        log(f"  échec {url} : {e}")
        return False
    finally:
        time.sleep(PAUSE_S)
    if not data:
        return False
    with open(dest + ".part", "wb") as f:
        f.write(data)
    os.replace(dest + ".part", dest)
    return True


def preparer(brut, sortie):
    """Image prête pour le serveur : PNG carré de 256 px au plus (fixe), ou GIF d'origine (animé) et sa
    première image en PNG. Renvoie (chemin de l'image, chemin de la version fixe ou None, animée, couleurs)."""
    img = Image.open(brut)
    anime = getattr(img, "is_animated", False) and img.format == "GIF"
    os.makedirs(os.path.dirname(sortie), exist_ok=True)
    premiere = next(iter(ImageSequence.Iterator(img))).convert("RGBA")
    if anime:
        fixe = sortie + ".fixe.png"
        if not os.path.exists(fixe):
            premiere.save(fixe, optimize=True)
        return brut, fixe, True, couleurs(premiere)
    png = sortie + ".png"
    if not os.path.exists(png):
        carre = premiere
        w, h = carre.size
        if w != h:  # centre sur un carré transparent, sans rien couper
            c = max(w, h)
            fond = Image.new("RGBA", (c, c), (0, 0, 0, 0))
            fond.paste(carre, ((c - w) // 2, (c - h) // 2))
            carre = fond
        if carre.size[0] > TAILLE:
            carre = carre.resize((TAILLE, TAILLE), Image.LANCZOS)
        carre.save(png, optimize=True)
    return png, None, False, couleurs(Image.open(png))


# --- Sources ---------------------------------------------------------------------------------

def sgdb_entetes():
    try:
        with open(CLE_SGDB) as f:
            return {"Authorization": "Bearer " + f.read().strip()}
    except OSError:
        return None


def sgdb_url(titre, icone_id, entetes):
    """Adresse d'une icône SteamGridDB choisie : recherche du jeu, puis de l'icône parmi les siennes."""
    jeux = lire_json(f"{SGDB}/search/autocomplete/{urllib.parse.quote(titre)}", entetes).get("data") or []
    for jeu in jeux[:3]:
        for ic in lire_json(f"{SGDB}/icons/game/{jeu['id']}?types=static&nsfw=false&humor=false", entetes).get("data") or []:
            if ic.get("id") == icone_id:
                return ic["url"]
    return None


# --- Programme -------------------------------------------------------------------------------

def collecter(cache, sans_sgdb):
    """Liste des images retenues : dict(slug, label, famille, source, url, brut). Ordre fixe : les slugs
    (identifiants des images sur le serveur et dans les profils) restent les mêmes d'un passage à l'autre."""
    pris, choix = set(), []

    log("lhub-data — icônes Nintendo Switch Online")
    for categorie, icones in LHUB_CHOIX.items():
        for num, label, famille in icones:
            gid = f"{categorie}_{num}"
            url = LHUB + urllib.parse.quote(f"assets/gifts/{gid}.webp", safe="") + "/raw?ref=main"
            choix.append({"slug": slug_unique(f"{slugifier(famille)}-{slugifier(label)}", pris), "label": label,
                          "famille": famille, "source": f"lhub-data (icône Nintendo Switch Online) {gid}", "url": url,
                          "brut": os.path.join(cache, "brut", "lhub", f"{gid}.webp")})

    log("PokeAPI — sprites animés")
    for num, nom in POKEMON_ANIMES:
        choix.append({"slug": slug_unique(f"pokemon-anime-{slugifier(nom)}", pris), "label": nom, "famille": "Pokémon",
                      "source": f"PokeAPI/sprites showdown {num}.gif", "url": f"{POKEAPI}/{num}.gif",
                      "brut": os.path.join(cache, "brut", "pokeapi", f"{num}.gif")})

    # Icônes déjà en cache : gardées même sans clé (une image retirée de la galerie rendrait illisible
    # le choix d'un profil) ; la clé ne sert qu'à télécharger celles qui manquent.
    entetes = None if sans_sgdb else sgdb_entetes()
    log("SteamGridDB — icônes de personnages choisies" + ("" if entetes else " (cache seulement)"))
    for titre, icone_id, label, famille in SGDB_CHOIX:
        brut = os.path.join(cache, "brut", "steamgriddb", f"{icone_id}.png")
        url = None
        if not os.path.exists(brut):
            if not entetes:
                log(f"  icône SteamGridDB {icone_id} ({titre}) absente du cache et pas de clé")
                continue
            try:
                url = sgdb_url(titre, icone_id, entetes)
            except (urllib.error.URLError, TimeoutError, OSError, ValueError) as e:
                log(f"  {titre} : {e}")
            if not url:
                log(f"  icône SteamGridDB {icone_id} ({titre}) introuvable")
                continue
        choix.append({"slug": slug_unique(f"{slugifier(famille)}-{slugifier(label)}", pris), "label": label, "famille": famille,
                      "source": f"SteamGridDB icône {icone_id} ({titre})", "url": url or "", "brut": brut})
    return choix


def preparer_tout(choix, cache):
    prets = []
    for c in choix:
        if not telecharger(c["url"], c["brut"]):
            continue
        try:
            image, fixe, anime, (accent, soft, ink) = preparer(c["brut"], os.path.join(cache, "pret", c["slug"]))
        except (OSError, ValueError) as e:
            log(f"  {c['slug']} : image illisible ({e})")
            continue
        prets.append({**c, "image": image, "fixe": fixe, "anime": anime, "accent": accent, "soft": soft, "ink": ink})
    return prets


def planche(prets, chemin):
    """Planche-contact des images retenues, sur leur couleur d'accent (relecture à l'œil)."""
    cote, col = 96, 16
    lignes = (len(prets) + col - 1) // col
    feuille = Image.new("RGB", (col * cote, lignes * cote), (22, 26, 34))
    for i, p in enumerate(prets):
        disque = Image.new("RGBA", (cote - 8, cote - 8), p["accent"])
        im = Image.open(p["fixe"] or p["image"]).convert("RGBA")
        im.thumbnail((cote - 8, cote - 8))
        disque.alpha_composite(im, ((cote - 8 - im.size[0]) // 2, (cote - 8 - im.size[1]) // 2))
        feuille.paste(disque.convert("RGB"), ((i % col) * cote + 4, (i // col) * cote + 4))
    feuille.save(chemin)


def ecrire_galerie(prets, chemin):
    """Écrit le catalogue de l'app ; refuse si une image déjà publiée disparaîtrait (choix de profil illisible)."""
    if os.path.exists(chemin):
        with open(chemin, encoding="utf-8") as f:
            avant = {i["slug"] for i in json.load(f).get("images", [])}
        perdus = sorted(avant - {p["slug"] for p in prets})
        if perdus:
            sys.exit(f"Galerie non écrite : {len(perdus)} image(s) disparaîtrai(en)t ({', '.join(perdus[:8])}…). "
                     "Source indisponible ? Relancer, ou retirer ces images à la main du fichier.")
    ordre = {f: i for i, f in enumerate(FAMILLES)}
    # Ordre de la sélection dans chaque famille (personnage principal d'abord), images animées après.
    lignes = sorted(prets, key=lambda p: (ordre.get(p["famille"], 99), p["anime"]))
    entrees = [{"slug": p["slug"], "label": p["label"], "famille": p["famille"], "anime": p["anime"], "accent": p["accent"],
                "soft": p["soft"], "ink": p["ink"], "source": p["source"]} for p in lignes]
    with open(chemin, "w", encoding="utf-8") as f:
        f.write(json.dumps({"genere_par": "scripts/avatars.py", "familles": FAMILLES, "images": entrees},
                           ensure_ascii=False, indent=1) + "\n")


def present(serveur, chemin):
    """Image déjà sur le serveur ? Erreur réseau = on tente l'envoi (le PUT dira ce qu'il en est)."""
    try:
        with requete(serveur + chemin, "HEAD"):
            return True
    except (urllib.error.URLError, TimeoutError, OSError):
        return False


def envoyer(serveur, prets, forcer):
    serveur = serveur.rstrip("/")
    envoyes = deja = echecs = 0
    for p in prets:
        parties = [("", p["image"])] + ([("/fixe", p["fixe"])] if p["fixe"] else [])
        for suffixe, fichier in parties:
            route = f"/api/v1/avatars/{p['slug']}{suffixe}"
            if not forcer and present(serveur, route):
                deja += 1
                continue
            with open(fichier, "rb") as f:
                data = f.read()
            try:
                with requete(serveur + route, "PUT", data, {"Content-Type": "application/octet-stream"}):
                    envoyes += 1
            except urllib.error.HTTPError as e:
                echecs += 1
                log(f"  PUT {route} refusé ({e.code})")
                if e.code in (404, 405) and envoyes == 0:
                    log("Le serveur ne connaît pas encore /api/v1/avatars (version trop ancienne) : arrêt.")
                    print(f"images : {envoyes} envoyées, {deja} déjà présentes, {echecs} en échec")
                    return 1
            except (urllib.error.URLError, TimeoutError, OSError) as e:
                echecs += 1
                log(f"  PUT {route} : {e}")
    print(f"images : {envoyes} envoyées, {deja} déjà présentes, {echecs} en échec")
    return 1 if echecs else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--serveur", default=os.environ.get("LUDO_SERVEUR", "https://ludotheque.srvdreamer.fr"))
    ap.add_argument("--cache", default=os.path.expanduser("~/.cache/ludotheque-avatars"))
    ap.add_argument("--galerie", metavar="FICHIER", help="écrit le catalogue de la galerie (avatars-galerie.json de l'app)")
    ap.add_argument("--planche", metavar="PNG", help="planche-contact (défaut : <cache>/planche.png)")
    ap.add_argument("--envoyer", action="store_true", help="envoie les images au serveur (PUT)")
    ap.add_argument("--forcer", action="store_true", help="renvoie aussi les images déjà présentes")
    ap.add_argument("--sans-steamgriddb", action="store_true", help="n'interroge pas SteamGridDB")
    a = ap.parse_args()

    choix = collecter(a.cache, a.sans_steamgriddb)
    prets = preparer_tout(choix, a.cache)
    print(f"{'famille':16} {'slug':42} {'animée':6} {'accent':8} source")
    for p in prets:
        print(f"{p['famille']:16} {p['slug']:42} {'oui' if p['anime'] else '':6} {p['accent']:8} {p['source'][:70]}")
    par = {}
    for p in prets:
        par[p["famille"]] = par.get(p["famille"], 0) + 1
    print("\nRésumé : " + ", ".join(f"{f} {n}" for f, n in par.items()) + f" — {len(prets)} images")
    chemin_planche = a.planche or os.path.join(a.cache, "planche.png")
    planche(prets, chemin_planche)
    print(f"Planche-contact : {chemin_planche}")
    if a.galerie:
        ecrire_galerie(prets, a.galerie)
        print(f"Galerie écrite : {a.galerie}")
    if a.envoyer:
        return envoyer(a.serveur, prets, a.forcer)
    return 0


if __name__ == "__main__":
    sys.exit(main())
