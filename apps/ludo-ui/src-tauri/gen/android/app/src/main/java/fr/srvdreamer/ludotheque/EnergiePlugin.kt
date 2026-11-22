package fr.srvdreamer.ludotheque

import android.app.Activity
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.webkit.WebView
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class ReposArgs {
  var actif: Boolean = false
}

/**
 * Énergie de la Ludothèque sur Android (lot energie, 22/11) :
 *
 * - **Veille de l'app** : dès que l'activité n'est plus visible (écran éteint, jeu lancé par-dessus,
 *   retour à Cocoon), les minuteries JavaScript de toutes les WebView du processus sont suspendues
 *   (`pauseTimers`) ; elles reprennent au retour (`resumeTimers`). Wry met déjà la WebView principale
 *   en pause dans onPause ; ceci arrête aussi les `setTimeout` restants. Événement : cycle de vie.
 * - **Repos** (inactivité décidée par l'interface, `Repos.svelte`) : la luminosité de la fenêtre de
 *   l'activité (écran du HAUT en double écran, écran unique sinon) descend à [LUMINOSITE_REPOS] ;
 *   l'écran du bas, lui, est assombri par un voile noir dans la page (dalles AMOLED : le noir ne
 *   consomme presque rien). Au premier appui, tout revient instantanément.
 *
 * La batterie, elle, est lue par la page (`navigator.getBattery()`, événements levelchange /
 * chargingchange de Chromium) : aucun récepteur ici. Aucun réglage système n'est modifié : la
 * luminosité de fenêtre est une surcharge propre à l'app, rendue dès qu'elle n'est plus visible.
 */
@TauriPlugin
class EnergiePlugin(private val activity: Activity) : Plugin(activity), DefaultLifecycleObserver {
  companion object {
    /** Luminosité de repos de la fenêtre (0..1) : assez pour voir que l'app est là. */
    const val LUMINOSITE_REPOS = 0.04f
  }

  private lateinit var main: WebView
  private var repos = false

  override fun load(webView: WebView) {
    super.load(webView)
    main = webView
    (activity as LifecycleOwner).lifecycle.addObserver(this)
  }

  override fun onStart(owner: LifecycleOwner) {
    if (::main.isInitialized) main.resumeTimers()
  }

  override fun onStop(owner: LifecycleOwner) {
    appliquer(false)
    if (::main.isInitialized) main.pauseTimers()
  }

  @Command
  fun repos(invoke: Invoke) {
    val args = invoke.parseArgs(ReposArgs::class.java)
    activity.runOnUiThread {
      appliquer(args.actif)
      invoke.resolve()
    }
  }

  private fun appliquer(actif: Boolean) {
    if (actif == repos) return
    repos = actif
    val window = activity.window
    val attrs = window.attributes
    attrs.screenBrightness = if (actif) LUMINOSITE_REPOS else WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE
    window.attributes = attrs
    // Page de l'écran du haut (WebView d'ambiance dans l'activité) : attribut `data-repos` pour
    // suspendre ses éventuelles animations. La WebView principale gère le sien elle-même.
    val js = "document.documentElement.toggleAttribute('data-repos',$actif)"
    for (w in webViews(window.decorView)) if (w !== main) w.evaluateJavascript(js, null)
  }

  private fun webViews(v: View): List<WebView> = when (v) {
    is WebView -> listOf(v)
    is ViewGroup -> (0 until v.childCount).flatMap { webViews(v.getChildAt(it)) }
    else -> emptyList()
  }
}
