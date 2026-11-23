package fr.srvdreamer.ludotheque

import android.os.Bundle
import android.os.SystemClock
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    immersive()
  }

  override fun onWindowFocusChanged(hasFocus: Boolean) {
    super.onWindowFocusChanged(hasFocus)
    if (hasFocus) immersive() else releaseArrows()
  }

  /** Plein écran façon console : barres système masquées, rappelées d'un glissement. */
  private fun immersive() {
    val controller = WindowCompat.getInsetsController(window, window.decorView)
    controller.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
    controller.hide(WindowInsetsCompat.Type.systemBars())
  }

  // Interface dessinée pour 1280×800 : la page impose sa largeur logique par la balise viewport
  // (App.svelte), que la WebView n'applique qu'en vue large.
  override fun onWebViewCreate(webView: WebView) {
    webView.settings.useWideViewPort = true
    webView.settings.loadWithOverviewMode = true
    webView.settings.setSupportZoom(false)
    // Sons de l'interface (Web Audio, sound.ts) : sans geste exigé par la WebView. Avec `true` (défaut),
    // le contexte audio reste suspendu tant que la WebView ne voit pas de geste qu'elle reconnaît, et
    // les touches de la manette traduites par dispatchKeyEvent n'en font pas toujours partie.
    webView.settings.mediaPlaybackRequiresUserGesture = false
  }

  /**
   * Boutons de la manette → touches que l'interface connaît (nav.ts) : A = Entrée, B = Échap,
   * X / Y, L1 = Q, R1 = E. Les touches de croix (KEYCODE_DPAD_*) passent telles quelles : la WebView
   * les donne en flèches. Le bouton Retour d'Android devient Échap (retour dans l'interface) au lieu
   * de fermer l'app.
   */
  override fun dispatchKeyEvent(event: KeyEvent): Boolean {
    val mapped = when (event.keyCode) {
      KeyEvent.KEYCODE_BUTTON_A -> KeyEvent.KEYCODE_ENTER
      KeyEvent.KEYCODE_BUTTON_B, KeyEvent.KEYCODE_BACK -> KeyEvent.KEYCODE_ESCAPE
      KeyEvent.KEYCODE_BUTTON_X -> KeyEvent.KEYCODE_X
      KeyEvent.KEYCODE_BUTTON_Y -> KeyEvent.KEYCODE_Y
      KeyEvent.KEYCODE_BUTTON_L1 -> KeyEvent.KEYCODE_Q
      KeyEvent.KEYCODE_BUTTON_R1 -> KeyEvent.KEYCODE_E
      else -> return super.dispatchKeyEvent(event)
    }
    return super.dispatchKeyEvent(KeyEvent(event.downTime, event.eventTime, event.action, mapped, event.repeatCount, event.metaState))
  }

  // Croix « chapeau » (AXIS_HAT_X/Y, manette Odin de la Thor) et stick gauche (AXIS_X/Y) : chaque
  // changement de direction devient un appui / relâchement de flèche. La répétition en maintenant
  // est cadencée par l'interface (nav.ts), pas ici. Sans cette traduction, la WebView consomme ces
  // mouvements pour l'API Gamepad et la croix ne déplace pas le focus.
  private var hatX = 0
  private var hatY = 0
  private var stickX = 0
  private var stickY = 0

  override fun dispatchGenericMotionEvent(event: MotionEvent): Boolean {
    val joystick = event.isFromSource(InputDevice.SOURCE_JOYSTICK) || event.isFromSource(InputDevice.SOURCE_GAMEPAD)
    if (!joystick || event.actionMasked != MotionEvent.ACTION_MOVE) return super.dispatchGenericMotionEvent(event)
    hatX = arrow(hatX, hat(event.getAxisValue(MotionEvent.AXIS_HAT_X)), KeyEvent.KEYCODE_DPAD_LEFT, KeyEvent.KEYCODE_DPAD_RIGHT)
    hatY = arrow(hatY, hat(event.getAxisValue(MotionEvent.AXIS_HAT_Y)), KeyEvent.KEYCODE_DPAD_UP, KeyEvent.KEYCODE_DPAD_DOWN)
    stickX = arrow(stickX, stick(event.getAxisValue(MotionEvent.AXIS_X), stickX), KeyEvent.KEYCODE_DPAD_LEFT, KeyEvent.KEYCODE_DPAD_RIGHT)
    stickY = arrow(stickY, stick(event.getAxisValue(MotionEvent.AXIS_Y), stickY), KeyEvent.KEYCODE_DPAD_UP, KeyEvent.KEYCODE_DPAD_DOWN)
    return true
  }

  /** Chapeau : -1, 0 ou 1. */
  private fun hat(v: Float): Int = if (v > 0.5f) 1 else if (v < -0.5f) -1 else 0

  /** Stick avec hystérésis : appui au-delà de 0,6, relâchement en deçà de 0,4 (comme pad.rs). */
  private fun stick(v: Float, old: Int): Int = when {
    v > 0.6f -> 1
    v < -0.6f -> -1
    kotlin.math.abs(v) < 0.4f -> 0
    else -> old
  }

  /** Changement de direction sur un axe : relâche l'ancienne flèche, appuie la nouvelle. */
  private fun arrow(old: Int, new: Int, negative: Int, positive: Int): Int {
    if (old == new) return old
    val now = SystemClock.uptimeMillis()
    if (old != 0) super.dispatchKeyEvent(KeyEvent(now, now, KeyEvent.ACTION_UP, if (old < 0) negative else positive, 0))
    if (new != 0) super.dispatchKeyEvent(KeyEvent(now, now, KeyEvent.ACTION_DOWN, if (new < 0) negative else positive, 0))
    return new
  }

  // Menu en jeu : la Ludothèque revient devant alors qu'un jeu est ouvert (bouton HOME, lanceur,
  // geste) : le jeu vient d'être quitté, ce qui fixe le délai de garde avant sa fermeture (MenuJeuPlugin).
  override fun onResume() {
    super.onResume()
    EnCours.restaurer(this)
    if (EnCours.jeu != null) EnCours.quitte()
    RetourService.arrivee()
  }

  // Fenêtre qui perd le focus (jeu lancé, autre app, autre écran) : toute flèche tenue est relâchée.
  override fun onPause() {
    releaseArrows()
    super.onPause()
  }

  private fun releaseArrows() {
    hatX = arrow(hatX, 0, KeyEvent.KEYCODE_DPAD_LEFT, KeyEvent.KEYCODE_DPAD_RIGHT)
    hatY = arrow(hatY, 0, KeyEvent.KEYCODE_DPAD_UP, KeyEvent.KEYCODE_DPAD_DOWN)
    stickX = arrow(stickX, 0, KeyEvent.KEYCODE_DPAD_LEFT, KeyEvent.KEYCODE_DPAD_RIGHT)
    stickY = arrow(stickY, 0, KeyEvent.KEYCODE_DPAD_UP, KeyEvent.KEYCODE_DPAD_DOWN)
  }
}
