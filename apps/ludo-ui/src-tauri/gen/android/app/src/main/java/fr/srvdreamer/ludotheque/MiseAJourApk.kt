package fr.srvdreamer.ludotheque

import android.app.Activity
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageInstaller
import android.net.Uri
import android.os.Build
import android.provider.Settings
import app.tauri.plugin.JSObject
import org.json.JSONObject
import java.io.File
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest

/**
 * Mise à jour de l'app Android depuis l'app elle-même.
 *
 * La CI publie, avec chaque release `app-vX.Y.Z`, un APK signé et `android.json` :
 * `{ "version": "0.7.0", "versionCode": 7000, "apk": "<url>", "sha256": "<hex>", "notes": "…" }`.
 * L'app compare `versionCode` au sien, télécharge l'APK dans son cache, vérifie l'empreinte SHA-256,
 * puis le confie à l'installeur d'Android (PackageInstaller). Android refuse de lui-même un APK signé
 * par une autre clé que l'app installée : l'empreinte protège le transfert, la signature l'origine.
 *
 * Première fois : Android demande l'autorisation « Installer des applis inconnues » pour la
 * Ludothèque (Réglages Android ouverts par l'app) ; ensuite, une simple confirmation par mise à jour.
 *
 * Progression : `window` reçoit l'événement `ludo-apk` (`{ etape, pourcent }`) par [notify], au fil
 * des octets reçus (aucune scrutation).
 */
class MiseAJourApk(private val activity: Activity, private val notify: (String) -> Unit) {

  companion object {
    const val AUCUNE_APK = "Aucune version Android publiée avec la dernière version de la Ludothèque : elle arrivera avec la prochaine."
    /** Paquet des APK publiées par la CI (variante release). */
    const val PAQUET_RELEASE = "fr.srvdreamer.ludotheque"
    const val ESSAI = "Version d'essai installée depuis le PC : ses mises à jour passent par le PC (scripts/apk.sh), pas par l'app."
    const val AUTORISATION = "Autorisation requise : dans les réglages Android ouverts, activer « Autoriser cette source » pour la Ludothèque, puis revenir et relancer la mise à jour."
    private const val ACTION_STATUT = "fr.srvdreamer.ludotheque.MAJ_STATUT"

    /** Message lisible (jamais une pile d'erreur brute en anglais). */
    fun message(e: Exception): String = when (e) {
      is MajException -> e.message ?: "Mise à jour impossible"
      is java.net.UnknownHostException, is java.net.ConnectException, is java.net.SocketTimeoutException ->
        "Serveur des mises à jour injoignable"
      is IOException -> "Téléchargement interrompu"
      else -> "Mise à jour impossible"
    }
  }

  class MajException(msg: String) : Exception(msg)

  private fun installedCode(): Long {
    val info = activity.packageManager.getPackageInfo(activity.packageName, 0)
    @Suppress("DEPRECATION")
    return if (Build.VERSION.SDK_INT >= 28) info.longVersionCode else info.versionCode.toLong()
  }

  private fun installedName(): String =
    activity.packageManager.getPackageInfo(activity.packageName, 0).versionName ?: ""

  private fun fetchJson(url: String): JSONObject {
    val c = URL(url).openConnection() as HttpURLConnection
    // Délais de garde uniques (connexion, inactivité), pas une relance.
    c.connectTimeout = 10_000
    c.readTimeout = 30_000
    c.instanceFollowRedirects = true
    try {
      // La CI ne publie l'APK (et android.json) qu'avec la clé de signature de l'app : une release
      // faite avant la clé existe pour le PC mais pas pour Android. Constat du 22/11 (app-v0.7.0,
      // clé créée ensuite).
      if (c.responseCode == 404) throw MajException(AUCUNE_APK)
      if (c.responseCode !in 200..299) throw MajException("Serveur des mises à jour : réponse ${c.responseCode}")
      return JSONObject(c.inputStream.bufferedReader().readText())
    } finally {
      c.disconnect()
    }
  }

  /**
   * Paquet de l'APK décrite par android.json (`package`, sinon celui des releases de la CI). Une APK
   * d'un autre paquet ne remplace jamais l'app installée : l'app d'essai (`….debug`, installée depuis
   * le PC) ne peut pas recevoir l'APK release, Android refuserait chaque tentative.
   */
  private fun autrePaquet(json: JSONObject): Boolean = json.optString("package", PAQUET_RELEASE) != activity.packageName

  /** `{ installee, code, version, versionCode, disponible, notes, raison? }`. */
  fun verifier(url: String): JSObject {
    val out = JSObject()
    out.put("installee", installedName())
    out.put("code", installedCode())
    // Aucune APK publiée : un état à expliquer, pas une erreur (l'app ne propose rien à installer).
    val json = try {
      fetchJson(url)
    } catch (e: MajException) {
      if (e.message != AUCUNE_APK) throw e
      out.put("version", "")
      out.put("versionCode", 0)
      out.put("notes", "")
      out.put("disponible", false)
      out.put("raison", AUCUNE_APK)
      return out
    }
    val code = json.optLong("versionCode", 0)
    out.put("version", json.optString("version"))
    out.put("versionCode", code)
    out.put("notes", json.optString("notes"))
    val autre = autrePaquet(json)
    out.put("disponible", !autre && code > installedCode())
    if (autre && code > installedCode()) out.put("raison", ESSAI)
    return out
  }

  fun autorise(): Boolean = Build.VERSION.SDK_INT < 26 || activity.packageManager.canRequestPackageInstalls()

  /** Ouvre la page Android « Installer des applis inconnues » de la Ludothèque. */
  fun demanderAutorisation() {
    if (Build.VERSION.SDK_INT < 26) return
    val intent = Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${activity.packageName}"))
    activity.runOnUiThread { activity.startActivity(intent) }
  }

  private fun progress(etape: String, pourcent: Int?) {
    val p = pourcent?.toString() ?: "null"
    notify("window.dispatchEvent(new CustomEvent('ludo-apk',{detail:{etape:'$etape',pourcent:$p}}))")
  }

  /** Télécharge, vérifie, installe. Rend la main quand Android a pris l'installation en charge. */
  fun installer(url: String) {
    val json = fetchJson(url)
    if (json.optLong("versionCode", 0) <= installedCode()) throw MajException("Déjà à jour")
    if (autrePaquet(json)) throw MajException(ESSAI)
    // HTTPS obligatoire ; HTTP accepté seulement si android.json vient lui-même d'une adresse HTTP
    // (essai local réglé dans config.toml, [maj] android). L'origine réelle est garantie par Android :
    // un APK signé par une autre clé que l'app installée est refusé.
    val apkUrl = json.optString("apk").takeIf { it.startsWith("https://") || (it.startsWith("http://") && url.startsWith("http://")) }
      ?: throw MajException("Fichier de mise à jour absent de la release")
    val expected = json.optString("sha256").lowercase().takeIf { it.length == 64 }
      ?: throw MajException("Empreinte de la mise à jour absente")

    val dir = File(activity.cacheDir, "maj").apply { mkdirs() }
    val file = File(dir, "ludotheque.apk")
    val digest = MessageDigest.getInstance("SHA-256")
    val c = URL(apkUrl).openConnection() as HttpURLConnection
    c.connectTimeout = 10_000
    c.readTimeout = 30_000
    c.instanceFollowRedirects = true
    try {
      if (c.responseCode !in 200..299) throw MajException("Téléchargement refusé (${c.responseCode})")
      val total = c.contentLengthLong
      var done = 0L
      var last = -1
      progress("telechargement", if (total > 0) 0 else null)
      c.inputStream.use { input ->
        file.outputStream().use { out ->
          val buf = ByteArray(64 * 1024)
          while (true) {
            val n = input.read(buf)
            if (n < 0) break
            out.write(buf, 0, n)
            digest.update(buf, 0, n)
            done += n
            if (total > 0) {
              val pct = (done * 100 / total).toInt()
              if (pct != last) { last = pct; progress("telechargement", pct) }
            }
          }
        }
      }
    } finally {
      c.disconnect()
    }
    val actual = digest.digest().joinToString("") { "%02x".format(it) }
    if (actual != expected) {
      file.delete()
      throw MajException("Fichier de mise à jour altéré (empreinte différente) : installation annulée")
    }
    progress("installation", 100)
    commit(file)
  }

  /** Récepteur de l'installation en cours (un seul à la fois). */
  @Volatile private var recepteur: Statut? = null

  private fun unregister(r: BroadcastReceiver) {
    try {
      activity.applicationContext.unregisterReceiver(r)
    } catch (_: IllegalArgumentException) {
      // Déjà retiré.
    }
    if (recepteur === r) recepteur = null
  }

  /** Session PackageInstaller ; la confirmation d'Android est affichée par [Statut]. */
  private fun commit(apk: File) {
    val installer = activity.packageManager.packageInstaller
    val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
    params.setAppPackageName(activity.packageName)
    params.setSize(apk.length())
    if (Build.VERSION.SDK_INT >= 31) params.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED)
    recepteur?.let { unregister(it) }
    val id = installer.createSession(params)
    val receiver = Statut(id, apk)
    try {
      installer.openSession(id).use { session ->
        session.openWrite("ludotheque.apk", 0, apk.length()).use { out ->
          apk.inputStream().use { it.copyTo(out) }
          session.fsync(out)
        }
        val filter = IntentFilter(ACTION_STATUT)
        if (Build.VERSION.SDK_INT >= 33) {
          activity.applicationContext.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
          @Suppress("UnspecifiedRegisterReceiverFlag")
          activity.applicationContext.registerReceiver(receiver, filter)
        }
        recepteur = receiver
        val intent = Intent(ACTION_STATUT).setPackage(activity.packageName)
        val flags = PendingIntent.FLAG_UPDATE_CURRENT or (if (Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else 0)
        val pending = PendingIntent.getBroadcast(activity, id, intent, flags)
        session.commit(pending.intentSender)
      }
    } catch (e: Exception) {
      unregister(receiver)
      try { installer.abandonSession(id) } catch (_: Exception) {}
      apk.delete()
      throw e
    }
  }

  /** Réponse de l'installeur : confirmation à afficher, succès (l'app est remplacée) ou échec. */
  private inner class Statut(private val session: Int, private val apk: File) : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
      // Diffusion d'une autre session (tentative précédente) : ignorée.
      if (intent.getIntExtra(PackageInstaller.EXTRA_SESSION_ID, -1) != session) return
      val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)
      if (status != PackageInstaller.STATUS_PENDING_USER_ACTION) {
        unregister(this)
        apk.delete()
      }
      when (status) {
        PackageInstaller.STATUS_PENDING_USER_ACTION -> {
          @Suppress("DEPRECATION")
          val confirm = intent.getParcelableExtra<Intent>(Intent.EXTRA_INTENT) ?: return
          confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
          activity.startActivity(confirm)
          progress("confirmation", 100)
        }
        PackageInstaller.STATUS_SUCCESS -> progress("installee", 100)
        PackageInstaller.STATUS_FAILURE_ABORTED -> progress("annulee", null)
        else -> {
          val detail = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE) ?: ""
          val etape = if (detail.contains("INSTALL_FAILED_UPDATE_INCOMPATIBLE") || detail.contains("signatures", true)) "signature" else "echec"
          progress(etape, null)
        }
      }
    }
  }
}
