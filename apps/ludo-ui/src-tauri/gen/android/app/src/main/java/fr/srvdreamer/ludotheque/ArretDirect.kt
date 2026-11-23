package fr.srvdreamer.ludotheque

import android.content.ComponentName
import android.content.Context
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.os.Binder
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Parcel
import android.os.SystemClock
import android.util.Log
import rikka.shizuku.Shizuku

/**
 * « Fermer le jeu » sans aucune page d'Android (lot arret, 23/11) : arrêt forcé de l'émulateur par
 * Shizuku, la Ludothèque reste devant.
 *
 * Pourquoi Shizuku : sans root, une app ne peut pas arrêter une autre app (`killBackgroundProcesses`
 * refusé, mesuré le 22/11) ; seul l'uid shell (adb) a le droit `FORCE_STOP_PACKAGES`. Shizuku (fork
 * thedjchi, démarré par le PC à la connexion de la Thor ou seul au démarrage) fait tourner un serveur à
 * l'identité shell ; la Ludothèque y lie un petit service ([ArretService], processus shell) qui exécute
 * `cmd activity force-stop <paquet>` : même effet que « Forcer l'arrêt » (processus ET tâche retirés,
 * `am_kill` dans le journal → synchro du PC), sans page ni confirmation.
 *
 * Événementiel : le binder de Shizuku arrive par `addBinderReceivedListenerSticky` (Shizuku démarré
 * avant ou après la Ludothèque), sa mort par `addBinderDeadListener` ; le service lié par
 * `ServiceConnection`. Seule attente : la liaison du service au premier arrêt, bornée par un délai de
 * garde unique ([DELAI_LIAISON_MS]). Aucune demande d'autorisation n'est affichée : l'accès est accordé
 * par le PC (adb, `shizuku.json`) ; sans lui, repli sur la page « Infos de l'appli » ([MenuJeuPlugin.fermer]).
 */
object ArretDirect {
  private const val TAG = "LudoArret"

  /** Paquet de Shizuku (fork thedjchi comme officiel). */
  const val SHIZUKU = "moe.shizuku.privileged.api"

  /** Liaison du service au premier arrêt : démarrage d'un processus `app_process` par Shizuku (~1 s). */
  private const val DELAI_LIAISON_MS = 6000L

  /** Disponibilité de l'arrêt direct, lue par l'interface (`arretDirect` de l'état du menu en jeu). */
  enum class Etat(val code: String) {
    PRET("pret"),
    ABSENT("absent"),
    ARRETE("arrete"),
    REFUSE("refuse"),
    ANCIEN("ancien"),
  }

  /** Issue d'un arrêt : [raison] (code d'[Etat] ou texte court) quand il n'a pas eu lieu. */
  data class Issue(val ok: Boolean, val raison: String? = null)

  private val verrou = Object()
  private var demarre = false
  private var appli: Context? = null
  @Volatile private var binderRecu = false
  private var service: IBinder? = null
  private var liaisonDemandee = false

  private val recu = Shizuku.OnBinderReceivedListener {
    binderRecu = true
    Log.i(TAG, "Shizuku disponible (${etat().code})")
    if (etat() == Etat.PRET) lier()
  }

  private val mort = Shizuku.OnBinderDeadListener {
    binderRecu = false
    synchronized(verrou) {
      service = null
      liaisonDemandee = false
    }
    Log.i(TAG, "Shizuku arrêté")
  }

  private val connexion = object : ServiceConnection {
    override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
      synchronized(verrou) {
        service = binder?.takeIf { it.pingBinder() }
        liaisonDemandee = service != null
        verrou.notifyAll()
      }
      Log.i(TAG, "service d'arrêt ${if (binder != null) "lié" else "refusé"}")
    }

    override fun onServiceDisconnected(name: ComponentName?) {
      synchronized(verrou) {
        service = null
        liaisonDemandee = false
      }
    }
  }

  /** Abonnement aux événements de Shizuku (une fois par processus). */
  fun demarrer(ctx: Context) {
    synchronized(verrou) {
      if (demarre) return
      demarre = true
      appli = ctx.applicationContext
    }
    Shizuku.addBinderReceivedListenerSticky(recu, Handler(Looper.getMainLooper()))
    Shizuku.addBinderDeadListener(mort, Handler(Looper.getMainLooper()))
  }

  private fun installe(): Boolean {
    val ctx = appli ?: return false
    return try {
      ctx.packageManager.getPackageInfo(SHIZUKU, 0)
      true
    } catch (e: PackageManager.NameNotFoundException) {
      false
    }
  }

  fun etat(): Etat {
    if (!binderRecu || !Shizuku.pingBinder()) return if (installe()) Etat.ARRETE else Etat.ABSENT
    return try {
      when {
        Shizuku.isPreV11() -> Etat.ANCIEN
        Shizuku.checkSelfPermission() != PackageManager.PERMISSION_GRANTED -> Etat.REFUSE
        else -> Etat.PRET
      }
    } catch (e: Exception) {
      // Binder mort entre la vérification et l'appel.
      Etat.ARRETE
    }
  }

  private fun arguments(ctx: Context): Shizuku.UserServiceArgs {
    val version = try {
      val pi = ctx.packageManager.getPackageInfo(ctx.packageName, 0)
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) pi.longVersionCode.toInt() else @Suppress("DEPRECATION") pi.versionCode
    } catch (e: Exception) {
      1
    }
    // `tag` fixe : le nom de classe change avec R8 ; `version` : une mise à jour remplace l'ancien service.
    return Shizuku.UserServiceArgs(ComponentName(ctx.packageName, ArretService::class.java.name))
      .tag("arret")
      .processNameSuffix("arret")
      .version(version)
      .daemon(false)
      .debuggable(false)
  }

  /** Lie le service d'arrêt (une demande à la fois ; la réponse arrive par [connexion]). */
  private fun lier() {
    val ctx = appli ?: return
    synchronized(verrou) {
      if (service != null || liaisonDemandee) return
      liaisonDemandee = true
    }
    try {
      Shizuku.bindUserService(arguments(ctx), connexion)
    } catch (e: Exception) {
      synchronized(verrou) { liaisonDemandee = false }
      Log.w(TAG, "liaison refusée : $e")
    }
  }

  /**
   * Arrête [paquet] (bloquant : fil de travail, jamais le fil principal). Rend l'issue ; aucune page
   * n'est ouverte ici.
   */
  fun arreter(paquet: String): Issue {
    val ctx = appli ?: return Issue(false, Etat.ARRETE.code)
    if (paquet == ctx.packageName || paquet == SHIZUKU) return Issue(false, "paquet protégé")
    val e = etat()
    if (e != Etat.PRET) return Issue(false, e.code)
    lier()
    // Délai de garde UNIQUE et calculé (liaison au premier arrêt), pas une scrutation : réveil par
    // `notifyAll` dès que le service est lié.
    val b = synchronized(verrou) {
      val fin = SystemClock.uptimeMillis() + DELAI_LIAISON_MS
      while (service == null) {
        val reste = fin - SystemClock.uptimeMillis()
        if (reste <= 0) break
        verrou.wait(reste)
      }
      // Pas lié à temps : une nouvelle liaison sera demandée au prochain arrêt (revue I2).
      if (service == null) liaisonDemandee = false
      service
    } ?: return Issue(false, "liaison").also { Log.w(TAG, "service d'arrêt non lié après $DELAI_LIAISON_MS ms") }
    val data = Parcel.obtain()
    val reply = Parcel.obtain()
    return try {
      val t0 = SystemClock.uptimeMillis()
      data.writeInterfaceToken(ArretService.DESCRIPTEUR)
      data.writeString(paquet)
      b.transact(ArretService.ARRETER, data, reply, 0)
      reply.readException()
      val code = reply.readInt()
      Log.i(TAG, "arrêt de $paquet : code $code en ${SystemClock.uptimeMillis() - t0} ms")
      if (code == 0) Issue(true) else Issue(false, "code $code")
    } catch (e: Exception) {
      Log.w(TAG, "arrêt de $paquet impossible : $e")
      Issue(false, e.javaClass.simpleName)
    } finally {
      data.recycle()
      reply.recycle()
    }
  }
}

/**
 * Service exécuté PAR SHIZUKU dans un processus à l'identité shell (uid 2000), pas dans l'app : il n'a
 * ni contexte d'application ni récepteurs. Une seule opération, `force-stop` d'un paquet au nom valide
 * (arguments passés à `cmd` sans shell : aucune injection). Constructeurs gardés par proguard-rules.pro
 * (Shizuku les appelle par réflexion).
 */
class ArretService() : Binder() {
  @Suppress("UNUSED_PARAMETER")
  constructor(context: Context) : this()

  companion object {
    const val DESCRIPTEUR = "fr.srvdreamer.ludotheque.ArretService"
    const val ARRETER = FIRST_CALL_TRANSACTION

    /** Code imposé par Shizuku pour détruire le service (`USER_SERVICE_TRANSACTION_destroy`). */
    const val DETRUIRE = 16777115
    private val NOM_PAQUET = Regex("^[A-Za-z][A-Za-z0-9_]*(\\.[A-Za-z0-9_]+)+$")
  }

  override fun onTransact(code: Int, data: Parcel, reply: Parcel?, flags: Int): Boolean {
    when (code) {
      ARRETER -> {
        data.enforceInterface(DESCRIPTEUR)
        val paquet = data.readString().orEmpty()
        val rc = if (NOM_PAQUET.matches(paquet)) forceStop(paquet) else -2
        reply?.writeNoException()
        reply?.writeInt(rc)
        return true
      }
      DETRUIRE -> {
        System.exit(0)
        return true
      }
    }
    return super.onTransact(code, data, reply, flags)
  }

  /**
   * `cmd activity force-stop` attendu jusqu'à sa fin, borné par un délai de garde unique (revue I1) :
   * au-delà, le processus est tué et l'appelant passe au repli (page d'Android).
   */
  private fun forceStop(paquet: String): Int = try {
    val p = ProcessBuilder("/system/bin/cmd", "activity", "force-stop", paquet).redirectErrorStream(true).start()
    // Délai de garde UNIQUE (10 s) : un `cmd` bloqué est tué, `waitFor` rend alors la main.
    val garde = Thread {
      try {
        Thread.sleep(10_000)
        p.destroy()
      } catch (e: InterruptedException) {
        // Fin normale avant le délai.
      }
    }.apply { isDaemon = true; start() }
    p.inputStream.use { it.readBytes() }
    val rc = p.waitFor()
    garde.interrupt()
    rc
  } catch (e: Exception) {
    -1
  }
}
