package fr.srvdreamer.ludotheque

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.provider.DocumentsContract
import android.view.InputDevice
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

/** Lancement explicite d'une activité d'une autre app (émulateur, Moonlight). */
@InvokeArg
class StartArgs {
  lateinit var packageName: String
  lateinit var activity: String
  /** Nom affiché dans les erreurs (« Azahar absent de l'appareil »). */
  var nom: String? = null
  var action: String? = null
  /** URI content de la ROM (donnée de l'intent). */
  var data: String? = null
  /** Lecture de `data` transmise à l'app lancée ; faux si l'URI est dans un arbre qu'elle détient déjà. */
  var grant: Boolean = true
  /** Fichier (chemin absolu) transmis par le FileProvider de l'app : son URI devient `data` et les extras `fileExtras`. */
  var file: String? = null
  var fileExtras: List<String> = emptyList()
  var extras: Map<String, String> = emptyMap()
  var boolExtras: Map<String, Boolean> = emptyMap()
  /** Arbre des ROM à accorder à l'app lancée (lecture, préfixe) : jeux en dossier (Cemu, Wii U). */
  var grantTree: String? = null
}

/** Adresse du fichier `android.json` des releases (réglage de l'appareil, sinon GitHub). */
@InvokeArg
class MajArgs {
  lateinit var url: String
}

/** Document (dossier d'un jeu Wii U) de l'arbre des ROM. */
@InvokeArg
class RpxArgs {
  lateinit var uri: String
}

/** Forme d'onde de vibration (rumble.rs `vibration_pattern`) : durées en ms, amplitudes 0..255. */
@InvokeArg
class VibrerArgs {
  var timings: List<Long> = emptyList()
  var amplitudes: List<Int> = emptyList()
}

/** Sous-dossiers de l'arbre des ROM à lister (« switch », « n3ds »…). */
@InvokeArg
class ListArgs {
  var dirs: List<String> = emptyList()
}

/**
 * Pont Android de la Ludothèque : intents explicites, accès aux ROM SANS jamais rien demander (accès
 * « tous les fichiers » accordé par le PC, arbre SAF accordé autrefois en repli), vibration, état des écrans.
 */
@TauriPlugin
class LudoPlugin(private val activity: Activity) : Plugin(activity) {
  private val prefs by lazy { activity.getSharedPreferences("ludo", Context.MODE_PRIVATE) }

  /** Écran du haut de la Thor (ambiance) : créé avec la WebView Tauri, piloté par le cycle de vie. */
  @Volatile private var ecrans: DoubleEcran? = null
  private lateinit var mainWebView: android.webkit.WebView
  private val maj by lazy { MiseAJourApk(activity) { js -> mainWebView.post { mainWebView.evaluateJavascript(js, null) } } }

  override fun load(webView: android.webkit.WebView) {
    super.load(webView)
    mainWebView = webView
    // Crée Android/data/<paquet>/files : le PC y dépose config.toml par adb.
    val files = activity.getExternalFilesDir(null)
    // Diagnostic sans recompiler : fichier drapeau `debug-webview` déposé par adb → inspection des
    // WebView (chrome://inspect, `adb forward … localabstract:webview_devtools_remote_<pid>`).
    if (files != null && java.io.File(files, "debug-webview").exists()) android.webkit.WebView.setWebContentsDebuggingEnabled(true)
    webView.post { ecrans = DoubleEcran(activity, webView).also { it.install() } }
  }

  /** État du jeu choisi (objet JSON quelconque), affiché par l'écran du haut s'il est actif. */
  @Command
  fun ecranHaut(invoke: Invoke) {
    val state = invoke.getRawArgs()
    val e = ecrans
    e?.push(state)
    val out = JSObject()
    out.put("actif", e?.active() == true)
    invoke.resolve(out)
  }

  /** Mode d'affichage courant (`mode`, taille de la vue en dp), lu par l'interface au démarrage. */
  @Command
  fun ecranEtat(invoke: Invoke) {
    val etat = ecrans?.etat()
    invoke.resolve(if (etat != null) JSObject(etat.toString()) else JSObject())
  }

  /** Accès aux ROM, sans rien demander : « tous les fichiers » accordé ? arbre SAF accordé autrefois ? */
  @Command
  fun accesFichiers(invoke: Invoke) {
    val out = JSObject()
    out.put("fichiers", Build.VERSION.SDK_INT >= 30 && Environment.isExternalStorageManager())
    grantedTree()?.let { out.put("arbre", it.toString()) }
    invoke.resolve(out)
  }

  /**
   * Vibration du moteur de l'appareil (Thor : moteur intégré, réglé par « Vibrator strength » d'AYN) ;
   * à défaut, celui d'une manette branchée. Appareil sans moteur : rien.
   */
  @Command
  fun vibrer(invoke: Invoke) {
    val args = invoke.parseArgs(VibrerArgs::class.java)
    val v = vibreur()
    if (v == null || args.timings.isEmpty() || args.timings.size != args.amplitudes.size) {
      invoke.resolve()
      return
    }
    val timings = args.timings.toLongArray()
    try {
      if (Build.VERSION.SDK_INT >= 26) {
        val effect = if (v.hasAmplitudeControl()) {
          VibrationEffect.createWaveform(timings, args.amplitudes.map { it.coerceIn(0, 255) }.toIntArray(), -1)
        } else {
          // Moteur tout-ou-rien : forme d'onde marche/arrêt, en commençant par une pause nulle si besoin.
          val onOff = if (args.amplitudes.first() > 0) longArrayOf(0) + timings else timings
          VibrationEffect.createWaveform(onOff, -1)
        }
        v.vibrate(effect)
      } else {
        @Suppress("DEPRECATION") v.vibrate(timings.sum())
      }
    } catch (e: Exception) {
      // Vibration refusée (réglage système) : l'interface continue sans.
    }
    invoke.resolve()
  }

  private fun vibreur(): Vibrator? {
    val own = if (Build.VERSION.SDK_INT >= 31) {
      (activity.getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as? VibratorManager)?.defaultVibrator
    } else {
      @Suppress("DEPRECATION") activity.getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
    }
    if (own?.hasVibrator() == true) return own
    for (id in InputDevice.getDeviceIds()) {
      val d = InputDevice.getDevice(id) ?: continue
      if (!d.supportsSource(InputDevice.SOURCE_GAMEPAD)) continue
      @Suppress("DEPRECATION") val pad = d.vibrator
      if (pad.hasVibrator()) return pad
    }
    return null
  }

  /** Version installée et version publiée (`android.json` des releases). */
  @Command
  fun apkVerifier(invoke: Invoke) {
    val args = invoke.parseArgs(MajArgs::class.java)
    Thread {
      try {
        invoke.resolve(maj.verifier(args.url))
      } catch (e: Exception) {
        invoke.reject(MiseAJourApk.message(e))
      }
    }.start()
  }

  /** Télécharge l'APK publié, vérifie son empreinte, puis le confie à l'installeur d'Android. */
  @Command
  fun apkInstaller(invoke: Invoke) {
    val args = invoke.parseArgs(MajArgs::class.java)
    if (!maj.autorise()) {
      maj.demanderAutorisation()
      invoke.reject(MiseAJourApk.AUTORISATION)
      return
    }
    Thread {
      try {
        maj.installer(args.url)
        invoke.resolve()
      } catch (e: Exception) {
        invoke.reject(MiseAJourApk.message(e))
      }
    }.start()
  }

  /** Fichier `.rpx` d'un jeu Wii U en dossier (fichier .rpx du sous-dossier « code » du jeu), pour Cemu. */
  @Command
  fun findRpx(invoke: Invoke) {
    val args = invoke.parseArgs(RpxArgs::class.java)
    val tree = grantedTree()
    val out = JSObject()
    if (tree != null) {
      val gameId = DocumentsContract.getDocumentId(Uri.parse(args.uri))
      val code = children(tree, gameId).firstOrNull { it.second && it.first.equals("code", ignoreCase = true) }
      val rpx = code?.let { c -> children(tree, c.third).firstOrNull { !it.second && it.first.endsWith(".rpx", ignoreCase = true) } }
      rpx?.let { out.put("uri", DocumentsContract.buildDocumentUriUsingTree(tree, it.third).toString()) }
    }
    invoke.resolve(out)
  }

  /** Petit fichier texte de l'arbre des ROM (raccourci Moonlight de Cocoon), 4 Kio au plus. */
  @Command
  fun readText(invoke: Invoke) {
    val args = invoke.parseArgs(RpxArgs::class.java)
    val out = JSObject()
    val tree = grantedTree()
    // Seulement un document de l'arbre des ROM accordé (pas file://, ni un autre fournisseur).
    if (tree != null && args.uri.startsWith(tree.toString() + "/document/")) {
      try {
        activity.contentResolver.openInputStream(Uri.parse(args.uri))?.use { s ->
          val buf = ByteArray(4096)
          var n = 0
          while (n < buf.size) {
            val r = s.read(buf, n, buf.size - n)
            if (r < 0) break
            n += r
          }
          out.put("text", String(buf, 0, n, Charsets.UTF_8))
        }
      } catch (e: Exception) {
        // Illisible : lancement par le nom seul.
      }
    }
    invoke.resolve(out)
  }

  /** Arbre des ROM accordé et toujours valable (l'utilisateur peut retirer l'accès dans les réglages). */
  private fun grantedTree(): Uri? {
    val saved = prefs.getString("roms_tree", null) ?: return null
    val uri = Uri.parse(saved)
    val ok = activity.contentResolver.persistedUriPermissions.any { it.uri == uri && it.isReadPermission }
    return if (ok) uri else null
  }

  @Command
  fun startActivity(invoke: Invoke) {
    val args = invoke.parseArgs(StartArgs::class.java)
    val nom = args.nom ?: args.packageName
    // Construction partagée avec la reprise du menu en jeu (EnCours.Lancement) : mêmes droits transmis.
    val lancement = EnCours.Lancement(args.packageName, args.activity, args.action, args.data, args.extras, args.boolExtras, args.grant, args.file, args.fileExtras)
    val intent = try {
      lancement.intent(activity)
    } catch (e: IllegalArgumentException) {
      invoke.reject("Fichier hors du stockage partagé : ${args.file}")
      return
    }
    try {
      // Comme Cocoon (« Granting tree URI permissions ») : l'émulateur lit les fichiers voisins de la ROM.
      args.grantTree?.let {
        activity.grantUriPermission(args.packageName, Uri.parse(it), Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_PREFIX_URI_PERMISSION)
      }
      // Écran principal imposé : le visuel de l'écran du bas (EcranBas) vient peut-être d'y être lancé.
      activity.startActivity(intent, EcranBas.principal())
      invoke.resolve()
    } catch (e: ActivityNotFoundException) {
      revokeTree(args)
      invoke.reject("$nom absent de l'appareil : installer l'application, puis réessayer")
    } catch (e: SecurityException) {
      revokeTree(args)
      invoke.reject("Lancement de $nom refusé par Android : ${e.message}")
    }
  }

  /** Lancement échoué : l'accès accordé à l'arbre de la console est retiré. */
  private fun revokeTree(args: StartArgs) {
    if (android.os.Build.VERSION.SDK_INT < 26) return
    args.grantTree?.let {
      try {
        activity.revokeUriPermission(args.packageName, Uri.parse(it), Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_PREFIX_URI_PERMISSION)
      } catch (e: Exception) {
        // Rien à retirer.
      }
    }
  }

  /** Fichiers des sous-dossiers demandés de l'arbre des ROM, avec leur URI content. */
  @Command
  fun listRoms(invoke: Invoke) {
    val args = invoke.parseArgs(ListArgs::class.java)
    val tree = grantedTree()
    val out = JSObject()
    out.put("granted", tree != null)
    val files = JSArray()
    if (tree != null) {
      val rootId = DocumentsContract.getTreeDocumentId(tree)
      val dirs = children(tree, rootId).filter { it.second }.associate { it.first to it.third }
      for (dir in args.dirs) {
        val dirId = dirs[dir] ?: continue
        for ((name, isDir, docId) in children(tree, dirId)) {
          val f = JSObject()
          f.put("dir", dir)
          f.put("name", name)
          f.put("isDir", isDir)
          f.put("uri", DocumentsContract.buildDocumentUriUsingTree(tree, docId).toString())
          files.put(f)
        }
      }
    }
    out.put("files", files)
    invoke.resolve(out)
  }

  /** (nom, dossier ?, identifiant de document) des enfants d'un document de l'arbre. */
  private fun children(tree: Uri, parentId: String): List<Triple<String, Boolean, String>> {
    val uri = DocumentsContract.buildChildDocumentsUriUsingTree(tree, parentId)
    val cols = arrayOf(
      DocumentsContract.Document.COLUMN_DISPLAY_NAME,
      DocumentsContract.Document.COLUMN_MIME_TYPE,
      DocumentsContract.Document.COLUMN_DOCUMENT_ID,
    )
    val out = mutableListOf<Triple<String, Boolean, String>>()
    activity.contentResolver.query(uri, cols, null, null, null)?.use { c ->
      while (c.moveToNext()) {
        out.add(Triple(c.getString(0), c.getString(1) == DocumentsContract.Document.MIME_TYPE_DIR, c.getString(2)))
      }
    }
    return out
  }
}
