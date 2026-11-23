package fr.srvdreamer.ludotheque

import android.accessibilityservice.AccessibilityService
import android.app.ActivityOptions
import android.content.ComponentName
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.SystemClock
import android.util.Log
import android.view.Display
import android.view.accessibility.AccessibilityEvent

/**
 * Retour à la Ludothèque en un appui après une partie qu'elle a lancée (lot menu-jeu2, 22/11 au soir).
 *
 * Architecture inchangée : Android → Cocoon (lanceur HOME) → Ludothèque (app). La Ludothèque ne capte
 * aucune touche. Une seule règle, événementielle : **si le lanceur passe au premier plan de l'écran
 * principal juste après le jeu que la Ludothèque a mis devant, la Ludothèque se remet devant**, sur la
 * carte « En cours ». Cela couvre, mesuré sur la Thor le 22/11 (22:01-22:03, `wm_set_resumed_activity`) :
 * - bouton Mode en jeu : le système envoie l'intent HOME (uid 0) → Cocoon ;
 * - HOME en un appui (touche de la manette Odin) → Cocoon ;
 * - jeu qui se termine alors qu'il est devant (arrêt, plantage, fin de tâche) → Cocoon reprend.
 * Un jeu quitté par le menu de l'émulateur qui montre sa bibliothèque (activité de lanceur du même
 * paquet) ramène aussi la Ludothèque, sur l'accueil : le jeu est fermé.
 *
 * Jamais intercepté : un jeu lancé depuis Cocoon (la règle n'est armée que par un lancement ou une
 * reprise faits par la Ludothèque, et désarmée dès que la Ludothèque ou une autre appli passe devant) ;
 * B ou HOME depuis la Ludothèque (l'appli devant est alors la Ludothèque, pas le jeu).
 *
 * Portée étroite : seuls les changements de fenêtre (paquet + classe de l'activité) sont reçus ; aucune
 * touche (pas de filtre de touches), aucun contenu d'écran (`canRetrieveWindowContent` absent).
 * Activé par le PC (adb : `scripts/apk.sh`, `ludo adb-sync`), à côté du service d'AYN, jamais à sa place.
 *
 * Mise au premier plan : `startActivity` depuis ce service lié par le système (exemption d'Android aux
 * lancements en arrière-plan pour un service d'accessibilité lié), et `SYSTEM_ALERT_WINDOW` accordé par
 * adb en second filet. Journal `LudoRetour` (logcat) : heure de la demande et délai jusqu'au retour
 * effectif (`MainActivity.onResume`), pour mesurer le passage visible par Cocoon.
 */
class RetourService : AccessibilityService() {
  companion object {
    const val TAG = "LudoRetour"

    /** Service lié par Android (activé dans les réglages d'accessibilité). */
    @Volatile var actif = false
      private set

    /** La règle est armée : le jeu en cours a été mis devant par la Ludothèque et l'est resté. */
    @Volatile var arme = false
      private set

    /**
     * Lancement ou reprise demandés par la Ludothèque : la règle s'arme quand la fenêtre du jeu arrive
     * (un changement de fenêtre tardif de la Ludothèque ne la désarme donc pas pendant le lancement).
     */
    @Volatile private var attendu = false

    /** Heure (horloge monotone) de la dernière demande de retour : délai mesuré à l'arrivée. */
    @Volatile var demandeA = 0L

    /** Fenêtres qui ne changent pas l'appli au premier plan (volet, clavier, surcouches AYN). */
    private val IGNORES = setOf(
      "com.android.systemui",
      "com.google.android.inputmethod.latin",
      "com.odin.gameassistant",
      "com.odin.dualscreen.assistant",
    )

    /** Un jeu vient d'être lancé ou repris par la Ludothèque : il va passer devant. */
    fun armer() {
      attendu = true
    }

    /** Délai entre la demande de retour et l'arrivée de la Ludothèque (journal seulement). */
    fun arrivee() {
      val d = demandeA
      if (d == 0L) return
      demandeA = 0L
      Log.i(TAG, "Ludothèque devant ${SystemClock.uptimeMillis() - d} ms après la demande")
    }
  }

  /** Paquet du lanceur HOME (Cocoon sur la Thor), relu à la liaison du service. */
  private var lanceur: String? = null

  /** Dernière appli au premier plan de l'écran principal (vraie activité, hors fenêtres ignorées). */
  private var devant: String? = null

  override fun onServiceConnected() {
    super.onServiceConnected()
    EnCours.restaurer(this)
    lanceur = packageManager
      .resolveActivity(Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_HOME), PackageManager.MATCH_DEFAULT_ONLY)
      ?.activityInfo?.packageName
    actif = true
    Log.i(TAG, "service lié (lanceur : $lanceur)")
  }

  override fun onUnbind(intent: Intent?): Boolean {
    actif = false
    return super.onUnbind(intent)
  }

  override fun onDestroy() {
    actif = false
    super.onDestroy()
  }

  override fun onInterrupt() {}

  override fun onAccessibilityEvent(e: AccessibilityEvent) {
    if (e.eventType != AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED) return
    // Écran du bas (Presentation, activité « externe » de Cocoon) : sans effet sur l'écran principal.
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R && e.displayId != Display.DEFAULT_DISPLAY) return
    val pkg = e.packageName?.toString() ?: return
    if (pkg in IGNORES) return
    val cls = e.className?.toString()
    val avant = devant
    val jeu = EnCours.jeu

    if (pkg == packageName) {
      // La Ludothèque est devant : B ou HOME depuis ici mènent au lanceur sans redirection.
      devant = pkg
      arme = false
      return
    }
    if (jeu != null && pkg == jeu.paquet) {
      // Jeu quitté par le menu de l'émulateur : sa bibliothèque (activité de lanceur) remplace le jeu.
      if (arme && avant == pkg && cls != null && cls != jeu.lancement.activite && cls == activiteLanceur(pkg)) {
        Log.i(TAG, "jeu quitté dans $pkg ($cls) : retour à l'accueil")
        retour(ferme = true)
        return
      }
      if (attendu) {
        attendu = false
        arme = true
      }
      devant = pkg
      return
    }
    // Une autre appli ne prend la place du jeu que si sa fenêtre est une vraie activité (pas une
    // bulle, un toast ou une boîte de dialogue système).
    if (!estActivite(pkg, cls)) return
    devant = pkg
    if (pkg == lanceur && arme && jeu != null && avant == jeu.paquet) {
      Log.i(TAG, "lanceur $pkg devant après ${jeu.paquet} : retour à « En cours »")
      retour(ferme = false)
      return
    }
    // Toute autre appli (ou le lanceur sans jeu de la Ludothèque juste avant) : la règle se désarme.
    arme = false
    attendu = false
  }

  private fun activiteLanceur(pkg: String): String? =
    packageManager.getLaunchIntentForPackage(pkg)?.component?.className

  private fun estActivite(pkg: String, cls: String?): Boolean {
    if (cls == null) return false
    return try {
      packageManager.getActivityInfo(ComponentName(pkg, cls), 0)
      true
    } catch (e: PackageManager.NameNotFoundException) {
      false
    }
  }

  /**
   * Remet la Ludothèque devant sur l'écran principal (activité singleTask : la tâche existante revient,
   * sans animation pour raccourcir le passage par le lanceur). [ferme] : le jeu est terminé, il est
   * oublié avant (l'accueil s'ouvre sans carte « En cours »).
   */
  private fun retour(ferme: Boolean) {
    arme = false
    attendu = false
    if (ferme) EnCours.oublier(this) else EnCours.quitte()
    demandeA = SystemClock.uptimeMillis()
    val intent = Intent(this, MainActivity::class.java)
      .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT)
    val options = ActivityOptions.makeCustomAnimation(this, 0, 0)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) options.setLaunchDisplayId(Display.DEFAULT_DISPLAY)
    try {
      startActivity(intent, options.toBundle())
      Log.i(TAG, "retour demandé (jeu ${if (ferme) "fermé" else "en cours"})")
    } catch (e: Exception) {
      demandeA = 0L
      Log.w(TAG, "retour refusé par Android : $e")
    }
  }
}
