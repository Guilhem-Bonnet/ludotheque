package fr.srvdreamer.ludotheque

import android.content.Context
import android.content.Intent
import android.os.SystemClock

/**
 * Jeu lancé par la Ludothèque et toujours ouvert (menu en jeu, lot menu-jeu du 22/11).
 *
 * Source de vérité côté Android, partagée par le plugin (interface) et par [MenuJeuService]
 * (bouton Mode) : les deux tournent dans le même processus. Gardée aussi dans les préférences, pour
 * survivre à l'arrêt du processus de la Ludothèque par Android pendant une longue partie.
 *
 * Limite assumée : Android ne dit pas à une app sans privilège qu'un AUTRE processus s'est arrêté.
 * Si l'émulateur est fermé par son propre menu, l'état reste « en cours » jusqu'à « Fermer le jeu »
 * ou au lancement d'un autre jeu ; « Reprendre » relance alors simplement le jeu (même intent).
 */
object EnCours {
  data class Jeu(
    /** Identifiant du jeu dans le catalogue de la Ludothèque. */
    val game: String,
    /** Paquet Android de l'émulateur (ou de Moonlight). */
    val paquet: String,
    /** Intent du lancement : le renvoyer remet la tâche existante au premier plan (même filtre d'intent). */
    val intent: Intent,
    /** Heure du lancement (horloge murale, ms) : temps de la session. */
    val depuis: Long,
  )

  @Volatile var jeu: Jeu? = null
    private set

  /** Dernier passage du jeu à la Ludothèque (horloge monotone) : délai de garde avant la fermeture. */
  @Volatile var quitteA: Long = 0L
    private set

  private const val PREFS = "ludo-en-cours"

  fun suivre(ctx: Context, j: Jeu) {
    jeu = j
    quitteA = 0L
    ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
      .putString("game", j.game)
      .putString("paquet", j.paquet)
      .putString("intent", j.intent.toUri(Intent.URI_INTENT_SCHEME))
      .putLong("depuis", j.depuis)
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
    val paquet = p.getString("paquet", null) ?: return
    val uri = p.getString("intent", null) ?: return
    val intent = try { Intent.parseUri(uri, Intent.URI_INTENT_SCHEME) } catch (e: Exception) { return }
    jeu = Jeu(game, paquet, intent, p.getLong("depuis", System.currentTimeMillis()))
  }
}
