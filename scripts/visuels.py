#!/usr/bin/env python3
"""Visuels de la ludothèque : noms français et images des jeux, depuis des bases publiques.

À lancer sur le PC (le serveur n'a volontairement aucun accès Internet) :
- Switch : blawar/titledb (FR.fr.json) : nom français, icône officielle, par identifiant de titre ;
- 3DS, DS, GameCube, Wii U : libretro-thumbnails (Named_Boxarts), retrouvées par le nom du fichier ROM.
Deux images par jeu : la jaquette (`PUT /api/v1/games/{id}/cover`) et le fond grand format
(`PUT /api/v1/games/{id}/background`, serveur >= 0.4.0) : bannière titledb (Switch), capture
d'écran libretro Named_Snaps (3DS, DS…). Images choisies à la main (jeux sans base publique, comme
la Switch 2) : `<manuel>/<id>.cover.(jpg|png|webp)` et `<manuel>/<id>.background.(…)`, prioritaires.
Les images sont téléchargées UNE fois dans un dossier de cache local, puis envoyées au serveur,
qui les garde dans son volume de données.

Usage (bibliothèque standard seulement) :
    scripts/visuels.py                          # essai à blanc : tableau des correspondances, rien n'est écrit
    scripts/visuels.py --catalogue out.toml     # écrit aussi le catalogue.toml (titres, titre court, title_id)
    scripts/visuels.py --envoyer-images         # envoie les images manquantes (serveur >= 0.3.0)
    scripts/visuels.py --rescan                 # POST /catalog/rescan (après dépôt de catalogue.toml)

Idempotent : les identifiants de jeu et les chemins d'entrée sont lus sur le serveur et conservés tels
quels (les sauvegardes sont indexées par identifiant) ; une image déjà présente sur le serveur n'est pas
renvoyée (sauf `--forcer`). Téléchargements un par un, User-Agent « Ludotheque/0.2 », rien de personnel
n'est transmis aux bases publiques. L'essai à blanc télécharge les images dans le cache (c'est le seul
effet de bord) ; `--sans-telechargement` s'en abstient.
"""
import argparse
import difflib
import hashlib
import html.parser
import json
import os
import re
import sys
import time
import unicodedata
import urllib.error
import urllib.parse
import urllib.request

UA = "Ludotheque/0.2"
TITLEDB = "https://raw.githubusercontent.com/blawar/titledb/master/FR.fr.json"
LIBRETRO = "https://thumbnails.libretro.com"
SYSTEMES = {
    "3ds": "Nintendo - Nintendo 3DS",
    "ds": "Nintendo - Nintendo DS",
    "gc": "Nintendo - GameCube",
    "wiiu": "Nintendo - Wii U",
}
MAX_IMAGE = 5 * 1024 * 1024
PAUSE_S = 0.3

# Noms français officiels, quand le nom du fichier (anglais, No-Intro) ne suffit pas.
# Clé : titre du fichier normalisé par `norm_mots` (minuscules, sans accents ni apostrophes).
NOMS_FR = {
    "resident evil revelations": "Resident Evil : Revelations",
    "professeur layton et le masque des miracles": "Professeur Layton et le Masque des Miracles",
    "professeur layton et lheritage des aslantes": "Professeur Layton et l'Héritage des Aslantes",
    "paper mario sticker star": "Paper Mario : Sticker Star",
    "luigis mansion 2": "Luigi's Mansion 2",
    "luigis mansion": "Luigi's Mansion",
    "fire emblem awakening": "Fire Emblem : Awakening",
    "pokemon mystery dungeon gates to infinity": "Pokémon Donjon Mystère : Les Portes de l'Infini",
    "pokemon x": "Pokémon X",
    "pokemon y": "Pokémon Y",
    "pokemon alpha sapphire": "Pokémon Saphir Alpha",
    "pokemon omega ruby": "Pokémon Rubis Oméga",
    "pokemon moon": "Pokémon Lune",
    "pokemon sun": "Pokémon Soleil",
    "pokemon art academy": "Pokémon Art Academy",
    "pokemon rumble world": "Pokémon Rumble World",
    "super pokemon rumble": "Super Pokémon Rumble",
    "pokemon super mystery dungeon": "Pokémon Super Donjon Mystère",
    "the legend of zelda a link between worlds": "The Legend of Zelda : A Link Between Worlds",
    "the legend of zelda ocarina of time 3d": "The Legend of Zelda : Ocarina of Time 3D",
    "the legend of zelda majoras mask 3d": "The Legend of Zelda : Majora's Mask 3D",
    "the legend of zelda triforce heroes": "The Legend of Zelda : Tri Force Heroes",
    "yoshis new island": "Yoshi's New Island",
    "kirby triple deluxe": "Kirby : Triple Deluxe",
    "captain toad treasure tracker": "Captain Toad : Treasure Tracker",
    "mario luigi bowsers inside story bowser jr s journey": "Mario & Luigi : Voyage au centre de Bowser + L'Aventure de Bowser Jr.",
    "mario luigi paper jam bros": "Mario & Luigi : Paper Jam Bros.",
    "mario luigi dream team bros": "Mario & Luigi : Dream Team Bros.",
    "animal crossing new leaf welcome amiibo": "Animal Crossing : New Leaf - Welcome amiibo",
    "dragon quest viii journey of the cursed king": "Dragon Quest VIII : L'Odyssée du roi maudit",
    "dragon quest vii fragments of the forgotten past": "Dragon Quest VII : La Quête des vestiges du monde",
    "super smash bros for nintendo 3ds": "Super Smash Bros. for Nintendo 3DS",
    "fire emblem echoes shadows of valentia": "Fire Emblem Echoes : Shadows of Valentia",
    "fire emblem fates special edition": "Fire Emblem Fates : Édition Spéciale",
    "metroid samus returns": "Metroid : Samus Returns",
    "bravely second end layer": "Bravely Second : End Layer",
    "yo kai watch": "Yo-kai Watch",
    "yo kai watch 3": "Yo-kai Watch 3",
    "star fox 64 3d v1 3": "Star Fox 64 3D",
    "pokemon version diamant": "Pokémon Version Diamant",
    "pokemon ultra moon": "Pokémon Ultra-Lune",
    "pokemon ultra sun": "Pokémon Ultra-Soleil",
    "rhythm heaven megamix": "Rhythm Paradise Megamix",
    "kid icarus uprising": "Kid Icarus : Uprising",
    "professor layton vs phoenix wright ace attorney": "Professeur Layton vs Phoenix Wright : Ace Attorney",
    "mario luigi superstar saga bowsers minions": "Mario & Luigi : Superstar Saga + Les Sbires de Bowser",
}

# Titre à chercher chez libretro quand il diffère du nom du fichier (même clé que NOMS_FR).
ALIAS_LIBRETRO = {
    "rhythm heaven megamix": "Rhythm Paradise Megamix",
}

# Noms Switch à fixer après titledb (typographie) : clé = identifiant de titre.
NOMS_SWITCH = {
    "01006f8002326000": "Animal Crossing : New Horizons",
    "0100dca0064a6000": "Luigi's Mansion 3",
    "0100152000022000": "Mario Kart 8 Deluxe",
    "010003f003a34000": "Pokémon : Let's Go, Pikachu !",
    "01004b3023412000": "Pokémon Rouge Feu",
    "010087c02342e000": "Pokémon Vert Feuille",
    "01007ef00011e000": "The Legend of Zelda : Breath of the Wild",
    "01002da013484000": "The Legend of Zelda : Skyward Sword HD",
    "0100f2c0115b6000": "The Legend of Zelda : Tears of the Kingdom",
    "0100ff500e34a000": "Xenoblade Chronicles : Definitive Edition",
}

# Jeux sans base publique (Switch 2) : nom à la main, pas d'image.
NOMS_PAR_ID = {
    "switch2-pokemon-pokopia": "Pokémon Pokopia",
    "switch2-the-legend-of-zelda-ocarina-of-time": "The Legend of Zelda : Ocarina of Time",
}


def log(msg):
    print(msg, file=sys.stderr, flush=True)


def requete(url, methode="GET", data=None, entetes=None, delai=120):
    req = urllib.request.Request(url, data=data, method=methode, headers={"User-Agent": UA, **(entetes or {})})
    return urllib.request.urlopen(req, timeout=delai)


def telecharger(url, dest):
    """Télécharge `url` vers `dest` (cache : jamais deux fois). Renvoie True si le fichier est là."""
    if os.path.exists(dest) and os.path.getsize(dest) > 0:
        return True
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    try:
        with requete(url) as r:
            data = r.read()
    except (urllib.error.URLError, TimeoutError, OSError):
        return False
    finally:
        time.sleep(PAUSE_S)
    if not data:
        return False
    tmp = dest + ".part"
    with open(tmp, "wb") as f:
        f.write(data)
    os.replace(tmp, dest)
    return True


def _ascii(s):
    return unicodedata.normalize("NFKD", s).encode("ascii", "ignore").decode().lower()


def norm(s):
    """Comparaison : minuscules, lettres et chiffres seulement."""
    return re.sub(r"[^a-z0-9]+", "", _ascii(s))


def norm_mots(s):
    """Clés des tables : mots en minuscules sans accents, apostrophes supprimées."""
    return re.sub(r"[^a-z0-9]+", " ", re.sub(r"['’`]", "", _ascii(s))).strip()


# --- Noms dérivés des fichiers ---------------------------------------------------------------

def deplacer_article(stem):
    """Convention No-Intro « Legend of Zelda, The - X » (ou « X, The ») -> « The Legend of Zelda - X »."""
    m = re.match(r"^(.*?), (The|An|A|Les|Le|La|L')(?= - |$)(.*)$", stem)
    if m:
        sep = "" if m.group(2) == "L'" else " "
        return f"{m.group(2)}{sep}{m.group(1)}{m.group(3)}"
    return stem


def base_titre(stem):
    """Titre sans numérotation « 3DS0080 - », groupes (...) / [...] ni extension déjà retirée."""
    stem = re.sub(r"^\s*3DS\d+\s*-\s*", "", stem)
    stem = re.sub(r"\([^)]*\)|\[[^\]]*\]", " ", stem)
    stem = re.sub(r"\s+", " ", stem).strip(" -.")
    return deplacer_article(stem)


def titre_fichier(chemin):
    return base_titre(os.path.splitext(os.path.basename(chemin.rstrip("/")))[0])


def joli(titre):
    """Titre anglais de fichier mis en forme : « A - B » devient « A : B »."""
    return re.sub(r"\s+", " ", titre.replace(" - ", " : ", 1)).strip()


def titre_court(titre):
    """Titre de tuile : coupé en deux lignes au plus."""
    t = titre
    # Seule la série Zelda perd son préfixe (« Breath of the Wild ») : ailleurs, le nom entier reste lisible.
    if t.startswith("The Legend of Zelda : "):
        t = t.split(" : ", 1)[1]
    if " : " in t:
        # Le sous-titre passe à la ligne, sans les deux-points (« Paper Mario\nSticker Star »).
        return t.replace(" : ", "\n", 1)
    if len(t) <= 14 or " " not in t:
        return t
    mots = t.split(" ")
    i = min(range(1, len(mots)), key=lambda k: abs(len(" ".join(mots[:k])) - len(" ".join(mots[k:]))))
    return " ".join(mots[:i]) + "\n" + " ".join(mots[i:])


def nettoyer_nom_switch(nom):
    nom = re.sub(r"[™®©]", "", nom)
    nom = re.sub(r"^\([^)]*\)\s*", "", nom)
    return re.sub(r"\s+", " ", nom).strip()


# --- libretro-thumbnails ---------------------------------------------------------------------

class _Liens(html.parser.HTMLParser):
    def __init__(self):
        super().__init__()
        self.noms = []

    def handle_starttag(self, tag, attrs):
        if tag == "a":
            h = dict(attrs).get("href", "")
            if h.lower().endswith(".png"):
                self.noms.append(urllib.parse.unquote(h))


REGIONS = ["europe", "france", "world", "germany", "spain", "italy", "usa", "australia", "japan"]


def rang_region(nom):
    bas = nom.lower()
    for i, r in enumerate(REGIONS):
        if f"({r}" in bas or f", {r}" in bas:
            return i
    return len(REGIONS)


def index_libretro(systeme, cache):
    """Liste des boxarts d'un système, mise en cache (une requête par système)."""
    dest = os.path.join(cache, "libretro", systeme.replace(" ", "_") + ".json")
    if os.path.exists(dest):
        with open(dest) as f:
            return json.load(f)
    url = f"{LIBRETRO}/{urllib.parse.quote(systeme)}/Named_Boxarts/"
    try:
        with requete(url) as r:
            texte = r.read().decode("utf-8", "replace")
    except (urllib.error.URLError, TimeoutError, OSError) as e:
        log(f"  libretro {systeme} : liste indisponible ({e})")
        return []
    p = _Liens()
    p.feed(texte)
    noms = sorted(set(p.noms))
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    with open(dest, "w") as f:
        json.dump(noms, f)
    time.sleep(PAUSE_S)
    return noms


def chercher_boxart(titre, noms):
    """(nom de fichier libretro, méthode) ou (None, None) : titre exact normalisé, sinon le plus proche."""
    groupes = {}
    for n in noms:
        groupes.setdefault(norm(base_titre(n[:-4])), []).append(n)
    cle = norm(titre)
    trouve, methode = groupes.get(cle), "exact"
    if not trouve:
        # Tolérance : numéro de version collé au titre, petite variante de sous-titre.
        proches = difflib.get_close_matches(cle, list(groupes), n=1, cutoff=0.86)
        if not proches:
            return None, None
        trouve, methode = groupes[proches[0]], "approchant"
    return sorted(trouve, key=lambda n: (rang_region(n), n))[0], methode


# --- Catalogue du serveur --------------------------------------------------------------------

def lire_catalogue(serveur):
    with requete(serveur.rstrip("/") + "/api/v1/games") as r:
        return json.load(r)


def fichier_base(jeu):
    bases = [f for f in jeu["files"] if f["role"] == "base"]
    return (bases or jeu["files"])[0]["path"]


def index_titledb(cache):
    dest = os.path.join(cache, "titledb", "FR.fr.json")
    if not telecharger(TITLEDB, dest):
        log("titledb : téléchargement impossible")
        return {}
    with open(dest) as f:
        brut = json.load(f)
    idx = {}
    for v in brut.values():
        tid = (v.get("id") or "").lower()
        if tid and v.get("name") and v.get("iconUrl"):
            idx.setdefault(tid, []).append(v)
    return idx


def meilleure_fiche_titledb(fiches):
    """Édition d'origine : ni « RETAIL ONLY », ni « Nintendo Switch 2 Edition », ni démo."""
    def pire(v):
        n = v["name"]
        return ("RETAIL ONLY" in n) * 2 + ("Switch 2" in n) + bool(v.get("isDemo"))
    return sorted(fiches, key=pire)[0]


def toml_chaine(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n") + '"'


def analyser(jeux, cache, avec_images):
    """Une ligne par jeu : nom, source du nom, image (chemin local) et sa source."""
    titledb = None
    libretro = {}
    lignes = []
    for j in jeux:
        entree = fichier_base(j)
        ligne = {"jeu": j, "entree": entree, "nom": None, "source_nom": "", "image": None, "source_image": "", "note": "",
                 "fond": None, "source_fond": ""}
        c = j["console"]
        tid = (j.get("title_id") or "").lower()
        if j["id"] in NOMS_PAR_ID:
            ligne.update(nom=NOMS_PAR_ID[j["id"]], source_nom="table (sans base publique)", note="pas de base publique")
        elif c == "switch" and tid:
            if titledb is None:
                titledb = index_titledb(cache)
            fiches = titledb.get(tid)
            if fiches:
                f = meilleure_fiche_titledb(fiches)
                nom = NOMS_SWITCH.get(tid) or nettoyer_nom_switch(f["name"])
                ligne.update(nom=nom, source_nom="titledb FR" + (" + table" if tid in NOMS_SWITCH else ""))
                if avec_images:
                    dest = os.path.join(cache, "switch", tid + ".jpg")
                    if telecharger(f["iconUrl"], dest):
                        ligne.update(image=dest, source_image="titledb icône")
                    else:
                        ligne["note"] = "icône : téléchargement échoué"
                    if f.get("bannerUrl"):
                        fond = os.path.join(cache, "switch", tid + ".banniere.jpg")
                        if telecharger(f["bannerUrl"], fond):
                            ligne.update(fond=fond, source_fond="titledb bannière")
                else:
                    ligne.update(source_image="titledb icône (non téléchargée)")
        if ligne["nom"] is None:
            t = titre_fichier(entree)
            cle = norm_mots(t)
            ligne.update(nom=NOMS_FR.get(cle) or joli(t), source_nom="table FR" if cle in NOMS_FR else "nom du fichier")
            if c in SYSTEMES:
                if c not in libretro:
                    libretro[c] = index_libretro(SYSTEMES[c], cache)
                fichier, methode = chercher_boxart(ALIAS_LIBRETRO.get(cle, t), libretro[c])
                if fichier:
                    if avec_images:
                        dest = os.path.join(cache, c, hashlib.sha1(fichier.encode()).hexdigest()[:16] + ".png")
                        url = f"{LIBRETRO}/{urllib.parse.quote(SYSTEMES[c])}/Named_Boxarts/{urllib.parse.quote(fichier)}"
                        if telecharger(url, dest):
                            ligne.update(image=dest, source_image=f"libretro {methode} : {fichier}")
                        else:
                            ligne["note"] = "boxart : téléchargement échoué"
                        # Fond : capture d'écran du même jeu (même nom de fichier dans Named_Snaps),
                        # sinon écran-titre (Named_Titles).
                        for dossier in ("Named_Snaps", "Named_Titles"):
                            fond = os.path.join(cache, c, hashlib.sha1((dossier + fichier).encode()).hexdigest()[:16] + ".png")
                            url_f = f"{LIBRETRO}/{urllib.parse.quote(SYSTEMES[c])}/{dossier}/{urllib.parse.quote(fichier)}"
                            if telecharger(url_f, fond):
                                ligne.update(fond=fond, source_fond=f"libretro {methode} {dossier}")
                                break
                    else:
                        ligne.update(source_image=f"libretro {methode} : {fichier} (non téléchargée)")
                else:
                    ligne["note"] = "aucune boxart libretro"
        lignes.append(ligne)
    return lignes


def appliquer_manuel(lignes, dossier):
    """Images choisies à la main : `<dossier>/<id>.cover.*` et `<dossier>/<id>.background.*` remplacent
    celles des bases publiques (jeux absents des bases, comme la Switch 2)."""
    if not dossier or not os.path.isdir(dossier):
        return
    fichiers = os.listdir(dossier)
    for l in lignes:
        jid = l["jeu"]["id"]
        for sorte, cle, source in (("cover", "image", "source_image"), ("background", "fond", "source_fond")):
            choix = sorted(f for f in fichiers if f.startswith(f"{jid}.{sorte}.") and f.rsplit(".", 1)[-1].lower() in ("jpg", "jpeg", "png", "webp"))
            if choix:
                l[cle] = os.path.join(dossier, choix[0])
                l[source] = f"manuel : {choix[0]}"


def ecrire_catalogue(lignes, chemin):
    morceaux = ["# Généré par scripts/visuels.py : titres, titres courts et identifiants de titre.\n"
                "# Les identifiants (id) et les chemins (entry) sont ceux du serveur, inchangés.\n"]
    for l in lignes:
        j = l["jeu"]
        m = ["[[game]]", f"id = {toml_chaine(j['id'])}", f"console = {toml_chaine(j['console'])}",
             f"title = {toml_chaine(l['nom'])}", f"short = {toml_chaine(titre_court(l['nom']))}"]
        if j.get("title_id"):
            m.append(f"title_id = {toml_chaine(j['title_id'])}")
        m.append(f"entry = {toml_chaine(l['entree'])}")
        morceaux.append("\n".join(m) + "\n")
    with open(chemin, "w", encoding="utf-8") as f:
        f.write("\n".join(morceaux))


def envoyer_images(serveur, lignes, forcer, approchants):
    """Envoie jaquettes et fonds manquants (`--forcer` : tous). Une sorte refusée par le serveur
    (version trop ancienne : 404 ou 405 dès le premier envoi) est abandonnée seule."""
    for sorte, cle, source, deja_la in (("cover", "image", "source_image", "cover"), ("background", "fond", "source_fond", "background")):
        envoyes = deja = echecs = douteux = 0
        for l in lignes:
            j, img = l["jeu"], l[cle]
            if not img:
                continue
            # Correspondance approchante (suite, portage, sous-titre) : risque de mauvaise image,
            # envoyée seulement sur demande explicite après relecture de l'essai à blanc.
            if "approchant" in (l.get(source) or "") and not approchants:
                log(f"  {j['id']} : {sorte} par correspondance approchante, non envoyée (--approchants pour l'envoyer)")
                douteux += 1
                continue
            if j.get(deja_la) and not forcer:
                deja += 1
                continue
            with open(img, "rb") as f:
                data = f.read()
            if len(data) > MAX_IMAGE:
                log(f"  {j['id']} : {sorte} trop lourde ({len(data)} octets), ignorée")
                echecs += 1
                continue
            mime = "image/png" if data[:4] == b"\x89PNG" else "image/webp" if data[8:12] == b"WEBP" else "image/jpeg"
            try:
                with requete(f"{serveur.rstrip('/')}/api/v1/games/{urllib.parse.quote(j['id'])}/{sorte}", "PUT", data, {"Content-Type": mime}):
                    envoyes += 1
            except urllib.error.HTTPError as e:
                echecs += 1
                log(f"  {j['id']} : PUT {sorte} refusé ({e.code})")
                if e.code in (404, 405) and envoyes == 0:
                    log(f"Le serveur ne connaît pas encore « {sorte} » (version trop ancienne) : abandon de cette sorte.")
                    break
            except (urllib.error.URLError, TimeoutError, OSError) as e:
                echecs += 1
                log(f"  {j['id']} : {e}")
        print(f"{sorte} : {envoyes} envoyées, {deja} déjà présentes, {douteux} approchantes écartées, {echecs} en échec")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--serveur", default=os.environ.get("LUDO_SERVEUR", "https://ludotheque.srvdreamer.fr"))
    ap.add_argument("--cache", default=os.path.expanduser("~/.cache/ludotheque-visuels"))
    ap.add_argument("--catalogue", metavar="FICHIER", help="écrit ce catalogue.toml")
    ap.add_argument("--envoyer-images", action="store_true", help="envoie les images au serveur (PUT)")
    ap.add_argument("--approchants", action="store_true", help="envoie aussi les images trouvées par correspondance approchante (relire l'essai à blanc d'abord)")
    ap.add_argument("--forcer", action="store_true", help="renvoie aussi les images déjà présentes")
    ap.add_argument("--rescan", action="store_true", help="POST /api/v1/catalog/rescan à la fin")
    ap.add_argument("--sans-telechargement", action="store_true", help="ne télécharge aucune image (listes seulement)")
    ap.add_argument("--manuel", default=os.path.expanduser("~/.local/share/ludotheque/visuels-manuels"),
                    help="dossier d'images choisies à la main (<id>.cover.jpg, <id>.background.jpg)")
    ap.add_argument("--json", metavar="FICHIER", help="écrit le tableau des correspondances en JSON")
    a = ap.parse_args()

    jeux = lire_catalogue(a.serveur)
    log(f"{len(jeux)} jeux lus sur {a.serveur}")
    lignes = analyser(jeux, a.cache, not a.sans_telechargement)
    appliquer_manuel(lignes, a.manuel)

    print(f"{'console':8} {'jeu (id inchangé)':56} {'nom':46} {'source du nom':22} image / fond")
    for l in lignes:
        j = l["jeu"]
        img = ("OUI  " + l["source_image"]) if l["source_image"] else ("NON  " + l["note"])
        fond = l["source_fond"] or "pas de fond"
        print(f"{j['console']:8} {j['id'][:56]:56} {l['nom'][:46]:46} {l['source_nom'][:22]:22} {img} | {fond}")
    resume = {}
    for l in lignes:
        r = resume.setdefault(l["jeu"]["console"], [0, 0, 0, 0])
        r[0] += 1
        r[1] += 1 if l["nom"] else 0
        r[2] += 1 if l["image"] or (l["source_image"] and "non téléchargée" in l["source_image"]) else 0
        r[3] += 1 if l["fond"] else 0
    print("\nRésumé  console  jeux  noms  images  fonds")
    for c, (n, nm, im, fo) in sorted(resume.items()):
        print(f"        {c:8} {n:4} {nm:5} {im:6} {fo:5}")

    if a.json:
        with open(a.json, "w", encoding="utf-8") as f:
            json.dump([{"id": l["jeu"]["id"], "console": l["jeu"]["console"], "nom": l["nom"], "source_nom": l["source_nom"],
                        "image": bool(l["source_image"]), "source_image": l["source_image"]} for l in lignes],
                      f, ensure_ascii=False, indent=1)
    if a.catalogue:
        ecrire_catalogue(lignes, a.catalogue)
        print(f"Catalogue écrit : {a.catalogue} (à copier à la racine des ROM du serveur)")
    if a.envoyer_images:
        envoyer_images(a.serveur, lignes, a.forcer, a.approchants)
    if a.rescan:
        with requete(a.serveur.rstrip("/") + "/api/v1/catalog/rescan", "POST", b"") as r:
            print("Rescan :", r.read().decode())


if __name__ == "__main__":
    main()
