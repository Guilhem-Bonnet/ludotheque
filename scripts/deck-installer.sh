#!/usr/bin/env bash
# Met un Steam Deck dans la Ludothèque depuis le PC, sans jamais risquer ses parties.
#
# À BLANC par défaut : contrôle tout et affiche ce qui serait fait. `--appliquer` pour agir.
#
#   scripts/deck-installer.sh [--appliquer] [--hote deck@192.168.2.124] [--profil claire]
#                             [--ludo <binaire ludo>] [--appimage <AppImage>] [--steam] [--sans-synchro]
#
# Étapes, dans cet ordre (une étape ratée arrête tout) :
#   0. contrôles et prérequis, AVANT toute copie : SSH par clé, aucun jeu en cours sur le Deck,
#      disques de destination montés et assez grands, binaire ludo compatible avec la glibc du
#      Deck, release de l'app, profil Ryujinx du Deck, catalogue du serveur ;
#   a. copie des sauvegardes d'émulateurs du Deck vers le PC ET vers l'hyperviseur (.22), sommes
#      SHA-256 relevées sur le Deck puis vérifiées sur chaque copie : rien n'est installé si une
#      somme diffère ou manque ;
#   b. installation : AppImage dans ~/Applications, `ludo` dans ~/.local/bin, configuration
#      ~/.config/ludo/config.toml, service utilisateur ludo-watch (inotify, aucune scrutation) ;
#   c. (--steam) ajout de l'app à Steam en jeu non-Steam (`steamos-add-to-steam`) ;
#   d. première synchro (`ludo sync`) des jeux Switch du profil trouvés dans les sauvegardes
#      Ryujinx (le moteur `decide` n'écrase jamais une sauvegarde locale, un conflit reste un
#      conflit), puis activation de ludo-watch si aucune synchro n'a échoué.
#
# Rien n'est supprimé sur le Deck : un fichier remplacé est renommé `<nom>.ludo-old-<horodatage>`.
# Les clés (`keys/`, `*.keys`) ne sont jamais copiées. Retour arrière : `--aide`.
set -euo pipefail

HOTE="deck@192.168.2.124"
PROFIL="claire"
APPAREIL="deck"
SERVEUR="https://ludotheque.srvdreamer.fr"
DEPOT="Guilhem-Bonnet/ludotheque"
HYPERVISEUR="root@192.168.2.22"
MONTAGE_PC="/mnt/games"
MONTAGE_HV="/mnt/storage-4tb"
GH="${GH:-$HOME/.local/lib/claude-ops/bin/gh}"
LUDO_BIN=""
APPIMAGE=""
APPLIQUER=0
STEAM=0
SYNCHRO=1
STAMP="$(date +%Y%m%d-%H%M%S)"

aide() { sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'; cat <<'EOF'

Retour arrière (sur le Deck) :
  systemctl --user disable --now ludo-watch.service
  mv ~/.config/systemd/user/ludo-watch.service{,.ludo-old-retour}
  mv ~/.local/bin/ludo{,.ludo-old-retour}; mv ~/.config/ludo{,.ludo-old-retour}
  mv ~/Applications/Ludotheque.AppImage{,.ludo-old-retour}
  # puis, pour chaque <fichier>.ludo-old-<horodatage> laissé par l'installation : lui rendre son nom.
  # Le jeu non-Steam se retire depuis Steam (Gérer > Retirer de la bibliothèque).
  # Sauvegardes : copies intactes dans /mnt/games/Sauvegardes-ludotheque-deck-<horodatage>/ (PC) et
  # .22:/mnt/storage-4tb/sauvegardes-ludotheque/deck-<horodatage>/ ; vérifier : sha256sum -c MANIFESTE.sha256
EOF
}

valeur() { [ $# -ge 2 ] && [ -n "$2" ] || { echo "option $1 : valeur manquante" >&2; exit 1; }; }
while [ $# -gt 0 ]; do
  case "$1" in
    --appliquer) APPLIQUER=1 ;;
    --hote) valeur "$@"; HOTE="$2"; shift ;;
    --profil) valeur "$@"; PROFIL="$2"; shift ;;
    --ludo) valeur "$@"; LUDO_BIN="$2"; shift ;;
    --appimage) valeur "$@"; APPIMAGE="$2"; shift ;;
    --steam) STEAM=1 ;;
    --sans-synchro) SYNCHRO=0 ;;
    -h|--aide|--help) aide; exit 0 ;;
    *) echo "option inconnue : $1 (voir --aide)" >&2; exit 1 ;;
  esac
  shift
done
[[ $PROFIL =~ ^[a-z0-9_-]+$ ]] || { echo "profil invalide : $PROFIL" >&2; exit 1; }

DEST_PC="$MONTAGE_PC/Sauvegardes-ludotheque-deck-$STAMP"
DEST_HV="$MONTAGE_HV/sauvegardes-ludotheque/deck-$STAMP"
OPTS_SSH=(-o BatchMode=yes -o ConnectTimeout=8 -o ServerAliveInterval=30)
RSYNC_SSH="ssh ${OPTS_SSH[*]}"

etape() { printf '\n== %s\n' "$*"; }
echec() { echo "  ÉCHEC : $*" >&2; exit "${CODE:-1}"; }
faire() { # faire <description> <commande…> : exécute seulement avec --appliquer
  local d="$1"; shift
  if [ "$APPLIQUER" = 1 ]; then echo "  → $d"; "$@"; else echo "  [à blanc] $d"; fi
}
deck() { ssh "${OPTS_SSH[@]}" "$HOTE" "$@"; }
hv() { ssh "${OPTS_SSH[@]}" "$HYPERVISEUR" "$@"; }

# Motifs entre crochets ([s]team…) : pgrep -f ne se reconnaît pas dans sa propre ligne de commande
# (ni dans le « bash -c » de ssh qui le porte). `ludo watch|sync` déjà actif : on n'y touche pas non plus.
verifier_aucun_jeu() {
  local jeux
  jeux="$(deck "pgrep -a -f '[s]teamrt64/reaper SteamLaunch|[/]Ryujinx|[Ee]den[.]AppImage|[a]zahar|[m]elonDS|[d]olphin-emu|[r]etroarch|[C]emu|[l]udo (watch|sync)' || true" </dev/null)"
  if [ -n "$jeux" ]; then
    echo "  Jeu (ou synchro) en cours sur le Deck, rien n'est fait. Attendre sa fin : ssh $HOTE pidwait -f '[s]teamrt64/reaper SteamLaunch'" >&2
    printf '%s\n' "$jeux" | cut -c1-160 | sed 's/^/    /' >&2
    exit 11
  fi
  echo "  Aucun jeu en cours sur le Deck"
}

# ---------------------------------------------------------------- 0. contrôles et prérequis
etape "0. Contrôles et prérequis (rien n'est encore copié ni modifié)"
deck true </dev/null 2>/dev/null || { echo "Accès SSH par clé refusé sur $HOTE. Sur le Deck : passwd (si aucun mot de passe), puis depuis le PC : ssh-copy-id $HOTE" >&2; exit 10; }
echo "  SSH par clé : OK ($HOTE)"
verifier_aucun_jeu

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
  if [ -e "$p" ]; then echo "$p"; fi
done
EOF
mapfile -t SOURCES < <(deck "$LISTER" </dev/null)
[ "${#SOURCES[@]}" -gt 0 ] || CODE=12 echec "aucun dossier de sauvegarde trouvé sur le Deck"
printf '  Dossier de sauvegarde : ~/%s\n' "${SOURCES[@]}"
Q_SOURCES="$(printf '%q ' "${SOURCES[@]}")"
TAILLE_KO="$(deck "cd \"\$HOME\" && du -sLk $Q_SOURCES | awk '{s+=\$1} END {print s+0}'" </dev/null)"
echo "  Volume à copier (liens suivis, doublons compris) : $((TAILLE_KO / 1024)) Mo"
deck "df -h --output=avail,pcent \$HOME | tail -1" </dev/null | sed 's/^ */  Place libre sur le Deck (\/home) : /'

# Destinations : montées (sinon mkdir écrirait sur le disque système) et assez grandes (×2 de marge).
mountpoint -q "$MONTAGE_PC" || CODE=13 echec "$MONTAGE_PC n'est pas monté sur le PC"
hv "mountpoint -q '$MONTAGE_HV'" </dev/null || CODE=13 echec "$MONTAGE_HV n'est pas monté sur l'hyperviseur"
LIBRE_PC="$(df -Pk "$MONTAGE_PC" | awk 'NR==2 {print $4}')"
LIBRE_HV="$(hv "df -Pk '$MONTAGE_HV' | awk 'NR==2 {print \$4}'" </dev/null)"
[ "$LIBRE_PC" -gt $((TAILLE_KO * 2)) ] || CODE=13 echec "place insuffisante sur $MONTAGE_PC ($((LIBRE_PC / 1024)) Mo)"
[ "$LIBRE_HV" -gt $((TAILLE_KO * 2)) ] || CODE=13 echec "place insuffisante sur l'hyperviseur ($((LIBRE_HV / 1024)) Mo)"
echo "  Destinations montées ; libre : PC $((LIBRE_PC / 1048576)) Go, hyperviseur $((LIBRE_HV / 1048576)) Go"

# Binaire ludo : présent, et glibc demandée ≤ glibc du Deck.
[ -n "$LUDO_BIN" ] || LUDO_BIN="$(command -v ludo || true)"
[ -n "$LUDO_BIN" ] && [ -x "$LUDO_BIN" ] || CODE=15 echec "binaire ludo introuvable (--ludo <chemin>)"
GLIBC_BIN="$(objdump -T "$LUDO_BIN" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -1)"
GLIBC_DECK="$(deck "ldd --version | head -1 | grep -o '[0-9.]*\$'" </dev/null)"
[ "$(printf '%s\n%s\n' "$GLIBC_BIN" "$GLIBC_DECK" | sort -V | tail -1)" = "$GLIBC_DECK" ] || CODE=15 echec "ludo demande glibc $GLIBC_BIN, le Deck a $GLIBC_DECK"
echo "  ludo : $LUDO_BIN ($("$LUDO_BIN" --version)), glibc $GLIBC_BIN ≤ $GLIBC_DECK (Deck)"

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
if [ -z "$APPIMAGE" ]; then
  TAG="$("$GH" release view -R "$DEPOT" --json tagName -q .tagName)" || CODE=15 echec "release introuvable sur $DEPOT"
  echo "  Dernière release de l'app : $TAG"
  if [ "$APPLIQUER" = 1 ]; then
    "$GH" release download "$TAG" -R "$DEPOT" -p '*_amd64.AppImage' -D "$TMP"
    APPIMAGE="$(/bin/ls "$TMP"/*_amd64.AppImage)"
  fi
fi
[ "$APPLIQUER" = 0 ] || [ -s "$APPIMAGE" ] || CODE=15 echec "AppImage absente : $APPIMAGE"

# Profil Ryujinx du Deck (dernier ouvert, sinon le premier) : identifiant 32 hex + nom en chaîne JSON
# (valide aussi en chaîne TOML).
RYU="$(deck "python3 -c 'import json,os;d=json.load(open(os.path.expanduser(\"~/.config/Ryujinx/system/Profiles.json\")));u=d.get(\"last_opened\") or d[\"profiles\"][0][\"user_id\"];print(u.lower());print(json.dumps(next(p[\"name\"] for p in d[\"profiles\"] if p[\"user_id\"].lower()==u.lower()),ensure_ascii=False))'" </dev/null)" || CODE=16 echec "profil Ryujinx du Deck illisible"
RYU_UID="$(sed -n 1p <<<"$RYU")"; RYU_NOM_TOML="$(sed -n 2p <<<"$RYU")"
[[ $RYU_UID =~ ^[0-9a-f]{32}$ ]] || CODE=16 echec "identifiant Ryujinx inattendu : $RYU_UID"
echo "  Profil Ryujinx du Deck → profil Ludothèque « $PROFIL » : $RYU_UID ($RYU_NOM_TOML)"

CATALOGUE="$(curl -fsS -m 15 "$SERVEUR/api/v1/games")" || CODE=17 echec "catalogue du serveur injoignable"
python3 -c 'import json,sys; d=json.loads(sys.argv[1]); assert isinstance(d,list) and d' "$CATALOGUE" || CODE=17 echec "catalogue du serveur illisible"
echo "  Catalogue du serveur : $(python3 -c 'import json,sys;print(len(json.loads(sys.argv[1])))' "$CATALOGUE") jeux"

# ---------------------------------------------------------------- a. sauvegarde vérifiée
etape "a. Copie des sauvegardes → $DEST_PC (PC) et $HYPERVISEUR:$DEST_HV"
copier_et_verifier() {
  CODE=14
  mkdir "$DEST_PC" || echec "$DEST_PC existe déjà"
  # Manifeste relevé SUR LE DECK, avant copie (liens suivis, clés et journaux exclus).
  # pipefail + stderr de find relu : un dossier illisible ou une boucle de liens = arrêt, pas un manifeste court.
  deck "bash -c $(printf '%q' "set -o pipefail; cd \"\$HOME\" && find -L $Q_SOURCES -type f ! -path '*/keys/*' ! -path '*.keys/*' ! -name '*.keys' ! -path '*/Logs/*' ! -path '*/log/*' -print0 2>/tmp/ludo-manifeste.err | sort -z | xargs -0 -r sha256sum && ! test -s /tmp/ludo-manifeste.err")" \
    </dev/null > "$DEST_PC/MANIFESTE.sha256" || echec "manifeste incomplet sur le Deck (voir /tmp/ludo-manifeste.err sur le Deck)"
  local n; n=$(wc -l < "$DEST_PC/MANIFESTE.sha256")
  [ "$n" -gt 0 ] || echec "manifeste vide"
  echo "  Manifeste du Deck : $n fichiers"
  # Code 23/24 (lien pendant, fichier disparu) toléré : les sommes jugent ensuite.
  local rc=0
  rsync -a --copy-links --relative -e "$RSYNC_SSH" \
    --exclude 'keys/' --exclude '*.keys' --exclude 'prod.keys*' --exclude 'title.keys*' --exclude 'Logs/' --exclude 'log/' \
    "${SOURCES[@]/#/$HOTE:./}" "$DEST_PC/" || rc=$?
  case $rc in 0|23|24) ;; *) echec "rsync Deck → PC : code $rc" ;; esac
  local ok
  ok=$(cd "$DEST_PC" && LC_ALL=C sha256sum -c MANIFESTE.sha256 2>/dev/null | grep -c ': OK$' || true)
  [ "$ok" = "$n" ] || echec "copie du PC : $ok/$n sommes identiques"
  echo "  Copie PC vérifiée : $ok/$n sommes identiques ($(du -sh "$DEST_PC" | cut -f1))"
  hv "mkdir -p '$(dirname "$DEST_HV")' && mkdir '$DEST_HV'" </dev/null || echec "$DEST_HV existe déjà sur l'hyperviseur"
  rsync -a -e "$RSYNC_SSH" "$DEST_PC/" "$HYPERVISEUR:$DEST_HV/"
  ok=$(hv "cd '$DEST_HV' && LC_ALL=C sha256sum -c MANIFESTE.sha256 2>/dev/null | grep -c ': OK\$' || true" </dev/null)
  [ "$ok" = "$n" ] || echec "copie de l'hyperviseur : $ok/$n sommes identiques"
  echo "  Copie hyperviseur vérifiée : $ok/$n sommes identiques"
  CODE=1
}
faire "copier puis comparer les sommes SHA-256 (Deck → PC → hyperviseur)" copier_et_verifier

# ---------------------------------------------------------------- b. installation
etape "b. Installation sur le Deck"
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
ryujinx_nom = $RYU_NOM_TOML
EOF

cat > "$TMP/ludo-watch.service" <<'EOF'
[Unit]
Description=Ludothèque : synchronisation des sauvegardes de ce Deck dès qu'un émulateur les écrit (inotify)

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

# Script distant de pose : set -eu ; l'ancien fichier (même un lien pendant) est renommé avant que
# le nouveau ne prenne sa place ; un binaire est d'abord exécuté (--version) pour prouver qu'il tourne.
read -r -d '' POSE <<'EOF' || true
set -eu
dst="$1"; mode="$2"; stamp="$3"; essai="$4"
cd "$HOME"
nouveau="$dst.ludo-nouveau-$stamp"
chmod "$mode" "$nouveau"
if [ "$essai" = 1 ]; then "./$nouveau" --version >/dev/null; fi
if [ -e "$dst" ] || [ -L "$dst" ]; then
  if cmp -s "$dst" "$nouveau"; then mv -T -- "$nouveau" "$dst.ludo-identique-$stamp"; echo "    inchangé : ~/$dst"; exit 0; fi
  mv -T -n -- "$dst" "$dst.ludo-old-$stamp"
  [ ! -e "$dst" ] && [ ! -L "$dst" ]
  echo "    ancien mis de côté : ~/$dst.ludo-old-$stamp"
fi
mv -T -n -- "$nouveau" "$dst"
[ ! -e "$nouveau" ]
echo "    posé : ~/$dst"
EOF
poser() { # poser <local> <distant relatif à $HOME> <mode> <essai 0|1>
  local src="$1" dst="$2" mode="$3" essai="$4"
  deck "mkdir -p \"\$HOME/$(dirname "$dst")\"" </dev/null
  scp -q "${OPTS_SSH[@]}" "$src" "$HOTE:$dst.ludo-nouveau-$STAMP"
  deck "bash -s -- $(printf '%q ' "$dst" "$mode" "$STAMP" "$essai")" <<<"$POSE"
}
installer() {
  verifier_aucun_jeu # revérifié : la copie a pu durer
  poser "$APPIMAGE" "Applications/Ludotheque.AppImage" 755 0
  poser "$LUDO_BIN" ".local/bin/ludo" 755 1
  poser "$TMP/config.toml" ".config/ludo/config.toml" 644 0
  poser "$TMP/ludo-watch.service" ".config/systemd/user/ludo-watch.service" 644 0
  deck "\$HOME/.local/bin/ludo --version && \$HOME/.local/bin/ludo emulateurs" </dev/null
}
faire "poser AppImage, ludo, config.toml, ludo-watch.service (anciens renommés .ludo-old-$STAMP)" installer
echo "  --- config.toml prévue :"; sed 's/^/    /' "$TMP/config.toml"

# ---------------------------------------------------------------- c. Steam
etape "c. Jeu non-Steam"
if [ "$STEAM" = 1 ]; then
  faire "steamos-add-to-steam ~/Applications/Ludotheque.AppImage" deck "steamos-add-to-steam \"\$HOME/Applications/Ludotheque.AppImage\"" </dev/null
else
  echo "  non demandé (--steam)"
fi

# ---------------------------------------------------------------- d. première synchro
etape "d. Première synchro (profil $PROFIL)"
synchroniser() {
  # Titres trouvés dans les sauvegardes Ryujinx (ExtraData0) de cet utilisateur ou de l'appareil.
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
  local -a liste; mapfile -t liste <<<"$titres"
  local n=0 ok=0 conflits=0 erreurs=0 t id rc sortie
  for t in "${liste[@]}"; do
    [[ $t =~ ^[0-9a-f]{16}$ ]] || continue
    id="$(python3 -c 'import json,sys;t=sys.argv[2];print(next((g["id"] for g in json.loads(sys.argv[1]) if (g.get("title_id") or "").lower()==t),""))' "$CATALOGUE" "$t")"
    if [ -z "$id" ]; then echo "    $t : absent du catalogue, ignoré"; continue; fi
    n=$((n+1)); rc=0
    sortie="$(deck "\$HOME/.local/bin/ludo sync $(printf '%q ' --profil "$PROFIL" --jeu "$id" --emulateur ryujinx --title-id "$t")" </dev/null 2>&1)" || rc=$?
    echo "    $id ($t) : $sortie (code $rc)"
    case $rc in 0|4) ok=$((ok+1)) ;; 2) conflits=$((conflits+1)) ;; *) erreurs=$((erreurs+1)) ;; esac
  done
  echo "  Synchro : $ok/$n sans conflit, $conflits conflit(s) laissé(s) à trancher, $erreurs échec(s)"
  [ "$erreurs" = 0 ] || CODE=18 echec "$erreurs synchro(s) en échec : ludo-watch n'est pas activé"
}
if [ "$SYNCHRO" = 1 ]; then
  faire "ludo sync de chaque jeu Switch du profil (aucun écrasement local, conflits laissés)" synchroniser
  faire "activer ludo-watch.service (systemctl --user enable --now)" deck "systemctl --user daemon-reload && systemctl --user enable --now ludo-watch.service && systemctl --user is-active ludo-watch.service" </dev/null
else
  echo "  non demandée (--sans-synchro)"
fi

if [ "$APPLIQUER" = 1 ]; then echo $'\nTerminé.'; else echo $'\nÀ blanc : rien n\'a été modifié. Relancer avec --appliquer.'; fi
