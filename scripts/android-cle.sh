#!/usr/bin/env bash
# Clé de signature de l'app Android (version release), à lancer UNE fois par Guilhem.
#
# 1. Génère la clé dans ~/.config/ludotheque/android/ (dossier 700, fichiers 600) si elle n'existe pas :
#    ludotheque-release.jks (PKCS12, RSA 4096, 100 ans) + signature.properties (lu par build.gradle.kts).
# 2. Avec --secrets : pose les secrets du dépôt lus par .github/workflows/android-release.yml
#    (LUDO_ANDROID_KEYSTORE_B64, LUDO_ANDROID_KEYSTORE_PASSWORD, LUDO_ANDROID_KEY_ALIAS), par `gh`.
#
# Rien n'est affiché : ni mot de passe, ni clé. PERDRE CETTE CLÉ = plus aucune mise à jour possible
# depuis l'app (Android refuse un APK signé par une autre clé) : la garder dans la sauvegarde du poste.
#
# Usage : scripts/android-cle.sh [--secrets] [--gh <binaire gh>]
set -euo pipefail

dir="$HOME/.config/ludotheque/android"
store="$dir/ludotheque-release.jks"
props="$dir/signature.properties"
alias=ludotheque
secrets=""
gh=gh
while [ $# -gt 0 ]; do
  case "$1" in
    --secrets) secrets=1 ;;
    --gh) gh="$2"; shift ;;
    -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "Option inconnue : $1" >&2; exit 2 ;;
  esac
  shift
done

if [ -z "${JAVA_HOME:-}" ]; then
  JAVA_HOME="$(ls -d "$HOME"/.cache/jdk/jdk-21* 2>/dev/null | sort | tail -1 || true)"
fi
keytool="${JAVA_HOME:+$JAVA_HOME/bin/}keytool"
command -v "$keytool" >/dev/null || { echo "keytool introuvable (JDK attendu dans JAVA_HOME ou ~/.cache/jdk/jdk-21*)" >&2; exit 1; }
# keytool traduit ses sorties selon la locale (« SHA 256: » en français, constat du 22/11) : sortie
# anglaise imposée pour lire l'empreinte.
kt() { "$keytool" -J-Duser.language=en -J-Duser.country=US "$@"; }
# Empreinte SHA-256 du certificat (« AB:CD:… »), quelle que soit l'orthographe de l'étiquette.
empreinte() { sed -n -E 's/^[[:space:]]*SHA[ -]?256[[:space:]]*:[[:space:]]*//p' | head -1; }

umask 077
mkdir -p "$dir"
chmod 700 "$dir"
if [ -f "$store" ]; then
  echo "Clé déjà présente : $store (gardée)"
else
  pass="$(head -c 32 /dev/urandom | base64 | tr -d '/+=' | head -c 40)"
  # Mot de passe passé par l'environnement (jamais sur la ligne de commande, visible dans ps).
  LUDO_KS_PASS="$pass" kt -genkeypair -keystore "$store" -storetype PKCS12 -alias "$alias" -keyalg RSA -keysize 4096 \
    -validity 36500 -dname "CN=Ludotheque, O=Ludotheque" -storepass:env LUDO_KS_PASS -keypass:env LUDO_KS_PASS >/dev/null 2>&1
  printf 'storeFile=%s\nstorePassword=%s\nkeyAlias=%s\nkeyPassword=%s\n' "$store" "$pass" "$alias" "$pass" > "$props"
  chmod 600 "$store" "$props"
  echo "Clé créée : $store"
fi

# Empreinte du certificat (publique) : permet de vérifier un APK avec apksigner verify --print-certs.
pass="$(sed -n 's/^storePassword=//p' "$props")"
cert="$(LUDO_KS_PASS="$pass" kt -list -v -keystore "$store" -storepass:env LUDO_KS_PASS 2>/dev/null | empreinte)"
[ -n "$cert" ] || { echo "Empreinte du certificat illisible (sortie de keytool inattendue)" >&2; exit 1; }
echo "Certificat SHA256: $cert"

if [ -n "$secrets" ]; then
  repo=Guilhem-Bonnet/ludotheque
  base64 -w0 "$store" | "$gh" secret set LUDO_ANDROID_KEYSTORE_B64 -R "$repo" >/dev/null
  printf '%s' "$pass" | "$gh" secret set LUDO_ANDROID_KEYSTORE_PASSWORD -R "$repo" >/dev/null
  printf '%s' "$alias" | "$gh" secret set LUDO_ANDROID_KEY_ALIAS -R "$repo" >/dev/null
  "$gh" variable set LUDO_ANDROID_CERT_SHA256 -R "$repo" --body "$cert" >/dev/null
  echo "Secrets du dépôt posés : LUDO_ANDROID_KEYSTORE_B64, LUDO_ANDROID_KEYSTORE_PASSWORD, LUDO_ANDROID_KEY_ALIAS (+ variable LUDO_ANDROID_CERT_SHA256)"
fi
