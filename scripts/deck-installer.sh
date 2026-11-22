#!/usr/bin/env bash
# Met un Steam Deck dans la Ludothèque depuis le PC, sans jamais risquer ses parties.
#
# À BLANC par défaut : affiche ce qui serait fait. `--appliquer` pour agir.
#
#   scripts/deck-installer.sh [--appliquer] [--hote deck@192.168.2.124] [--profil claire]
#                             [--ludo <binaire ludo>] [--appimage <AppImage>] [--steam] [--sans-synchro]
#
# Étapes, dans cet ordre (une étape ratée arrête tout) :
#   0. contrôles : SSH par clé, aucun jeu en cours sur le Deck, place disque ;
#   a. copie des sauvegardes d'émulateurs du Deck vers le PC ET vers l'hyperviseur (.22), sommes
#      SHA-256 relevées sur le Deck puis vérifiées sur chaque copie : rien n'est installé si une
#      somme diffère ;
#   b. installation : AppImage dans ~/Applications, `ludo` dans ~/.local/bin, configuration
#      ~/.config/ludo/config.toml, service utilisateur ludo-watch (inotify, aucune scrutation) ;
#   c. (--steam) ajout de l'app à Steam en jeu non-Steam (`steamos-add-to-steam`) ;
#   d. première synchro (`ludo sync`) des jeux Switch du profil trouvés dans les sauvegardes
#      Ryujinx : le moteur `decide` n'écrase jamais une sauvegarde locale, un conflit reste un conflit.
#
# Rien n'est supprimé sur le Deck : un fichier remplacé est renommé `<nom>.ludo-old-<horodatage>`.
# Les clés (`keys/`, `*.keys`) ne sont jamais copiées.
# Retour arrière : voir `--aide` (section « Retour arrière »).
set -euo pipefail

HOTE="deck@192.168.2.124"
PROFIL="claire"
APPAREIL="deck"
SERVEUR="https://ludotheque.srvdreamer.fr"
DEPOT="Guilhem-Bonnet/ludotheque"
HYPERVISEUR="root@192.168.2.22"
DEST_PC_BASE="/mnt/games"
DEST_HV_BASE="/mnt/storage-4tb/sauvegardes-ludotheque"
GH="${GH:-$HOME/.local/lib/claude-ops/bin/gh}"
LUDO_BIN=""
APPIMAGE=""
APPLIQUER=0
STEAM=0
SYNCHRO=1
STAMP="$(date +%Y%m%d-%H%M%S)"
JOUR="$(date +%Y%m%d)"

aide() { sed -n '2,23p' "$0" | sed 's/^# \{0,1\}//'; cat <<'EOF'

Retour arrière (sur le Deck) :
  systemctl --user disable --now ludo-watch.service
  mv ~/.config/systemd/user/ludo-watch.service{,.ludo-old-retour}
  mv ~/.local/bin/ludo{,.ludo-old-retour}; mv ~/.config/ludo{,.ludo-old-retour}
  mv ~/Applications/Ludotheque.AppImage{,.ludo-old-retour}
  # et, pour chaque <fichier>.ludo-old-<horodatage> laissé par l'installation : le renommer à l'identique.
  # Le jeu non-Steam se retire depuis Steam (Gérer > Retirer de la bibliothèque).
  # Sauvegardes : copies intactes dans /mnt/games/Sauvegardes-ludotheque-deck-<date>/ (PC) et
  # .22:/mnt/storage-4tb/sauvegardes-ludotheque/deck-<date>/ (MANIFESTE.sha256 pour vérifier).
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --appliquer) APPLIQUER=1 ;;
    --hote) HOTE="$2"; shift ;;
    --profil) PROFIL="$2"; shift ;;
    --ludo) LUDO_BIN="$2"; shift ;;
    --appimage) APPIMAGE="$2"; shift ;;
    --steam) STEAM=1 ;;
    --sans-synchro) SYNCHRO=0 ;;
    -h|--aide|--help) aide; exit 0 ;;
    *) echo "option inconnue : $1 (voir --aide)" >&2; exit 1 ;;
  esac
  shift
done

DEST_PC="$DEST_PC_BASE/Sauvegardes-ludotheque-deck-$JOUR"
DEST_HV="$DEST_HV_BASE/deck-$JOUR"
SSH=(ssh -o BatchMode=yes -o ConnectTimeout=8 "$HOTE")

etape() { printf '\n== %s\n' "$*"; }
faire() { # faire <description> <commande…> : exécute seulement avec --appliquer
  local d="$1"; shift
  if [ "$APPLIQUER" = 1 ]; then echo "  → $d"; "$@"; else echo "  [à blanc] $d"; fi
}
deck() { "${SSH[@]}" "$@"; }

# ---------------------------------------------------------------- 0. contrôles
etape "0. Contrôles"
deck true 2>/dev/null || { echo "Accès SSH par clé refusé sur $HOTE. Sur le Deck : passwd (si aucun mot de passe), puis depuis le PC : ssh-copy-id $HOTE" >&2; exit 10; }
echo "  SSH par clé : OK ($HOTE)"
# Motifs entre crochets ([s]team…) : pgrep/pidwait -f ne se reconnaissent pas dans leur propre ligne
# de commande (ni dans le « bash -c » de ssh qui les porte).
JEUX="$(deck "pgrep -a -f '[s]teamrt64/reaper SteamLaunch|[/]Ryujinx|[Ee]den[.]AppImage|[a]zahar|[m]elonDS|[d]olphin-emu|[r]etroarch' || true")"
if [ -n "$JEUX" ]; then
  echo "  Jeu en cours sur le Deck, rien n'est fait (attendre sa fin : ssh $HOTE pidwait -f '[s]teamrt64/reaper SteamLaunch') :" >&2
  printf '%s\n' "$JEUX" | cut -c1-160 | sed 's/^/    /' >&2
  exit 11
fi
echo "  Aucun jeu en cours"
deck "df -h --output=avail,pcent \$HOME | tail -1" | sed 's/^/  Place libre sur le Deck (\/home) : /'

# Dossiers de sauvegarde (chemins relatifs à $HOME sur le Deck). Les liens d'EmuDeck
# (~/Emulation/saves/<émulateur>/<x> → dossier réel) sont suivis à la copie.
read -r -d '' LISTER <<'EOF' || true
cd "$HOME" || exit 1
for p in Emulation/saves \
         .config/Ryujinx/bis/user/save .config/Ryujinx/bis/user/saveMeta .config/Ryujinx/bis/system/save .config/Ryujinx/system \
         .local/share/eden/nand/user/save .local/share/eden/nand/system/save \
         .local/share/azahar-emu/sdmc .local/share/azahar-emu/states \
         .var/app/net.kuribo64.melonDS/config/melonDS .config/melonDS \
         .local/share/Cemu/mlc01/usr/save; do
  [ -e "$p" ] && echo "$p"
done
EOF
mapfile -t SOURCES < <(deck "$LISTER")
[ "${#SOURCES[@]}" -gt 0 ] || { echo "Aucun dossier de sauvegarde trouvé sur le Deck" >&2; exit 12; }
printf '  Dossiers de sauvegarde : %s\n' "${SOURCES[@]}"
EXCLUS=(--exclude 'keys/' --exclude '*.keys' --exclude 'Logs/' --exclude 'log/')

# ---------------------------------------------------------------- a. sauvegarde vérifiée
etape "a. Copie des sauvegardes → $DEST_PC (PC) et $HYPERVISEUR:$DEST_HV"
copier_et_verifier() {
  mkdir -p "$DEST_PC"
  [ -z "$(/bin/ls -A "$DEST_PC")" ] || { echo "  $DEST_PC existe déjà et n'est pas vide : rien n'est écrasé, arrêt." >&2; exit 13; }
  # Manifeste relevé SUR LE DECK (fichiers des liens suivis, clés exclues), avant copie.
  local q; q="$(printf '%q ' "${SOURCES[@]}")"
  deck "cd \"\$HOME\" && find -L $q -type f ! -path '*/keys/*' ! -name '*.keys' ! -path '*/Logs/*' ! -path '*/log/*' -print0 | sort -z | xargs -0 sha256sum" > "$DEST_PC/MANIFESTE.sha256"
  local n; n=$(wc -l < "$DEST_PC/MANIFESTE.sha256")
  echo "  Manifeste du Deck : $n fichiers"
  rsync -a --copy-links --relative "${EXCLUS[@]}" "${SOURCES[@]/#/$HOTE:./}" "$DEST_PC/"
  ( cd "$DEST_PC" && sha256sum --quiet -c MANIFESTE.sha256 ) || { echo "  Somme différente sur la copie du PC : arrêt, rien n'est installé." >&2; exit 14; }
  echo "  Copie PC vérifiée : $n/$n sommes identiques ($(du -sh "$DEST_PC" | cut -f1))"
  ssh -o BatchMode=yes "$HYPERVISEUR" "mkdir -p '$DEST_HV' && [ -z \"\$(ls -A '$DEST_HV')\" ]" || { echo "  $DEST_HV existe déjà et n'est pas vide : arrêt." >&2; exit 13; }
  rsync -a "$DEST_PC/" "$HYPERVISEUR:$DEST_HV/"
  ssh -o BatchMode=yes "$HYPERVISEUR" "cd '$DEST_HV' && sha256sum --quiet -c MANIFESTE.sha256" || { echo "  Somme différente sur la copie de l'hyperviseur : arrêt." >&2; exit 14; }
  echo "  Copie hyperviseur vérifiée : $n/$n sommes identiques"
}
faire "copier puis comparer les sommes SHA-256 (Deck → PC → hyperviseur)" copier_et_verifier

# ---------------------------------------------------------------- b. installation
etape "b. Installation sur le Deck"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
if [ -z "$APPIMAGE" ]; then
  TAG="$("$GH" release view -R "$DEPOT" --json tagName -q .tagName)"
  echo "  Dernière release : $TAG"
  if [ "$APPLIQUER" = 1 ]; then
    "$GH" release download "$TAG" -R "$DEPOT" -p '*_amd64.AppImage' -D "$TMP"
    APPIMAGE="$(/bin/ls "$TMP"/*_amd64.AppImage)"
  fi
fi
if [ -z "$LUDO_BIN" ]; then
  LUDO_BIN="$(command -v ludo || true)"
fi
[ -n "$LUDO_BIN" ] && [ -x "$LUDO_BIN" ] || { echo "  Binaire ludo introuvable (--ludo <chemin>)" >&2; exit 15; }
echo "  ludo : $LUDO_BIN ($("$LUDO_BIN" --version))"

RYU_UID="$(deck "python3 -c 'import json,os;d=json.load(open(os.path.expanduser(\"~/.config/Ryujinx/system/Profiles.json\")));print(d.get(\"last_opened\") or d[\"profiles\"][0][\"user_id\"])'" 2>/dev/null || true)"
RYU_NOM="$(deck "python3 -c 'import json,os;d=json.load(open(os.path.expanduser(\"~/.config/Ryujinx/system/Profiles.json\")));u=\"$RYU_UID\";print(next(p[\"name\"] for p in d[\"profiles\"] if p[\"user_id\"]==u))'" 2>/dev/null || true)"
[ -n "$RYU_UID" ] || { echo "  Profil Ryujinx du Deck introuvable" >&2; exit 16; }
echo "  Profil Ryujinx du Deck → profil Ludothèque « $PROFIL » : $RYU_UID ($RYU_NOM)"

cat > "$TMP/config.toml" <<EOF
# Ludothèque — Steam Deck ($PROFIL). Écrit par scripts/deck-installer.sh le $STAMP.
serveur = "$SERVEUR"
appareil = "$APPAREIL"
profil_defaut = "$PROFIL"

# Émulateur Switch utilisé sur ce Deck : Ryubing (flatpak io.github.ryubing.Ryujinx, lancé hors conteneur
# par ~/Emulation/tools/launchers/ryubing-flatpak.sh). Eden n'est pas relié : ses sauvegardes plus
# anciennes entreraient en conflit avec celles de Ryujinx pour les mêmes jeux.
[consoles]
switch = "ryujinx"

[profils.$PROFIL]
ryujinx_user_id = "$RYU_UID"
ryujinx_nom = "$RYU_NOM"
EOF

cat > "$TMP/ludo-watch.service" <<'EOF'
[Unit]
Description=Ludothèque : synchronisation des sauvegardes de ce Deck dès qu'un émulateur les écrit (inotify)
After=network-online.target

[Service]
# Réagit aux écritures (inotify) dans les dossiers de sauvegarde ; synchronise un jeu 20 s après
# sa dernière écriture. Aucun travail à vide, aucune scrutation.
Environment=PATH=%h/.local/bin:/usr/local/bin:/usr/bin:/bin
ExecStart=%h/.local/bin/ludo watch
Restart=on-failure
RestartSec=10
Nice=10

[Install]
WantedBy=default.target
EOF

# Pose un fichier sur le Deck ; un fichier existant différent est renommé .ludo-old-<horodatage>.
poser() { # poser <local> <distant relatif à $HOME> <mode>
  local src="$1" dst="$2" mode="$3"
  deck "mkdir -p \"\$(dirname ~/'$dst')\""
  scp -q -o BatchMode=yes "$src" "$HOTE:$dst.ludo-nouveau"
  deck "cd ~ && chmod $mode '$dst.ludo-nouveau' && if [ -e '$dst' ]; then if cmp -s '$dst' '$dst.ludo-nouveau'; then rm '$dst.ludo-nouveau'; echo '    inchangé : ~/$dst'; exit 0; fi; mv '$dst' '$dst.ludo-old-$STAMP'; echo '    ancien mis de côté : ~/$dst.ludo-old-$STAMP'; fi; mv '$dst.ludo-nouveau' '$dst'; echo '    posé : ~/$dst'"
}
installer() {
  poser "$APPIMAGE" "Applications/Ludotheque.AppImage" 755
  poser "$LUDO_BIN" ".local/bin/ludo" 755
  poser "$TMP/config.toml" ".config/ludo/config.toml" 644
  poser "$TMP/ludo-watch.service" ".config/systemd/user/ludo-watch.service" 644
  deck "\$HOME/.local/bin/ludo --version && \$HOME/.local/bin/ludo emulateurs"
}
faire "poser AppImage, ludo, config.toml, ludo-watch.service (anciens renommés .ludo-old-$STAMP)" installer
echo "  --- config.toml prévue :"; sed 's/^/    /' "$TMP/config.toml"

# ---------------------------------------------------------------- c. Steam
etape "c. Jeu non-Steam"
if [ "$STEAM" = 1 ]; then
  faire "steamos-add-to-steam ~/Applications/Ludotheque.AppImage" deck "steamos-add-to-steam \"\$HOME/Applications/Ludotheque.AppImage\""
else
  echo "  non demandé (--steam)"
fi

# ---------------------------------------------------------------- d. première synchro
etape "d. Première synchro (profil $PROFIL)"
synchroniser() {
  # Jeux Switch du profil : titres trouvés dans les sauvegardes Ryujinx (ExtraData0) de cet
  # utilisateur ou de l'appareil, croisés avec le catalogue du serveur.
  local titres; titres="$(deck "python3 - '$RYU_UID'" <<'PY'
import os, struct, sys
# Même ordre que layout::ryujinx_uid_bytes : moitié haute puis basse, chacune en u64 petit-boutiste.
uid = bytes.fromhex(sys.argv[1]); uid = uid[:8][::-1] + uid[8:][::-1]
racine = os.path.expanduser("~/.config/Ryujinx/bis/user/save")
vus = set()
for d in sorted(os.listdir(racine)):
    try:
        e = open(os.path.join(racine, d, "ExtraData0"), "rb").read()
    except OSError:
        continue
    if len(e) < 33:
        continue
    titre, u = struct.unpack("<Q", e[0:8])[0], e[8:24]
    if titre and (u == uid or u == bytes(16)):
        vus.add(f"{titre:016x}")
print("\n".join(sorted(vus)))
PY
)"
  local cat; cat="$(curl -s -m 15 "$SERVEUR/api/v1/games")"
  local n=0 ok=0 conflits=0
  while read -r t; do
    [ -n "$t" ] || continue
    local id; id="$(printf '%s' "$cat" | python3 -c "import json,sys;t=sys.argv[1];print(next((g['id'] for g in json.load(sys.stdin) if (g.get('titleId') or g.get('title_id') or '').lower()==t),''))" "$t")"
    if [ -z "$id" ]; then echo "    $t : absent du catalogue, ignoré"; continue; fi
    n=$((n+1))
    local rc=0 sortie
    sortie="$(deck "\$HOME/.local/bin/ludo sync --profil '$PROFIL' --jeu '$id' --emulateur ryujinx --title-id '$t'" 2>&1)" || rc=$?
    echo "    $id ($t) : $sortie (code $rc)"
    case $rc in 0) ok=$((ok+1)) ;; 2) conflits=$((conflits+1)) ;; esac
  done <<<"$titres"
  echo "  Synchro : $ok/$n à jour, $conflits conflit(s) laissé(s) à trancher"
}
if [ "$SYNCHRO" = 1 ]; then
  faire "ludo sync de chaque jeu Switch du profil (aucun écrasement local, conflits laissés)" synchroniser
  faire "activer ludo-watch.service (systemctl --user enable --now)" deck "systemctl --user daemon-reload && systemctl --user enable --now ludo-watch.service && systemctl --user is-active ludo-watch.service"
else
  echo "  non demandée (--sans-synchro)"
fi

[ "$APPLIQUER" = 1 ] && echo $'\nTerminé.' || echo $'\nÀ blanc : rien n\'a été modifié. Relancer avec --appliquer.'
