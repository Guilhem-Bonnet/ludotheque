package fr.srvdreamer.ludotheque

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
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
 * Menu en jeu (lot menu-jeu du 22/11) : état du jeu en cours, Reprendre, Fermer le jeu.
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
    invoke.resolve()
  }

  /**
   * Remet le jeu devant comme une tuile de lanceur (Cocoon) : intent de lanceur de l'émulateur avec
   * RESET_TASK_IF_NEEDED → la tâche existante revient telle quelle, sans nouvel intent.
   *
   * Mesuré sur la Thor le 22/11 (12:41) : renvoyer l'intent du LANCEMENT (VIEW + ROM) fait recevoir à
   * Azahar (EmulationActivity) un nouveau jeu : il nettoie le jeu en cours (« Cleaning up process 11 »)
   * et reste figé, écrans noirs, 0 % de CPU. L'intent du lancement ne sert plus qu'en repli (émulateur
   * sans activité de lanceur, ou processus arrêté : le jeu est alors relancé).
   */
  @Command
  fun reprendre(invoke: Invoke) {
    val j = EnCours.jeu ?: return invoke.reject("Aucun jeu en cours")
    val lanceur = activity.packageManager.getLaunchIntentForPackage(j.paquet)
      ?.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_RESET_TASK_IF_NEEDED)
    try {
      activity.startActivity(lanceur ?: j.lancement.intent())
      invoke.resolve()
    } catch (e: ActivityNotFoundException) {
      invoke.reject("Application absente de l'appareil : ${j.paquet}")
    } catch (e: SecurityException) {
      invoke.reject("Reprise refusée par Android : ${e.message}")
    }
  }

  /**
   * Fermer le jeu, après le délai de garde (écriture d'une sauvegarde en cours) : la page « Infos de
   * l'appli » de l'émulateur s'ouvre, où « Forcer l'arrêt » ferme le jeu (une confirmation d'Android).
   *
   * Pourquoi pas un arrêt direct : mesuré sur la Thor le 22/11 (12:43:14), `killBackgroundProcesses`
   * est refusé pour un autre paquet (« ActivityManager: Invalid packageName: org.azahar_emu.azahar »,
   * permission pourtant accordée), comme sur Android 14, et Android ne dit pas à l'app si l'arrêt a eu
   * lieu. « Forcer l'arrêt » ferme le processus ET retire sa tâche (un simple arrêt laissait une tâche
   * périmée qu'Azahar faisait planter au lancement suivant, 12:49:23). Ce qui n'a pas été sauvegardé
   * dans le jeu est perdu, comme « Fermer le logiciel » d'une console ; l'arrêt (`am_kill`) déclenche
   * la synchro du PC (`ludo-thor-watch`).
   */
  @Command
  fun fermer(invoke: Invoke) {
    val j = EnCours.jeu ?: return invoke.resolve()
    val quitte = EnCours.quitteA
    Thread {
      try {
        val reste = if (quitte == 0L) GARDE_MS else quitte + GARDE_MS - SystemClock.uptimeMillis()
        if (reste > 0) Thread.sleep(reste)
        EnCours.oublier(activity)
        val infos = Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", j.paquet, null))
          .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        activity.runOnUiThread {
          try {
            activity.startActivity(infos)
          } catch (e: ActivityNotFoundException) {
            // Réglages sans page d'appli : la carte disparaît quand même, l'émulateur se ferme depuis son menu.
          }
        }
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
}
