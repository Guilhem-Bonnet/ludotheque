package fr.srvdreamer.ludotheque

import android.app.Activity
import android.app.ActivityOptions
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Color
import android.graphics.Outline
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.hardware.display.DisplayManager
import android.os.BatteryManager
import android.os.Bundle
import android.os.PowerManager
import android.util.Log
import android.util.TypedValue
import android.view.Display
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewOutlineProvider
import android.view.WindowManager
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextClock
import android.widget.TextView
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/**
 * Écran du bas pendant un jeu à UN écran lancé par la Ludothèque (lot ecran-bas, 23/11).
 *
 * Demande de Guilhem : « un visuel propice sur le screen du bas […] qu'il casse pas le jeu si on appuie
 * dessus. mais si le joueur souhaite mettre un onglet autre en bas il faut pas le bloquer non plus ».
 *
 * Mécanisme : une activité ordinaire ([EcranBasActivity]) dans sa propre tâche, lancée sur l'écran de
 * présentation (display 4 sur la Thor) JUSTE AVANT l'intent du jeu, qui part ensuite sur l'écran
 * principal et y prend le focus. Une activité plutôt qu'une Presentation : une Presentation appartient
 * à l'activité de la Ludothèque et disparaît avec elle quand le jeu passe devant ; une fenêtre
 * superposée (SYSTEM_ALERT_WINDOW) passerait au-dessus de toute autre appli mise en bas. Une activité
 * se laisse simplement recouvrir : Cocoon, la bascule AYN ou les récents mettent leur appli par-dessus,
 * et la Ludothèque ne la relance jamais d'elle-même.
 *
 * - Fenêtre `FLAG_NOT_FOCUSABLE` : jamais focalisée, donc ni la manette ni un toucher ne déplacent le
 *   focus vers l'écran du bas (Android ne donne le focus d'écran au toucher qu'à une fenêtre qui peut
 *   recevoir les touches) : le jeu garde la manette.
 * - Aucun bouton, aucune action au toucher : le toucher est absorbé sans effet.
 * - Retirée sur événements seulement : « Fermer le jeu » ([MenuJeuPlugin.fermer]), retour de la
 *   Ludothèque devant (MainActivity.onStart), autre appli devant à la place du jeu ([RetourService]),
 *   activité recouverte par une autre appli en bas (onStop, écran allumé). Aucune minuterie.
 * - Jeu lancé depuis Cocoon : la Ludothèque n'intervient pas, rien n'est affiché.
 * - Jeux à deux écrans (3DS, DS) : rien, l'émulateur peut utiliser l'écran du bas (choix fait côté Rust,
 *   `menu_jeu::ecran_bas_pour`).
 */
object EcranBas {
  const val TAG = "LudoEcranBas"

  /** Activité affichée (fil principal) : retirée par [retirer]. */
  @Volatile internal var affichee: EcranBasActivity? = null

  /** Un visuel vient d'être demandé : le lancement de jeu qui suit impose l'écran principal ([principal]). */
  @Volatile private var demande = false

  /** Écran de présentation allumé, autre que l'écran principal (même règle que DoubleEcran). */
  private fun ecranDuBas(ctx: Context): Display? {
    val displays = ctx.getSystemService(Context.DISPLAY_SERVICE) as DisplayManager
    return displays.getDisplays(DisplayManager.DISPLAY_CATEGORY_PRESENTATION)
      .firstOrNull { it.displayId != Display.DEFAULT_DISPLAY && it.state == Display.STATE_ON }
  }

  /**
   * Affiche le visuel du jeu [info] (JSON `EcranBasInfo` de menu_jeu.rs) sur l'écran du bas. Sans second
   * écran allumé : rien. Appelée avant l'intent du jeu (lancement, « Reprendre »).
   */
  fun montrer(activity: Activity, info: String) {
    // Lancement sur un écran choisi : Android 8 et plus (la Thor est en Android 13).
    if (android.os.Build.VERSION.SDK_INT < 26) return
    val display = ecranDuBas(activity)
    if (display == null) {
      Log.i(TAG, "pas d'écran du bas allumé : rien à afficher")
      return
    }
    // CLEAR_TASK : toujours une instance neuve (« Fermer et lancer » enchaîne pendant que l'ancienne se
    // retire ; une instance réutilisée garderait le titre et l'heure du jeu précédent, revue I2).
    val intent = Intent(activity, EcranBasActivity::class.java)
      .putExtra(EcranBasActivity.EXTRA_INFO, info)
      .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK or Intent.FLAG_ACTIVITY_NO_ANIMATION)
    val options = ActivityOptions.makeBasic().setLaunchDisplayId(display.displayId)
    // Appel direct, sur le fil de la commande (startActivity est permis depuis tout fil) : la demande
    // part AVANT l'intent du jeu, sans dépendre de la charge du fil principal (revue I1).
    try {
      activity.startActivity(intent, options.toBundle())
      demande = true
      Log.i(TAG, "visuel demandé sur l'écran ${display.displayId}")
    } catch (e: Exception) {
      // Écran refusé par Android (lancement sur un autre écran interdit) : le jeu part quand même.
      Log.w(TAG, "visuel refusé par Android : $e")
    }
  }

  /**
   * Options d'un lancement de jeu : écran principal imposé. Le visuel vient d'être lancé sur l'écran du
   * bas ; sans cette précision, Android pourrait choisir l'écran qui a reçu la dernière activité.
   */
  fun principal(): Bundle? {
    // Sans visuel demandé (jeu 3DS/DS, autre intent) : lancement inchangé (revue I6).
    if (!demande || android.os.Build.VERSION.SDK_INT < 26) return null
    demande = false
    return ActivityOptions.makeBasic().setLaunchDisplayId(Display.DEFAULT_DISPLAY).toBundle()
  }

  /** Retire le visuel s'il est affiché (n'importe quel fil). */
  fun retirer(raison: String) {
    val a = affichee ?: return
    a.runOnUiThread {
      if (!a.isFinishing) {
        Log.i(TAG, "visuel retiré : $raison")
        a.finishAndRemoveTask()
      }
    }
  }
}

/**
 * Visuel « En cours » de l'écran du bas : jaquette, titre, console, heure, batterie, temps de session.
 * Vue native (pas de WebView) : pendant une partie Cemu ou Eden, rien ne doit coûter de calcul.
 */
class EcranBasActivity : Activity() {
  companion object {
    const val EXTRA_INFO = "info"

    // Palette sombre de référence (règles visuelles de Guilhem) : aplats neutres, la couleur du jeu
    // n'apparaît qu'en filet fin.
    private const val FOND = 0xFF121419.toInt()
    private const val CARTE = 0xFF171A21.toInt()
    private const val TEXTE = 0xFFF5F7FA.toInt()
    private const val TEXTE_2 = 0xFFB7BFCC.toInt()
    private const val FAIBLE = 0xFFFF8A94.toInt()

    /** Décalages anti-marquage de l'écran OLED (dp), un pas par minute (annonce TIME_TICK du système). */
    private val PAS = arrayOf(0 to 0, 4 to 2, 6 to -3, 2 to -6, -4 to -4, -6 to 2, -2 to 6, 3 to 5)
  }

  private lateinit var racine: LinearLayout
  private lateinit var session: TextView
  private lateinit var batterie: TextView
  private var depuisMs = 0L
  private var pas = 0
  private var recus = false

  private fun dp(v: Number): Int = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v.toFloat(), resources.displayMetrics).toInt()

  /**
   * Annonces du système, seules sources de rafraîchissement : minute écoulée (heure affichée par
   * TextClock, temps de session, pas anti-marquage) et batterie (annonce collante ACTION_BATTERY_CHANGED).
   */
  private val annonces = object : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
      when (intent.action) {
        Intent.ACTION_TIME_TICK, Intent.ACTION_TIME_CHANGED -> minute()
        Intent.ACTION_BATTERY_CHANGED -> batterie(intent)
      }
    }
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    // Jamais focalisée : la manette et le focus d'écran restent au jeu, même après un toucher en bas.
    window.addFlags(WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL)
    window.decorView.setBackgroundColor(FOND)
    val info = try {
      JSONObject(intent.getStringExtra(EXTRA_INFO) ?: "{}")
    } catch (e: Exception) {
      JSONObject()
    }
    depuisMs = info.optLong("depuisMs", System.currentTimeMillis())
    // Garde-fous (revue I3, I4) : posée sur l'écran principal (écran du bas refusé, repli d'Android), ou
    // recréée par Android après l'arrêt du processus alors qu'aucun jeu n'est plus suivi : rien à montrer.
    EnCours.restaurer(this)
    if (ecran().displayId == Display.DEFAULT_DISPLAY || EnCours.jeu == null) {
      Log.i(EcranBas.TAG, "visuel abandonné (écran ${ecran().displayId}, jeu suivi : ${EnCours.jeu?.game})")
      finishAndRemoveTask()
      return
    }
    setContentView(construire(info))
    EcranBas.affichee = this
    Log.i(EcranBas.TAG, "visuel affiché sur l'écran ${ecran().displayId} (${info.optString("titre")})")
  }

  override fun onStart() {
    super.onStart()
    val filtre = IntentFilter().apply {
      addAction(Intent.ACTION_TIME_TICK)
      addAction(Intent.ACTION_TIME_CHANGED)
      addAction(Intent.ACTION_BATTERY_CHANGED)
    }
    // Annonces protégées du système : écoute non exportée suffisante.
    val collante = if (android.os.Build.VERSION.SDK_INT >= 33) registerReceiver(annonces, filtre, Context.RECEIVER_NOT_EXPORTED) else registerReceiver(annonces, filtre)
    recus = true
    collante?.let { batterie(it) }
    minute()
  }

  /**
   * Recouverte alors que l'écran est allumé : une autre appli a pris l'écran du bas (Cocoon, bascule AYN,
   * récents). Le visuel s'efface pour de bon : il ne revient jamais par-dessus le choix du joueur.
   * Écran éteint (veille) : il reste, pour la reprise.
   */
  override fun onStop() {
    if (recus) {
      recus = false
      try {
        unregisterReceiver(annonces)
      } catch (e: IllegalArgumentException) {
        // Déjà retiré.
      }
    }
    super.onStop()
    val allume = (getSystemService(Context.POWER_SERVICE) as PowerManager).isInteractive && ecran().state == Display.STATE_ON
    if (allume && !isFinishing && !isChangingConfigurations) {
      Log.i(EcranBas.TAG, "visuel recouvert par une autre appli : retiré")
      finishAndRemoveTask()
    }
  }

  override fun onDestroy() {
    if (EcranBas.affichee === this) EcranBas.affichee = null
    super.onDestroy()
  }

  // Retour d'Android sur l'écran du bas (geste) : le visuel s'en va, le jeu n'est pas touché.
  @Deprecated("Remplacé par OnBackPressedDispatcher, inutile ici (activité sans AndroidX)")
  override fun onBackPressed() {
    finishAndRemoveTask()
  }

  /** Écran de cette activité (`Activity.getDisplay` n'existe qu'à partir d'Android 11). */
  @Suppress("DEPRECATION")
  private fun ecran(): Display = windowManager.defaultDisplay

  private fun minute() {
    val min = ((System.currentTimeMillis() - depuisMs) / 60_000).coerceAtLeast(0)
    session.text = when {
      min < 1 -> "En jeu"
      min < 60 -> "En jeu · $min min"
      else -> "En jeu · ${min / 60} h ${(min % 60).toString().padStart(2, '0')}"
    }
    val (x, y) = PAS[pas++ % PAS.size]
    racine.translationX = dp(x).toFloat()
    racine.translationY = dp(y).toFloat()
  }

  private fun batterie(i: Intent) {
    val niveau = i.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
    val echelle = i.getIntExtra(BatteryManager.EXTRA_SCALE, 100)
    if (niveau < 0 || echelle <= 0) {
      batterie.visibility = View.GONE
      return
    }
    val pct = niveau * 100 / echelle
    val statut = i.getIntExtra(BatteryManager.EXTRA_STATUS, -1)
    val charge = statut == BatteryManager.BATTERY_STATUS_CHARGING || statut == BatteryManager.BATTERY_STATUS_FULL
    batterie.visibility = View.VISIBLE
    batterie.text = if (charge) "Batterie $pct % · en charge" else "Batterie $pct %"
    batterie.setTextColor(if (!charge && pct <= 15) FAIBLE else TEXTE_2)
  }

  private fun texte(taille: Float, couleur: Int, gras: Boolean = false): TextView = TextView(this).apply {
    setTextSize(TypedValue.COMPLEX_UNIT_SP, taille)
    setTextColor(couleur)
    gravity = Gravity.CENTER
    if (gras) typeface = Typeface.create(Typeface.DEFAULT, Typeface.BOLD)
    includeFontPadding = false
  }

  private fun couleur(hex: String?, defaut: Int): Int = try {
    if (hex.isNullOrBlank()) defaut else Color.parseColor(hex)
  } catch (e: IllegalArgumentException) {
    defaut
  }

  private fun construire(info: JSONObject): View {
    val accent = couleur(info.optString("couleur"), TEXTE_2)
    racine = LinearLayout(this).apply {
      orientation = LinearLayout.VERTICAL
      gravity = Gravity.CENTER
      setPadding(dp(32), dp(32), dp(32), dp(32))
    }

    // Jaquette : carte neutre arrondie ; l'image arrive par un seul téléchargement (délai de garde unique).
    val cote = (minOf(resources.displayMetrics.widthPixels, resources.displayMetrics.heightPixels) * 0.42f).toInt()
    val jaquette = ImageView(this).apply {
      scaleType = ImageView.ScaleType.CENTER_CROP
      background = GradientDrawable().apply { setColor(CARTE); cornerRadius = dp(18).toFloat() }
      outlineProvider = object : ViewOutlineProvider() {
        override fun getOutline(v: View, o: Outline) = o.setRoundRect(0, 0, v.width, v.height, dp(18).toFloat())
      }
      clipToOutline = true
    }
    racine.addView(jaquette, LinearLayout.LayoutParams(cote, cote))
    // Filet à la couleur du jeu : seul signe coloré de l'écran.
    racine.addView(View(this).apply {
      background = GradientDrawable().apply { setColor(accent); cornerRadius = dp(2).toFloat() }
    }, LinearLayout.LayoutParams(dp(56), dp(4)).apply { topMargin = dp(20) })

    val titre = texte(24f, TEXTE, gras = true).apply {
      text = info.optString("titre").ifBlank { "Jeu en cours" }
      maxLines = 2
      ellipsize = android.text.TextUtils.TruncateAt.END
    }
    racine.addView(titre, LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(18) })
    val soustitre = texte(15f, TEXTE_2).apply {
      text = listOf(info.optString("console"), info.optString("via")).filter { it.isNotBlank() }.joinToString(" · ")
    }
    racine.addView(soustitre, LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(8) })

    // Heure : TextClock suit les annonces d'heure du système (aucune minuterie ici).
    val heure = TextClock(this).apply {
      format24Hour = "HH:mm"
      format12Hour = "HH:mm"
      setTextSize(TypedValue.COMPLEX_UNIT_SP, 56f)
      setTextColor(TEXTE)
      gravity = Gravity.CENTER
      typeface = Typeface.create("sans-serif-light", Typeface.NORMAL)
      includeFontPadding = false
    }
    racine.addView(heure, LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(28) })

    session = texte(15f, TEXTE_2)
    racine.addView(session, LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(10) })
    batterie = texte(15f, TEXTE_2)
    racine.addView(batterie, LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(6) })
    val sauvegarde = texte(13f, TEXTE_2).apply {
      text = info.optString("sauvegarde")
      visibility = if (text.isNullOrBlank()) View.GONE else View.VISIBLE
    }
    racine.addView(sauvegarde, LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(6) })
    if (RetourService.actif) {
      racine.addView(texte(13f, TEXTE_2).apply { text = "HOME : menu de la Ludothèque" }, LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(22) })
    }

    info.optString("couverture").takeIf { it.startsWith("http") }?.let { charger(it, jaquette, cote) }

    // Toucher absorbé, sans aucune action : rien ne touche au jeu depuis l'écran du bas.
    return object : FrameLayout(this) {
      override fun onInterceptTouchEvent(ev: MotionEvent) = true

      @android.annotation.SuppressLint("ClickableViewAccessibility")
      override fun onTouchEvent(event: MotionEvent) = true
    }.apply {
      setBackgroundColor(FOND)
      addView(racine, FrameLayout.LayoutParams(-1, -1))
    }
  }

  /** Jaquette téléchargée une fois depuis le serveur de la Ludothèque (réseau local), réduite au côté affiché. */
  private fun charger(url: String, vue: ImageView, cote: Int) {
    Thread {
      val image: Bitmap? = try {
        val c = URL(url).openConnection() as HttpURLConnection
        // Délais de garde uniques : sans réponse, la carte neutre reste.
        c.connectTimeout = 4000
        c.readTimeout = 6000
        try {
          // Plafond de 8 Mo : une réponse anormale ne doit pas épuiser la mémoire pendant la partie.
          if (c.contentLengthLong > 8_000_000) throw java.io.IOException("jaquette trop grande (${c.contentLengthLong} octets)")
          val octets = c.inputStream.use { flux ->
            val tampon = java.io.ByteArrayOutputStream()
            val bloc = ByteArray(64 * 1024)
            while (true) {
              val n = flux.read(bloc)
              if (n < 0) break
              tampon.write(bloc, 0, n)
              if (tampon.size() > 8_000_000) throw java.io.IOException("jaquette trop grande")
            }
            tampon.toByteArray()
          }
          val bornes = BitmapFactory.Options().apply { inJustDecodeBounds = true }
          BitmapFactory.decodeByteArray(octets, 0, octets.size, bornes)
          var reduction = 1
          while (bornes.outWidth / (reduction * 2) >= cote && bornes.outHeight / (reduction * 2) >= cote) reduction *= 2
          BitmapFactory.decodeByteArray(octets, 0, octets.size, BitmapFactory.Options().apply { inSampleSize = reduction })
        } finally {
          c.disconnect()
        }
      } catch (e: Exception) {
        Log.w(EcranBas.TAG, "jaquette indisponible : $e")
        null
      }
      if (image != null) runOnUiThread { if (!isDestroyed) vue.setImageBitmap(image) }
    }.start()
  }
}
