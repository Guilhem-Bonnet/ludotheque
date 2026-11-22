package fr.srvdreamer.ludotheque

import android.app.Activity
import android.app.ActivityManager
import android.content.ActivityNotFoundException
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.os.SystemClock
import android.provider.Settings
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

/** Jeu qui vient d'être lancé : identifiant du catalogue + intent envoyé (mêmes champs que `StartArgs`). */
@InvokeArg
class SuivreArgs {
  lateinit var game: String
  lateinit var intent: StartArgs
}

/**
 * Menu en jeu (lot menu-jeu du 22/11) : état du jeu en cours, Reprendre, Fermer le jeu, et accès au
 * réglage d'accessibilité du bouton Mode en jeu ([MenuJeuService], facultatif).
 */
@TauriPlugin
class MenuJeuPlugin(private val activity: Activity) : Plugin(activity) {
  companion object {
    /**
     * Délai de garde après le retour du jeu avant de fermer l'émulateur : laisse finir l'écriture
     * d'une sauvegarde que l'émulateur vide à sa mise en pause (melonDS écrit le fichier .sav un peu
     * après la dernière écriture du jeu). Attente UNIQUE et calculée, pas une scrutation.
     */
    const val GARDE_MS = 2500L
  }

  override fun load(webView: android.webkit.WebView) {
    super.load(webView)
    EnCours.restaurer(activity)
  }

  @Command
  fun etat(invoke: Invoke) {
    val out = JSObject()
    out.put("serviceActif", MenuJeuService.actif)
    out.put("serviceRequis", true)
    EnCours.jeu?.let { j ->
      val jeu = JSObject()
      jeu.put("game", j.game)
      jeu.put("paquet", j.paquet)
      jeu.put("depuisMs", j.depuis)
      out.put("jeu", jeu)
    }
    invoke.resolve(out)
  }

  @Command
  fun suivre(invoke: Invoke) {
    val args = invoke.parseArgs(SuivreArgs::class.java)
    val a = args.intent
    val lancement = EnCours.Lancement(a.packageName, a.activity, a.action, a.data, a.extras, a.boolExtras)
    EnCours.suivre(activity, EnCours.Jeu(args.game, lancement, System.currentTimeMillis()))
    MenuJeuService.jeuLance()
    invoke.resolve()
  }

  /** Remet le jeu devant : même intent que le lancement, donc la tâche existante revient telle quelle. */
  @Command
  fun reprendre(invoke: Invoke) {
    val j = EnCours.jeu ?: return invoke.reject("Aucun jeu en cours")
    try {
      activity.startActivity(j.lancement.intent())
      invoke.resolve()
    } catch (e: ActivityNotFoundException) {
      invoke.reject("Application absente de l'appareil : ${j.paquet}")
    } catch (e: SecurityException) {
      invoke.reject("Reprise refusée par Android : ${e.message}")
    }
  }

  /**
   * Ferme l'émulateur, alors en arrière-plan derrière la Ludothèque : `killBackgroundProcesses`
   * (permission normale KILL_BACKGROUND_PROCESSES ; Android 13 l'accepte pour un autre paquet, Android
   * 14 la limitera à l'app elle-même). Comme « Fermer le logiciel » d'une console : ce qui n'a pas été
   * sauvegardé dans le jeu est perdu, ce qui l'a été est déjà sur le stockage. La fin du processus
   * (`am_proc_died`) déclenche ensuite la synchro du PC (`ludo-thor-watch`).
   */
  @Command
  fun fermer(invoke: Invoke) {
    val j = EnCours.jeu ?: return invoke.resolve()
    val quitte = EnCours.quitteA
    Thread {
      try {
        val reste = if (quitte == 0L) GARDE_MS else quitte + GARDE_MS - SystemClock.uptimeMillis()
        if (reste > 0) Thread.sleep(reste)
        val am = activity.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
        am.killBackgroundProcesses(j.paquet)
        EnCours.oublier(activity)
        invoke.resolve()
      } catch (e: Exception) {
        invoke.reject("Fermeture impossible : ${e.message ?: e.javaClass.simpleName}")
      }
    }.start()
  }

  /** Oublie le jeu sans rien fermer (il s'est terminé tout seul). */
  @Command
  fun oublier(invoke: Invoke) {
    EnCours.oublier(activity)
    invoke.resolve()
  }

  /** Réglages d'accessibilité d'Android, le service de la Ludothèque mis en évidence. */
  @Command
  fun reglages(invoke: Invoke) {
    val cible = ComponentName(activity, MenuJeuService::class.java).flattenToString()
    val intent = Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS)
      .putExtra(":settings:fragment_args_key", cible)
      .putExtra(":settings:show_fragment_args", Bundle().apply { putString(":settings:fragment_args_key", cible) })
      .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    try {
      activity.startActivity(intent)
      invoke.resolve()
    } catch (e: ActivityNotFoundException) {
      invoke.reject("Réglages d'accessibilité introuvables")
    }
  }
}
