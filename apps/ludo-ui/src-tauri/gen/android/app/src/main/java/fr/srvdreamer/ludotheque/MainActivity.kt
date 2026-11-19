package fr.srvdreamer.ludotheque

import android.os.Bundle
import android.view.KeyEvent
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
    if (hasFocus) immersive()
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
  }

  /**
   * Boutons de la manette → touches que l'interface connaît (nav.ts) : A = Entrée, B = Échap,
   * X / Y, L1 = Q, R1 = E. La croix arrive déjà en flèches. Le bouton Retour d'Android devient
   * Échap (retour dans l'interface) au lieu de fermer l'app.
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
}
