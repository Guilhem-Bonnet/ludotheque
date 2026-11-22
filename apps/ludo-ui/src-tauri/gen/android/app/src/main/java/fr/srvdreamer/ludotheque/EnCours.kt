package fr.srvdreamer.ludotheque

import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.SystemClock
import android.provider.Settings
import org.json.JSONObject

/**
 * Jeu lancé par la Ludothèque et toujours ouvert (menu en jeu, lot menu-jeu du 22/11).
 *
 * Source de vérité côté Android, partagée par le plugin (interface) et par [MenuJeuService]
 * (bouton Mode) : les deux tournent dans le même processus. Gardée aussi dans les préférences, pour
 * survivre à l'arrêt du processus de la Ludothèque par Android pendant une longue partie ; oubliée
 * après un redémarrage de l'appareil (compteur de démarrages différent : plus aucun jeu n'est ouvert).
 *
 * Limite assumée : Android ne dit pas à une app sans privilège qu'un AUTRE processus s'est arrêté.
 * Si l'émulateur est fermé par son propre menu, l'état reste « en cours » jusqu'à « Fermer le jeu »
 * ou au lancement d'un autre jeu ; « Reprendre » relance alors simplement le jeu (même intent).
 */
object EnCours {
  /** Champs de l'intent de lancement (mêmes que `StartArgs`) : l'intent est reconstruit à partir d'eux. */
  data class Lancement(
    val paquet: String,
    val activite: String,
    val action: String?,
    val data: String?,
    val extras: Map<String, String>,
    val boolExtras: Map<String, Boolean>,
  ) {
    /** Même construction que `LudoPlugin.startActivity` : la reprise renvoie exactement cet intent. */
    fun intent(): Intent {
      val intent = Intent(action ?: Intent.ACTION_VIEW)
      intent.setClassName(paquet, activite)
      data?.let {
        val uri = Uri.parse(it)
        intent.data = uri
        intent.clipData = ClipData.newRawUri("rom", uri)
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
      }
      for ((k, v) in extras) intent.putExtra(k, v)
      for ((k, v) in boolExtras) intent.putExtra(k, v)
      intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
      return intent
    }

    fun json(): String = JSONObject()
      .put("paquet", paquet).put("activite", activite).put("action", action).put("data", data)
      .put("extras", JSONObject(extras as Map<*, *>)).put("boolExtras", JSONObject(boolExtras as Map<*, *>))
      .toString()

    companion object {
      fun depuis(texte: String): Lancement? = try {
        val o = JSONObject(texte)
        val ex = o.optJSONObject("extras")
        val bx = o.optJSONObject("boolExtras")
        Lancement(
          o.getString("paquet"),
          o.getString("activite"),
          o.optString("action").takeIf { o.has("action") && !o.isNull("action") },
          o.optString("data").takeIf { o.has("data") && !o.isNull("data") },
          ex?.keys()?.asSequence()?.associateWith { ex.getString(it) } ?: emptyMap(),
          bx?.keys()?.asSequence()?.associateWith { bx.getBoolean(it) } ?: emptyMap(),
        )
      } catch (e: Exception) {
        null
      }
    }
  }

  data class Jeu(
    /** Identifiant du jeu dans le catalogue de la Ludothèque. */
    val game: String,
    /** Intent du lancement : le renvoyer remet la tâche existante au premier plan (même filtre d'intent). */
    val lancement: Lancement,
    /** Heure du lancement (horloge murale, ms) : temps de la session. */
    val depuis: Long,
  ) {
    /** Paquet Android de l'émulateur (ou de Moonlight). */
    val paquet: String get() = lancement.paquet
  }

  @Volatile var jeu: Jeu? = null
    private set

  /** Dernier passage du jeu à la Ludothèque (horloge monotone) : délai de garde avant la fermeture. */
  @Volatile var quitteA: Long = 0L
    private set

  private const val PREFS = "ludo-en-cours"

  private fun demarrages(ctx: Context): Int =
    Settings.Global.getInt(ctx.contentResolver, Settings.Global.BOOT_COUNT, -1)

  fun suivre(ctx: Context, j: Jeu) {
    jeu = j
    quitteA = 0L
    ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
      .putString("game", j.game)
      .putString("lancement", j.lancement.json())
      .putLong("depuis", j.depuis)
      .putInt("demarrages", demarrages(ctx))
      .apply()
  }

  fun oublier(ctx: Context) {
    jeu = null
    quitteA = 0L
    ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().clear().apply()
  }

  /** Le jeu vient de céder la place à la Ludothèque (bouton Mode, raccourci AYN, HOME puis Cocoon). */
  fun quitte() {
    quitteA = SystemClock.uptimeMillis()
  }

  /** Relit l'état gardé (processus relancé par Android). Sans effet si l'état est déjà en mémoire. */
  fun restaurer(ctx: Context) {
    if (jeu != null) return
    val p = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
    val game = p.getString("game", null) ?: return
    // Appareil redémarré depuis le lancement : plus aucun jeu n'est ouvert.
    if (p.getInt("demarrages", -2) != demarrages(ctx)) {
      oublier(ctx)
      return
    }
    val lancement = p.getString("lancement", null)?.let { Lancement.depuis(it) } ?: return oublier(ctx)
    jeu = Jeu(game, lancement, p.getLong("depuis", System.currentTimeMillis()))
  }
}
