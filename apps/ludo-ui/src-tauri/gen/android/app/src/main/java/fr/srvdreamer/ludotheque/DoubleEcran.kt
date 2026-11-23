package fr.srvdreamer.ludotheque

import android.annotation.SuppressLint
import android.app.Activity
import android.app.Presentation
import android.content.Context
import android.graphics.Color
import android.hardware.display.DisplayManager
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.Display
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.ViewTreeObserver
import android.view.WindowManager
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.webkit.ScriptHandler
import androidx.webkit.WebViewAssetLoader
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import org.json.JSONObject

/**
 * Deux écrans de l'Ayn Thor : l'app complète (WebView Tauri, tactile, manette) passe sur l'écran du
 * BAS (« Screen-2 », 1080×1240, FLAG_PRESENTATION) dans une [Presentation] ; l'écran du HAUT (celui
 * de l'activité) reçoit une seconde WebView, d'affichage seul, qui charge la même interface en mode
 * `?ecran=haut` (EcranHaut.svelte) et reçoit l'état du jeu choisi poussé par l'écran du bas
 * (`ecranHaut` du plugin) : aucune scrutation entre les deux.
 *
 * Tout est piloté par des événements :
 * - cycle de vie de l'activité (onStart / onStop) : un émulateur lancé par-dessus récupère l'écran du
 *   bas (il en a souvent besoin : DS, 3DS) ; au retour, le double écran revient ;
 * - [DisplayManager.DisplayListener] : écran du bas éteint, rallumé, ajouté, retiré, redimensionné ;
 * - focus de la fenêtre de l'activité : relayé à la WebView du bas (sa fenêtre ne prend jamais le
 *   focus, pour que la manette arrive toujours à l'activité et à sa traduction des touches).
 *
 * Un seul écran (Screen-2 éteint ou absent) : l'app reste dans l'activité, mise en page habituelle.
 */
@SuppressLint("SetJavaScriptEnabled")
class DoubleEcran(private val activity: Activity, private val main: WebView) :
  DisplayManager.DisplayListener, DefaultLifecycleObserver {

  private val handler = Handler(Looper.getMainLooper())
  private val displays = activity.getSystemService(Context.DISPLAY_SERVICE) as DisplayManager
  private var started = false
  @Volatile private var presentation: Presentation? = null
  private var relais: Relais? = null
  private var haut: WebView? = null
  /** Place d'origine de la WebView Tauri dans l'activité (rendue à la sortie du double écran). */
  private var home: Triple<ViewGroup, Int, ViewGroup.LayoutParams>? = null
  /** Dernier état poussé vers l'écran du haut, rejoué quand sa page (re)charge. */
  @Volatile private var lastState: String? = null
  private val focusListener = ViewTreeObserver.OnWindowFocusChangeListener { focus ->
    if (presentation != null) main.onWindowFocusChanged(focus)
  }
  private var observer: ViewTreeObserver? = null

  /**
   * État annoncé à l'interface du bas : mode (`bas` | `seul`) et taille de la vue en dp. La largeur de la
   * page (balise viewport, App.svelte) se déduit de cette taille ; `window.outerWidth` ne convient pas :
   * sur la WebView Android il suit la balise viewport déjà posée (écran du bas petit à CHAQUE lancement,
   * 22/11 21:16-21:52 : mode `bas` bien appliqué, échelle ≈ 1 px CSS par pixel).
   */
  @Volatile private var etat: JSONObject = JSONObject()
  /** Script injecté au début de chaque document : l'état est connu AVANT le premier rendu. */
  private var docStart: ScriptHandler? = null
  /** Taille de la vue changée (Presentation affichée, rotation) : nouvelle annonce, sans minuterie. */
  private val layoutListener = View.OnLayoutChangeListener { _, l, t, r, b, oL, oT, oR, oB ->
    if (r - l != oR - oL || b - t != oB - oT) announce(if (presentation != null) "bas" else "seul")
  }

  /** WebView pas encore attachée au démarrage à froid : le double écran attend son attachement. */
  private val attach = object : View.OnAttachStateChangeListener {
    override fun onViewAttachedToWindow(v: View) = update()
    override fun onViewDetachedFromWindow(v: View) {}
  }

  fun install() {
    main.addOnAttachStateChangeListener(attach)
    main.addOnLayoutChangeListener(layoutListener)
    (activity as LifecycleOwner).lifecycle.addObserver(this)
    // Démarrage sur un seul écran : aucune transition n'annoncerait le mode (la page retomberait sur
    // window.outerWidth). Annonce initiale ; le double écran, s'il arrive, la remplace.
    announce(if (presentation != null) "bas" else "seul")
  }

  override fun onStart(owner: LifecycleOwner) {
    started = true
    displays.registerDisplayListener(this, handler)
    observer = activity.window.decorView.viewTreeObserver.also { it.addOnWindowFocusChangeListener(focusListener) }
    update()
  }

  override fun onStop(owner: LifecycleOwner) {
    started = false
    displays.unregisterDisplayListener(this)
    observer?.takeIf { it.isAlive }?.removeOnWindowFocusChangeListener(focusListener)
    observer = null
    leave()
  }

  override fun onDisplayAdded(displayId: Int) = update()
  override fun onDisplayRemoved(displayId: Int) = update()
  override fun onDisplayChanged(displayId: Int) = update()

  /** Écran secondaire de présentation allumé, autre que celui de l'activité. */
  private fun secondary(): Display? {
    val own = activity.windowManager.defaultDisplay.displayId
    return displays.getDisplays(DisplayManager.DISPLAY_CATEGORY_PRESENTATION)
      .firstOrNull { it.displayId != own && it.state == Display.STATE_ON }
  }

  private fun update() {
    if (!started || activity.isFinishing) return
    val target = secondary()
    val current = presentation
    if (current != null && (target == null || current.display.displayId != target.displayId)) leave()
    if (target != null && presentation == null) enter(target)
  }

  /** Mode deux écrans : WebView Tauri → écran du bas, WebView d'ambiance → activité (haut). */
  private fun enter(display: Display) {
    val parent = main.parent as? ViewGroup ?: return
    home = Triple(parent, parent.indexOfChild(main), main.layoutParams)
    val top = buildHaut()
    val box = Relais(activity, main).apply { addView(top, FrameLayout.LayoutParams(-1, -1)) }
    parent.removeView(main)
    parent.addView(box, home!!.second, home!!.third)
    box.requestFocus()

    val p = object : Presentation(activity, display) {
      override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Fenêtre jamais focalisée : la manette reste à l'activité (traduction des touches) ; le
        // toucher, lui, arrive bien à la WebView.
        window?.addFlags(WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE)
        window?.setBackgroundDrawableResource(android.R.color.black)
        val frame = FrameLayout(context)
        frame.addView(main, FrameLayout.LayoutParams(-1, -1))
        setContentView(frame)
      }
    }
    // Fermée par le système (écran retiré, taille changée) : la WebView revient à l'activité.
    p.setOnDismissListener { if (presentation === p) leave() }
    presentation = p
    relais = box
    haut = top
    try {
      p.show()
    } catch (e: WindowManager.InvalidDisplayException) {
      leave()
      return
    }
    main.requestFocus()
    main.onWindowFocusChanged(activity.hasWindowFocus())
    // Taille pas encore celle de l'écran du bas (la vue vient de changer de fenêtre) : annonce du mode seul,
    // la taille suit au premier agencement dans la Presentation (layoutListener).
    announce("bas", withSize = false)
  }

  /** Retour à un seul écran : la WebView Tauri reprend sa place dans l'activité. */
  private fun leave() {
    val p = presentation ?: return
    presentation = null
    (main.parent as? ViewGroup)?.removeView(main)
    if (p.isShowing) p.dismiss()
    haut?.let { (it.parent as? ViewGroup)?.removeView(it); it.destroy() }
    haut = null
    val box = relais
    relais = null
    home?.let { (parent, index, params) ->
      box?.let { parent.removeView(it) }
      parent.addView(main, index.coerceAtMost(parent.childCount), params)
    }
    home = null
    main.requestFocus()
    main.onWindowFocusChanged(activity.hasWindowFocus())
    announce("seul", withSize = false)
  }

  /**
   * Annonce le mode à l'interface du bas (`data-ecran` sur la racine, événement `ludo-ecran`), avec la
   * taille de la vue en dp (densité de l'écran où elle est affichée) ; rejouée à chaque nouveau document
   * par un script de début de document, et lisible à tout moment par `ecranEtat` (LudoPlugin).
   */
  private fun announce(mode: String, withSize: Boolean = true) {
    val info = JSONObject().put("mode", mode)
    val disp = main.display
    if (withSize && main.width > 0 && main.height > 0 && disp != null) {
      val density = activity.createDisplayContext(disp).resources.displayMetrics.density
      info.put("vueLargeurDp", Math.round(main.width / density)).put("vueHauteurDp", Math.round(main.height / density)).put("densite", density.toDouble())
    }
    if (info.toString() == etat.toString()) return
    etat = info
    // `document.documentElement` peut manquer au tout début du document : attribut posé dès qu'il existe.
    val js = "window.__ludoEcran=$info;(function(){var s=function(){var r=document.documentElement;" +
      "if(r)r.dataset.ecran='$mode';return!!r};if(!s())document.addEventListener('readystatechange',s,{once:true})})();"
    if (WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) {
      docStart?.remove()
      // Seule la WebView du bas reçoit ce script ; elle ne charge que l'interface de l'app.
      docStart = WebViewCompat.addDocumentStartJavaScript(main, js, setOf("*"))
    }
    // Sans taille (la vue vient de changer de fenêtre) : pas d'événement, le recalcul attend la vraie taille
    // (layoutListener), sinon la page repasserait un instant par window.outerWidth.
    val event = if (info.has("vueLargeurDp") || mode == "seul") "window.dispatchEvent(new CustomEvent('ludo-ecran',{detail:window.__ludoEcran}));" else ""
    main.evaluateJavascript(js + event, null)
  }

  /** Dernier état annoncé (copie). */
  fun etat(): JSONObject = JSONObject(etat.toString())

  /** État du jeu choisi (JSON) poussé par l'interface du bas ; rejoué au chargement du haut. */
  fun push(state: String) {
    lastState = state
    handler.post { deliver() }
  }

  /** Vrai si l'écran du haut affiche l'ambiance (deux écrans actifs). */
  fun active(): Boolean = presentation != null

  private fun deliver() {
    val state = lastState ?: return
    haut?.evaluateJavascript("window.__ludoHaut&&window.__ludoHaut($state)", null)
  }

  /**
   * WebView du haut : même origine que la WebView Tauri (`http://tauri.localhost`), requêtes servies
   * par le même moteur (`Rust.handleRequest` avec l'identifiant de la WebView Tauri : fichiers de
   * l'interface et protocole `asset:` des jaquettes). Affichage seul : ni focus ni toucher.
   */
  private fun buildHaut(): WebView {
    val rust = main as? RustWebView
    val origin = main.url?.let { android.net.Uri.parse(it) }?.takeIf { it.host != null }
      ?.let { "${it.scheme}://${it.host}${if (it.port > 0) ":" + it.port else ""}" } ?: "http://tauri.localhost"
    val loader = rust?.let { Rust.assetLoaderDomain(it.id) }?.let { domain ->
      WebViewAssetLoader.Builder().setDomain(domain).addPathHandler("/", WebViewAssetLoader.AssetsPathHandler(activity)).build()
    }
    val view = WebView(activity)
    view.setBackgroundColor(Color.BLACK)
    view.isVerticalScrollBarEnabled = false
    view.isHorizontalScrollBarEnabled = false
    view.overScrollMode = View.OVER_SCROLL_NEVER
    view.isFocusable = false
    view.isFocusableInTouchMode = false
    view.settings.javaScriptEnabled = true
    view.settings.domStorageEnabled = true
    view.settings.mediaPlaybackRequiresUserGesture = false
    view.settings.useWideViewPort = true
    view.settings.loadWithOverviewMode = true
    view.settings.setSupportZoom(false)
    view.webViewClient = object : WebViewClient() {
      override fun shouldInterceptRequest(v: WebView, request: WebResourceRequest): WebResourceResponse? {
        if (rust == null) return null
        if (loader != null) return loader.shouldInterceptRequest(request.url)
        // `false` : le moteur injecte lui-même ses scripts d'initialisation dans la page.
        return Rust.handleRequest(rust.id, request, false)
      }

      // Lien ou redirection hors de l'interface : jamais ouvert dans l'écran du haut.
      override fun shouldOverrideUrlLoading(v: WebView, request: WebResourceRequest): Boolean =
        !request.url.toString().startsWith(origin)

      override fun onPageFinished(v: WebView, url: String) = deliver()
    }
    view.loadUrl("$origin/?ecran=haut")
    return view
  }

  /**
   * Conteneur de l'écran du haut dans l'activité : il garde le focus de la fenêtre et transmet chaque
   * touche (déjà traduite par MainActivity) à la WebView du bas ; il absorbe le toucher du haut.
   */
  @SuppressLint("ViewConstructor")
  private class Relais(context: Context, private val target: WebView) : FrameLayout(context) {
    init {
      isFocusable = true
      isFocusableInTouchMode = true
      descendantFocusability = ViewGroup.FOCUS_BLOCK_DESCENDANTS
    }

    override fun dispatchKeyEvent(event: KeyEvent): Boolean = target.dispatchKeyEvent(event)

    override fun onInterceptTouchEvent(ev: MotionEvent): Boolean = true

    @SuppressLint("ClickableViewAccessibility")
    override fun onTouchEvent(event: MotionEvent): Boolean = true

    override fun dispatchGenericMotionEvent(event: MotionEvent): Boolean = target.dispatchGenericMotionEvent(event)
  }
}
