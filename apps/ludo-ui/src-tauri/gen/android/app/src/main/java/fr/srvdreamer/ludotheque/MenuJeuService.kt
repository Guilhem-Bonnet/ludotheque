package fr.srvdreamer.ludotheque

import android.accessibilityservice.AccessibilityService
import android.content.ComponentName
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.view.Display
import android.view.KeyEvent
import android.view.accessibility.AccessibilityEvent

/**
 * Bouton « Mode » de la manette pendant une partie → menu de la Ludothèque (esprit du bouton HOME
 * d'une console). Facultatif : activé une fois par l'utilisateur dans l'accessibilité d'Android.
 *
 * HOME n'est PAS capté : il appartient à Android et au lanceur Cocoon (architecture décidée avec
 * Guilhem le 22/11, regles-22.md). Le retour à la Ludothèque marche aussi sans ce service : raccourci
 * de touches AYN « Lancer une appli » (Réglages AYN), HOME puis Cocoon ; l'accueil met alors le jeu
 * ouvert en tête.
 *
 * Pourquoi un service d'accessibilité : quand un émulateur est au premier plan, la Ludothèque ne
 * reçoit aucune touche. Sur Android 13 sans root, le seul moyen propre de voir un bouton de la manette
 * Odin (BTN_MODE → KEYCODE_BUTTON_MODE, keylayout Vendor_2020_Product_0111.kl relevé le 22/11) est le
 * filtre de touches d'un service d'accessibilité (`flagRequestFilterKeyEvents`).
 *
 * Portée volontairement étroite :
 * - la touche n'est retenue que si un jeu lancé par la Ludothèque ([EnCours]) est l'appli au premier
 *   plan de l'écran principal ; partout ailleurs, elle va au jeu ou au système comme avant ;
 * - le service ne lit aucun contenu d'écran (pas de `canRetrieveWindowContent`) : il ne reçoit que
 *   les changements de fenêtre (nom du paquet et de l'activité), qui disent quelle appli est devant.
 *
 * Tout est événementiel : `onAccessibilityEvent` (changement de fenêtre) et `onKeyEvent` (appui).
 */
class MenuJeuService : AccessibilityService() {
  companion object {
    /** Service lié par Android (activé dans les réglages d'accessibilité). */
    @Volatile var actif = false
      private set
    /** Touche qui ouvre le menu : « Mode » de la manette (jamais HOME, BACK ni APP_SWITCH). */
    val TOUCHES = setOf(KeyEvent.KEYCODE_BUTTON_MODE)
    /** Fenêtres qui ne changent pas l'appli au premier plan (volet, clavier, bulles d'AYN). */
    private val IGNORES = setOf("com.android.systemui", "com.google.android.inputmethod.latin", "com.odin.gameassistant")
  }

  /** Le jeu en cours est-il l'appli au premier plan de l'écran principal ? */
  @Volatile private var jeuDevant = false
  /** Touche dont l'appui a été retenu : son relâchement (et ses répétitions) le sont aussi. */
  private var retenue = -1

  override fun onServiceConnected() {
    super.onServiceConnected()
    EnCours.restaurer(this)
    actif = true
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
    // Écran du bas (Presentation, lanceur double écran) : sans effet sur le jeu de l'écran principal.
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R && e.displayId != Display.DEFAULT_DISPLAY) return
    val pkg = e.packageName?.toString() ?: return
    if (pkg in IGNORES) return
    val jeu = EnCours.jeu
    if (jeu != null && pkg == jeu.paquet) {
      jeuDevant = true
      return
    }
    // Une autre appli ne prend la place du jeu que si sa fenêtre est une vraie activité (pas une
    // bulle, un toast ou une boîte de dialogue système).
    if (pkg == packageName || estActivite(pkg, e.className?.toString())) jeuDevant = false
  }

  private fun estActivite(pkg: String, cls: String?): Boolean {
    if (cls == null) return false
    return try {
      packageManager.getActivityInfo(ComponentName(pkg, cls), 0)
      true
    } catch (e: PackageManager.NameNotFoundException) {
      false
    }
  }

  override fun onKeyEvent(ev: KeyEvent): Boolean {
    val code = ev.keyCode
    if (code !in TOUCHES) return false
    if (code == retenue) {
      if (ev.action == KeyEvent.ACTION_UP) retenue = -1
      return true
    }
    if (ev.action != KeyEvent.ACTION_DOWN || ev.repeatCount > 0) return false
    val jeu = EnCours.jeu ?: return false
    if (!jeuDevant) return false
    retenue = code
    jeuDevant = false
    EnCours.quitte()
    ouvrirMenu(jeu)
    return true
  }

  /** Ramène la Ludothèque devant (activité singleTask) ; le jeu passe en arrière-plan, en pause. */
  private fun ouvrirMenu(jeu: EnCours.Jeu) {
    val intent = Intent(this, MainActivity::class.java)
      .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT)
      .putExtra("menuJeu", jeu.game)
    startActivity(intent)
  }
}
