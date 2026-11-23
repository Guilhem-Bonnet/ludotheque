//! Menu en jeu : pendant une partie, retour à l'accueil de la Ludothèque avec le jeu ouvert en tête,
//! « Reprendre » ou « Fermer le jeu » (lot menu-jeu du 22/11/2026).
//!
//! - **Android** : la Ludothèque ne capte aucune touche système (HOME appartient à Cocoon) ; elle
//!   revient devant par le raccourci de touches AYN « Start app », ou HOME puis Cocoon. L'état du jeu
//!   en cours vit côté Kotlin (`EnCours.kt`) ; [`lance`] l'alimente juste avant l'intent de lancement
//!   (`android_launch`, lib.rs) : c'est la source de vérité du jeu lancé.
//! - **Bureau** : la partie est un processus enfant de l'app (`launch.rs`) ; « Fermer le jeu » le
//!   termine proprement (SIGTERM au groupe), la sauvegarde part ensuite comme après toute partie.

use serde::{Deserialize, Serialize};

/// Bureau : bouton Guide pressé pendant une partie (pad.rs) ; l'écran « En jeu » met ses actions en avant.
pub const EVENT_MENU: &str = "ludo://menu-jeu";

/// Jeu ouvert, tel que l'interface l'affiche (`JeuEnCours` de `enCours.svelte.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JeuEnCours {
    pub game: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paquet: Option<String>,
    /// Heure du lancement (ms depuis l'époque Unix).
    pub depuis_ms: u64,
}

/// État du menu en jeu (`EtatMenu` de `enCours.svelte.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EtatMenu {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jeu: Option<JeuEnCours>,
    /// Android : service de retour après une partie lié (`RetourService.kt`, activé par le PC).
    #[serde(default)]
    pub retour_actif: bool,
    /// Android : arrêt d'un jeu sans page d'Android (`ArretDirect.kt`) : `pret`, ou la raison de son
    /// absence (`absent`, `arrete`, `refuse`, `ancien` : état de Shizuku).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arret_direct: Option<String>,
}

/// Issue de « Fermer le jeu » (`Fermeture` de `enCours.svelte.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fermeture {
    /// Jeu arrêté sans aucune page (Shizuku, ou bureau) : la suite (« Fermer et lancer ») part aussitôt.
    /// Faux : la page « Infos de l'appli » d'Android s'est ouverte, la suite attend le retour ici.
    pub direct: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raison: Option<String>,
}

#[cfg(target_os = "android")]
mod pont {
    use super::*;
    use crate::android::StartActivity;
    use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
    use tauri::{Manager, Wry};

    /// Plugin Kotlin `MenuJeuPlugin` (gen/android/app/src/main/java/fr/srvdreamer/ludotheque).
    pub struct Pont(PluginHandle<Wry>);

    pub fn plugin() -> TauriPlugin<Wry> {
        Builder::new("menu-jeu")
            .setup(|app, api| {
                let handle = api.register_android_plugin("fr.srvdreamer.ludotheque", "MenuJeuPlugin")?;
                app.manage(Pont(handle));
                Ok(())
            })
            .build()
    }

    fn appel<R: serde::de::DeserializeOwned>(app: &tauri::AppHandle, cmd: &str, args: impl Serialize) -> Result<R, String> {
        let pont = app.try_state::<Pont>().ok_or_else(|| "Pont Android indisponible".to_string())?;
        pont.0.run_mobile_plugin(cmd, args).map_err(|e| match e {
            tauri::plugin::mobile::PluginInvokeError::InvokeRejected(r) => r.message.unwrap_or_else(|| "Action impossible".into()),
            other => other.to_string(),
        })
    }

    #[derive(Serialize)]
    struct Suivre<'a> {
        game: &'a str,
        intent: &'a StartActivity,
    }

    /// Retient le jeu qui va être lancé (juste avant `android::start`). Une erreur ici n'empêche pas
    /// la partie : seul le menu en jeu manquera.
    pub fn lance(app: &tauri::AppHandle, game: &str, intent: &StartActivity) {
        if let Err(e) = appel::<serde_json::Value>(app, "suivre", Suivre { game, intent }) {
            eprintln!("ludo-ui : menu en jeu indisponible pour {game} : {e}");
        }
    }

    /// Lancement refusé par Android : rien n'est ouvert.
    pub fn oublier(app: &tauri::AppHandle) {
        let _ = appel::<serde_json::Value>(app, "oublier", ());
    }

    pub fn etat(app: &tauri::AppHandle) -> Result<EtatMenu, String> {
        appel(app, "etat", ())
    }

    pub fn action(app: &tauri::AppHandle, cmd: &str) -> Result<(), String> {
        appel::<serde_json::Value>(app, cmd, ()).map(|_| ())
    }

    pub fn fermer(app: &tauri::AppHandle) -> Result<Fermeture, String> {
        appel(app, "fermer", ())
    }
}

#[cfg(target_os = "android")]
pub use pont::{lance, oublier, plugin};

/// État du menu en jeu : jeu ouvert (Android).
#[tauri::command(rename = "jeuEnCours")]
pub async fn jeu_en_cours(app: tauri::AppHandle) -> Result<EtatMenu, String> {
    #[cfg(target_os = "android")]
    return crate::blocking(move || pont::etat(&app)).await;
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        // Bureau : l'écran « En jeu » (Passage) porte déjà la partie en cours.
        Ok(EtatMenu { jeu: None, retour_actif: false, arret_direct: None })
    }
}

/// Remet le jeu devant. Android : intent de lanceur de l'émulateur (la tâche existante revient telle
/// quelle, sans nouvel intent de jeu). Bureau : réduit la fenêtre de la Ludothèque, le jeu est juste derrière.
#[tauri::command(rename = "reprendreJeu")]
pub async fn reprendre_jeu(app: tauri::AppHandle, window: tauri::Window) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        let _ = window;
        return crate::blocking(move || pont::action(&app, "reprendre")).await;
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        window.minimize().map_err(|e| e.to_string())
    }
}

/// Ferme le jeu ouvert. Android : après un court délai de garde, arrêt forcé de l'émulateur par
/// Shizuku sans aucune page (`direct`), sinon la page « Infos de l'appli » de l'émulateur s'ouvre
/// (« Forcer l'arrêt ») ; l'arrêt (`am_kill`) déclenche la synchro du PC. Bureau : SIGTERM à
/// l'émulateur, la sauvegarde part ensuite comme après toute partie (écran « En jeu »).
#[tauri::command(rename = "fermerJeu")]
pub async fn fermer_jeu(app: tauri::AppHandle) -> Result<Fermeture, String> {
    #[cfg(target_os = "android")]
    return crate::blocking(move || pont::fermer(&app)).await;
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        ludo_client::launch::terminate_running();
        Ok(Fermeture { direct: true, raison: None })
    }
}

/// B à la racine de l'interface. Android : la Ludothèque passe derrière et le lanceur (Cocoon) revient,
/// sans fermer l'app. Bureau : rien (la fenêtre reste, comme avant).
#[tauri::command(rename = "versLanceur")]
pub async fn vers_lanceur(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "android")]
    return crate::blocking(move || pont::action(&app, "versLanceur")).await;
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etat_pour_l_interface() {
        let e = EtatMenu {
            jeu: Some(JeuEnCours { game: "mm3d".into(), paquet: Some("io.github.lime3ds.android".into()), depuis_ms: 1_795_000_000_000 }),
            retour_actif: true,
            arret_direct: Some("pret".into()),
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["jeu"]["game"], "mm3d");
        assert_eq!(v["jeu"]["depuisMs"], 1_795_000_000_000u64);
        assert_eq!(v["retourActif"], true);
        assert_eq!(v["arretDirect"], "pret");
        // Réponse du plugin Kotlin (champs camelCase, `jeu` absent quand rien n'est ouvert).
        let k: EtatMenu = serde_json::from_str("{}").unwrap();
        assert_eq!(k.jeu, None);
        assert!(!k.retour_actif);
        let k: EtatMenu = serde_json::from_str(r#"{"retourActif":true}"#).unwrap();
        assert!(k.retour_actif);
        assert_eq!(k.arret_direct, None);
        let k: EtatMenu = serde_json::from_str(r#"{"arretDirect":"refuse"}"#).unwrap();
        assert_eq!(k.arret_direct.as_deref(), Some("refuse"));
    }

    #[test]
    fn fermeture_pour_l_interface() {
        // Réponses du plugin Kotlin (`MenuJeuPlugin.fermeture`).
        let f: Fermeture = serde_json::from_str(r#"{"direct":true}"#).unwrap();
        assert_eq!(f, Fermeture { direct: true, raison: None });
        let f: Fermeture = serde_json::from_str(r#"{"direct":false,"raison":"arrete"}"#).unwrap();
        assert_eq!(f.raison.as_deref(), Some("arrete"));
        assert_eq!(serde_json::to_value(Fermeture { direct: true, raison: None }).unwrap(), serde_json::json!({"direct": true}));
    }
}
