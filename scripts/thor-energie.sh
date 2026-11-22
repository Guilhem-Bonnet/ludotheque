#!/usr/bin/env bash
# Énergie de l'Ayn Thor (ou de tout appareil Android relié par adb) : relevés et réglages RÉVERSIBLES.
# Lecture seule sauf `doze activer|retour`. Ne touche à aucun réglage permanent.
#
#   scripts/thor-energie.sh releve <fichier>       instantané : batterie, écran, suspensions, sources de réveil
#   scripts/thor-energie.sh ecart <avant> <apres>  écart entre deux instantanés : suspensions/h, réveils/h par source, %/h
#   scripts/thor-energie.sh historique             décharge quotidienne écran éteint / allumé (%/h) des « Daily stats »
#   scripts/thor-energie.sh doze etat|activer|retour
#       activer : `dumpsys deviceidle enable all` (Doze, veille profonde d'Android) ; retour : `disable all`.
#       Volatil : perdu au redémarrage de la Thor (la ROM AYN livre config_enableAutoPowerModes=false).
#
# Appareil : $LUDO_ADB_SERIAL (défaut : seul appareil adb). Jamais de boucle d'attente : deux relevés
# encadrent une période (écran éteint, par exemple), c'est l'utilisateur ou un événement qui les déclenche.
# Attention : `cat /sys/power/wakeup_count` BLOQUE tant qu'un réveil est en cours ; ce script ne le lit pas.
set -euo pipefail

adb_() { if [[ -n "${LUDO_ADB_SERIAL:-}" ]]; then adb -s "$LUDO_ADB_SERIAL" "$@"; else adb "$@"; fi; }

releve() {
  local out=$1
  adb_ shell '
    echo "horloge $(date +%s)"
    echo "boottime $(cut -d" " -f1 /proc/uptime)"
    echo "niveau $(cat /sys/class/power_supply/battery/capacity)"
    echo "charge_uah $(cat /sys/class/power_supply/battery/charge_counter 2>/dev/null || echo 0)"
    echo "statut $(cat /sys/class/power_supply/battery/status)"
    echo "ecran $(dumpsys power | grep -o "mWakefulness=[A-Za-z]*" | head -1 | cut -d= -f2)"
    echo "doze $(dumpsys deviceidle | grep -m1 -o "mState=[A-Z_]*" | cut -d= -f2) $(dumpsys deviceidle | grep -m1 -o "mLightState=[A-Z_]*" | cut -d= -f2)"
    echo "suspensions $(cat /sys/power/suspend_stats/success)"
    echo "echecs $(cat /sys/power/suspend_stats/fail)"
    cd /sys/class/wakeup && for w in *; do
      a=$(cat $w/active_count 2>/dev/null) || continue
      [ "$a" = 0 ] && continue
      echo "source $w $(cat $w/wakeup_count) $a $(cat $w/active_time_ms) $(cat $w/name)"
    done
  ' > "$out"
  echo "relevé écrit : $out ($(grep -c '^source' "$out") sources actives)"
}

ecart() {
  python3 - "$1" "$2" <<'EOF'
import sys
def lire(p):
    d, src = {}, {}
    for l in open(p):
        k, _, v = l.rstrip("\n").partition(" ")
        if k == "source":
            w, wc, ac, at, *nom = v.split(" ")
            src[w] = (int(wc), int(ac), int(at), " ".join(nom))
        else:
            d[k] = v
    return d, src
(a, sa), (b, sb) = lire(sys.argv[1]), lire(sys.argv[2])
h = (float(b["boottime"]) - float(a["boottime"])) / 3600
print(f"période : {h*60:.1f} min ; écran {a['ecran']} → {b['ecran']} ; doze {a['doze']} → {b['doze']} ; statut {a['statut']} → {b['statut']}")
dn = int(a["niveau"]) - int(b["niveau"])
dq = (int(a["charge_uah"]) - int(b["charge_uah"])) / 1000
print(f"batterie : {a['niveau']} → {b['niveau']} % ({dn/h:.2f} %/h) ; compteur de charge {dq:+.1f} mAh ({dq/h:.1f} mAh/h)")
ds = int(b["suspensions"]) - int(a["suspensions"])
print(f"suspensions réussies : {ds} ({ds/h:.0f}/h) ; échecs : {int(b['echecs']) - int(a['echecs'])}")
rows = []
for w, (wc, ac, at, nom) in sb.items():
    wc0, ac0, at0, _ = sa.get(w, (0, 0, 0, nom))
    if wc - wc0 or ac - ac0:
        rows.append((wc - wc0, ac - ac0, at - at0, nom or w))
rows.sort(reverse=True)
print("source de réveil : réveils (/h) · activations · temps actif")
for wc, ac, at, nom in rows[:15]:
    print(f"  {nom:28s} {wc:7d} ({wc/h:6.0f}/h) · {ac:7d} · {at/1000:7.1f} s")
EOF
}

historique() {
  adb_ shell dumpsys batterystats | python3 -c '
import re, sys
txt = sys.stdin.read()
def h(s):
    return sum(int(v) * {"d": 24, "h": 1, "m": 1 / 60, "s": 1 / 3600}[u] for v, u in re.findall(r"(\d+)(d|h|m|s)(?![a-z])", s))
print("période | écran éteint %/h | écran allumé %/h")
for b in re.split(r"\n  Daily from ", txt)[1:]:
    per = b.split(":\n")[0]
    f = lambda k: (lambda m: f"{100 / h(m.group(1)):.2f} ({m.group(2)} pas)" if m else "—")(re.search(rf"Discharge screen {k} time: ([^(]+)\(from (\d+) steps", b))
    off, on = f("off"), f("on")
    print(f"{per} | {off} | {on}")
'
}

case "${1:-}" in
  releve) releve "${2:?fichier de sortie}" ;;
  ecart) ecart "${2:?avant}" "${3:?après}" ;;
  historique) historique ;;
  doze)
    case "${2:-etat}" in
      etat) adb_ shell dumpsys deviceidle | grep -E 'mLightEnabled|mState=' ;;
      activer) adb_ shell dumpsys deviceidle enable all ;;
      retour) adb_ shell dumpsys deviceidle disable all ;;
      *) echo "doze etat|activer|retour" >&2; exit 2 ;;
    esac ;;
  *) sed -n '2,15p' "$0"; exit 2 ;;
esac
