package fr.srvdreamer.ludotheque

import android.accessibilityservice.AccessibilityService
import android.app.ActivityOptions
import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
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

    /**
     * Une activité de JEU (autre que la bibliothèque de l'émulateur) a été vue depuis l'armement : seule
     * condition pour conclure « jeu quitté » quand la bibliothèque la remplace (revue I3 : après
     * « Reprendre », un émulateur qui recréerait sa bibliothèque au-dessus du jeu ne doit pas faire oublier
     * un jeu vivant).
     */
    private var jeuVu = false

    /** Verrou des champs d'état : `armer` (fil du plugin) et les événements (fil principal). */
    private val verrou = Any()

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
    fun armer() = synchronized(verrou) {
      attendu = true
      jeuVu = false
    }

    /** Lancement refusé ou jeu oublié : plus rien à attendre. */
    fun desarmer() = synchronized(verrou) {
      attendu = false
      arme = false
    }

    /**
     * Paquet dont « Fermer le jeu » vient d'ouvrir la page « Infos de l'appli » : quand Android annonce
     * son arrêt forcé (`PACKAGE_RESTARTED`), la Ludothèque revient seule sur l'accueil, sans l'appui sur
     * B qui restait nécessaire pour quitter la page des Réglages (essai Thor 0.9.0, 23/11 00:44).
     * « Annuler » ne produit aucune annonce : rien ne se passe. Effacé dès que la Ludothèque repasse devant.
     */
    @Volatile var fermeture: String? = null
      private set

    fun attendreFermeture(paquet: String) {
      fermeture = paquet
    }

    /** Délai entre la demande de retour et l'arrivée de la Ludothèque (journal seulement). */
    fun arrivee() {
      val d = demandeA
      if (d == 0L) return
      demandeA = 0L
      Log.i(TAG, "Ludothèque devant ${SystemClock.uptimeMillis() - d} ms après la demande")
    }
  }

  /**
   * Paquet du lanceur HOME par défaut (Cocoon sur la Thor), relu à chaque fenêtre candidate : un
   * changement de lanceur est suivi. Sans lanceur par défaut, Android rend le sélecteur (« android ») :
   * la règle ne se déclenche pas et le journal le dit.
   */
  private fun lanceur(): String? = packageManager
    .resolveActivity(Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_HOME), PackageManager.MATCH_DEFAULT_ONLY)
    ?.activityInfo?.packageName
    ?.takeIf { it != "android" }

  /** Dernière appli au premier plan de l'écran principal (vraie activité, hors fenêtres ignorées). */
  private var devant: String? = null

  /**
   * Arrêt forcé d'un paquet (annonce système protégée : seul Android l'envoie). Ne sert qu'à la
   * fermeture demandée par la Ludothèque ; tout autre paquet est ignoré.
   */
  private val arrets = object : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
      val pkg = intent.data?.schemeSpecificPart ?: return
      synchronized(verrou) {
        if (pkg != fermeture) return
        fermeture = null
        Log.i(TAG, "arrêt forcé de $pkg confirmé : retour à l'accueil")
        retour(ferme = true)
      }
    }
  }
  private var arretsEcoutes = false

  override fun onServiceConnected() {
    super.onServiceConnected()
    EnCours.restaurer(this)
    actif = true
    val filtre = IntentFilter(Intent.ACTION_PACKAGE_RESTARTED).apply { addDataScheme("package") }
    try {
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        registerReceiver(arrets, filtre, Context.RECEIVER_EXPORTED)
      } else {
        registerReceiver(arrets, filtre)
      }
      arretsEcoutes = true
    } catch (e: Exception) {
      Log.w(TAG, "annonce d'arrêt forcé non écoutée : $e")
    }
    Log.i(TAG, "service lié (lanceur : ${lanceur() ?: "aucun lanceur par défaut : retour inactif"})")
  }

  private fun arreterEcoute() {
    if (!arretsEcoutes) return
    arretsEcoutes = false
    try {
      unregisterReceiver(arrets)
    } catch (e: IllegalArgumentException) {
      // Déjà retiré.
    }
  }

  override fun onUnbind(intent: Intent?): Boolean {
    actif = false
    arreterEcoute()
    return super.onUnbind(intent)
  }

  override fun onDestroy() {
    actif = false
    arreterEcoute()
    super.onDestroy()
  }

  override fun onInterrupt() {}

  override fun onAccessibilityEvent(e: AccessibilityEvent) = synchronized(verrou) { surEvenement(e) }

  private fun surEvenement(e: AccessibilityEvent) {
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
      fermeture = null
      return
    }
    if (jeu != null && pkg == jeu.paquet) {
      // Jeu quitté par le menu de l'émulateur : sa bibliothèque (activité de lanceur) remplace le jeu.
      val bibliotheque = cls != null && cls != jeu.lancement.activite && cls == activiteLanceur(pkg)
      if (arme && jeuVu && avant == pkg && bibliotheque) {
        Log.i(TAG, "jeu quitté dans $pkg ($cls) : retour à l'accueil")
        retour(ferme = true)
        return
      }
      if (attendu) {
        attendu = false
        arme = true
      }
      if (cls != null && !bibliotheque && estActivite(pkg, cls)) jeuVu = true
      devant = pkg
      return
    }
    // Une autre appli ne prend la place du jeu que si sa fenêtre est une vraie activité (pas une
    // bulle, un toast ou une boîte de dialogue système).
    if (!estActivite(pkg, cls)) return
    devant = pkg
    if (arme && jeu != null && avant == jeu.paquet && pkg == lanceur()) {
      Log.i(TAG, "lanceur $pkg devant après ${jeu.paquet} : retour à « En cours »")
      retour(ferme = false)
      return
    }
    // Toute autre appli (ou le lanceur sans jeu de la Ludothèque juste avant) : la règle se désarme.
    // Le jeu n'est plus devant ni suivi : le visuel de l'écran du bas s'efface (lot ecran-bas).
    if (jeu != null && avant == jeu.paquet && pkg != lanceur()) EcranBas.retirer("$pkg devant à la place du jeu")
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
