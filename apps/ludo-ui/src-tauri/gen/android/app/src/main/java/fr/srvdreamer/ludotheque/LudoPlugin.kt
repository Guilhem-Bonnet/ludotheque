package fr.srvdreamer.ludotheque

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
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
  var action: String? = null
  /** URI content de la ROM (donnée de l'intent, lecture accordée à l'app lancée). */
  var data: String? = null
  var extras: Map<String, String> = emptyMap()
  var boolExtras: Map<String, Boolean> = emptyMap()
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

/** Sous-dossiers de l'arbre des ROM à lister (« switch », « n3ds »…). */
@InvokeArg
class ListArgs {
  var dirs: List<String> = emptyList()
}

/**
 * Pont Android de la Ludothèque : intents explicites, arbre SAF des ROM (choisi une fois, mémorisé
 * par takePersistableUriPermission), URI content des fichiers de ROM.
 */
@TauriPlugin
class LudoPlugin(private val activity: Activity) : Plugin(activity) {
  private val prefs by lazy { activity.getSharedPreferences("ludo", Context.MODE_PRIVATE) }

  /** Écran du haut de la Thor (ambiance) : créé avec la WebView Tauri, piloté par le cycle de vie. */
  private var ecrans: DoubleEcran? = null
  private lateinit var mainWebView: android.webkit.WebView
  private val maj by lazy { MiseAJourApk(activity) { js -> mainWebView.post { mainWebView.evaluateJavascript(js, null) } } }

  override fun load(webView: android.webkit.WebView) {
    super.load(webView)
    mainWebView = webView
    // Crée Android/data/<paquet>/files : le PC y dépose config.toml par adb.
    activity.getExternalFilesDir(null)
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

  /** Fichier `.rpx` d'un jeu Wii U en dossier (`<jeu>/code/*.rpx`), pour Cemu. */
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
    val intent = Intent(args.action ?: Intent.ACTION_VIEW)
    intent.setClassName(args.packageName, args.activity)
    args.data?.let {
      val uri = Uri.parse(it)
      intent.data = uri
      intent.clipData = ClipData.newRawUri("rom", uri)
      intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
    for ((k, v) in args.extras) intent.putExtra(k, v)
    for ((k, v) in args.boolExtras) intent.putExtra(k, v)
    intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    try {
      activity.startActivity(intent)
      invoke.resolve()
    } catch (e: ActivityNotFoundException) {
      invoke.reject("Application absente de l'appareil : ${args.packageName}")
    } catch (e: SecurityException) {
      invoke.reject("Lancement refusé par Android : ${e.message}")
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

  /** Demande l'accès au dossier des ROM (Documents/Roms proposé d'emblée), puis le mémorise. */
  @Command
  fun pickRomsTree(invoke: Invoke) {
    val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
    val initial = DocumentsContract.buildDocumentUri("com.android.externalstorage.documents", "primary:Documents/Roms")
    intent.putExtra(DocumentsContract.EXTRA_INITIAL_URI, initial)
    intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
    startActivityForResult(invoke, intent, "pickRomsTreeResult")
  }

  @ActivityCallback
  fun pickRomsTreeResult(invoke: Invoke, result: ActivityResult) {
    val uri = result.data?.data
    if (result.resultCode != Activity.RESULT_OK || uri == null) {
      invoke.reject("Accès au dossier des jeux refusé")
      return
    }
    activity.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
    prefs.edit().putString("roms_tree", uri.toString()).apply()
    val out = JSObject()
    out.put("uri", uri.toString())
    invoke.resolve(out)
  }
}
