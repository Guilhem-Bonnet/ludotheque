//! Appareil Android (Ayn Thor) : configuration, ROM accessibles par l'arbre SAF accordé, lancement
//! d'un émulateur Android ou de Moonlight par intent explicite.
//!
//! Sur Android, l'app ne synchronise aucune sauvegarde : les émulateurs les gardent dans
//! `Android/data`, illisible pour une app. Le PC s'en charge par adb (`ludo adb-watch`), et publie
//! l'inventaire de l'appareil (jeux présents, jouables en flux) sur le serveur.
//!
//! Les fonctions de ce module sans pont Android (rapprochement des ROM, choix de l'émulateur, intents)
//! se compilent partout, pour être testées sur le PC.

use ludo_client::local::{match_key, title_key};
use ludo_proto::{CatalogGame, DeviceGame, FileRole};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Réglages propres à Android, lus dans le même `config.toml` que la configuration de `ludo`.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct AndroidConf {
    #[serde(default)]
    pub moonlight: MoonlightConf,
    #[serde(default)]
    pub maj: MajConf,
}

/// Mise à jour de l'app depuis l'app : `android.json` publié avec les releases (CI).
#[derive(Debug, Default, Clone, Deserialize)]
pub struct MajConf {
    /// Adresse de `android.json` (essai sur un serveur local) ; défaut : dernière release GitHub.
    pub android: Option<String>,
}

/// `android.json` de la dernière release de l'app (workflow android-release.yml).
pub const MAJ_URL: &str = "https://github.com/Guilhem-Bonnet/ludotheque/releases/latest/download/android.json";

impl MajConf {
    pub fn url(&self) -> String {
        self.android.clone().filter(|u| !u.is_empty()).unwrap_or_else(|| MAJ_URL.to_string())
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct MoonlightConf {
    /// UUID de l'hôte Sunshine connu de Moonlight (le PC).
    pub hote: Option<String>,
    /// Flux HDR demandé à Moonlight.
    #[serde(default)]
    pub hdr: bool,
    /// Appli Sunshine imposée par jeu (identifiant Ludothèque → nom), avant celle de l'inventaire.
    #[serde(default)]
    pub applis: BTreeMap<String, String>,
}

impl AndroidConf {
    pub fn parse(text: &str) -> Self {
        toml::from_str(text).unwrap_or_default()
    }
}

/// Configuration écrite au premier démarrage si le PC n'en a pas encore déposé une.
pub const DEFAULT_CONFIG: &str = r#"# Ludothèque — configuration de cet appareil Android.
# Déposée par le PC (scripts/apk.sh), ou à modifier ici : Android/data/<paquet>/files/config.toml
serveur = "https://ludotheque.srvdreamer.fr"
appareil = "thor"
# profil_defaut = "guilhem"

[moonlight]
# UUID de l'hôte Sunshine (PC) : à recopier depuis Moonlight ou depuis le PC.
# hote = "00000000-0000-0000-0000-000000000000"
hdr = false
"#;

/// Émulateur Android d'une console et la façon de lui passer la ROM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidEmu {
    pub name: &'static str,
    pub package: &'static str,
    pub activity: &'static str,
    /// Action de l'intent (VIEW par défaut).
    pub action: Option<&'static str>,
    /// Extra qui reçoit aussi l'URI de la ROM (melonDS).
    pub uri_extra: Option<&'static str>,
    /// Sous-dossier de l'arbre des ROM.
    pub dir: &'static str,
    /// Jeu rangé en dossier (Wii U : `<jeu>/code/*.rpx`) : l'émulateur reçoit le `.rpx` du dossier.
    pub folder: bool,
}

/// Émulateurs de la Thor, relevés dans le journal du lanceur Cocoon (`launch_debug.log`) ; Dolphin et
/// Cemu d'après leurs activités (`dumpsys package`, 21/11) : Dolphin ouvre le fichier de l'extra
/// `AutoStartFile` (sa MainActivity), Cemu une URI content `.rpx`/`.wud`/`.wua` (EmulationActivity).
pub const EMULATORS: &[(&str, AndroidEmu)] = &[
    (
        "switch",
        AndroidEmu { name: "Eden", package: "dev.eden.eden_emulator", activity: "org.yuzu.yuzu_emu.activities.EmulationActivity", action: None, uri_extra: None, dir: "switch", folder: false },
    ),
    (
        "3ds",
        AndroidEmu { name: "Azahar", package: "org.azahar_emu.azahar", activity: "org.citra.citra_emu.activities.EmulationActivity", action: None, uri_extra: None, dir: "n3ds", folder: false },
    ),
    (
        "ds",
        AndroidEmu {
            name: "melonDS",
            package: "me.magnum.melondualds",
            activity: "me.magnum.melonds.ui.emulator.EmulatorActivity",
            action: Some("me.magnum.melondualds.LAUNCH_ROM"),
            uri_extra: Some("uri"),
            dir: "nds",
            folder: false,
        },
    ),
    (
        "gc",
        AndroidEmu {
            name: "Dolphin",
            package: "org.dolphinemu.dolphinemu",
            activity: "org.dolphinemu.dolphinemu.ui.main.MainActivity",
            action: None,
            uri_extra: Some("AutoStartFile"),
            dir: "gc",
            folder: false,
        },
    ),
    (
        "wiiu",
        AndroidEmu { name: "Cemu", package: "info.cemu.cemu", activity: "info.cemu.cemu.emulation.EmulationActivity", action: None, uri_extra: None, dir: "wiiu", folder: true },
    ),
];

pub fn emulator_for(console: &str) -> Option<&'static AndroidEmu> {
    EMULATORS.iter().find(|(c, _)| *c == console).map(|(_, e)| e)
}

/// Sous-dossiers de l'arbre des ROM lus par l'app.
pub fn rom_dirs() -> Vec<String> {
    EMULATORS.iter().map(|(_, e)| e.dir.to_string()).collect()
}

/// Fichier de l'arbre des ROM (réponse de `listRoms` du plugin Kotlin).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RomFile {
    pub dir: String,
    pub name: String,
    #[serde(default)]
    pub is_dir: bool,
    pub uri: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RomList {
    pub granted: bool,
    #[serde(default)]
    pub files: Vec<RomFile>,
}

/// ROM de ce jeu dans l'arbre : même nom (formats équivalents), sinon même identité Switch. Pour une
/// console à jeux en dossier (Wii U), le dossier du jeu.
pub fn find_rom<'a>(game: &CatalogGame, files: &'a [RomFile]) -> Option<&'a RomFile> {
    let emu = emulator_for(&game.console)?;
    let bases: Vec<&str> = game.files.iter().filter(|f| f.role == FileRole::Base).map(|f| f.path.as_str()).collect();
    let in_dir = || files.iter().filter(|r| r.dir == emu.dir && r.is_dir == emu.folder);
    in_dir()
        .find(|r| bases.iter().any(|b| match_key(b) == match_key(&r.name)))
        .or_else(|| in_dir().find(|r| title_key(&r.name).is_some_and(|t| bases.iter().any(|b| title_key(b).as_ref() == Some(&t)))))
}

/// Intent explicite vers une activité d'une autre app (`startActivity` du plugin Kotlin).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartActivity {
    pub package_name: String,
    pub activity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    pub extras: BTreeMap<String, String>,
    pub bool_extras: BTreeMap<String, bool>,
}

/// Ouvre la ROM dans l'émulateur (URI content de l'arbre, lecture accordée à l'émulateur).
pub fn emulator_intent(emu: &AndroidEmu, rom_uri: &str) -> StartActivity {
    let mut extras = BTreeMap::new();
    if let Some(k) = emu.uri_extra {
        extras.insert(k.to_string(), rom_uri.to_string());
    }
    StartActivity {
        package_name: emu.package.into(),
        activity: emu.activity.into(),
        action: emu.action.map(str::to_string),
        data: Some(rom_uri.to_string()),
        extras,
        bool_extras: BTreeMap::new(),
    }
}

/// Moonlight 12 : `ShortcutTrampoline` (exportée, sans filtre d'intent) avec l'hôte et l'appli
/// Sunshine par son NOM (l'identifiant change avec le nom ou la jaquette).
pub fn moonlight_intent(host_uuid: &str, app_name: &str, hdr: bool) -> StartActivity {
    StartActivity {
        package_name: "com.limelight".into(),
        activity: "com.limelight.ShortcutTrampoline".into(),
        action: Some("android.intent.action.VIEW".into()),
        data: None,
        extras: BTreeMap::from([("UUID".to_string(), host_uuid.to_string()), ("AppName".to_string(), app_name.to_string())]),
        bool_extras: BTreeMap::from([("HDR".to_string(), hdr)]),
    }
}

/// Appli Sunshine d'un jeu : réglage de l'appareil, sinon inventaire publié par le PC.
pub fn stream_app(conf: &AndroidConf, inv: Option<&DeviceGame>, game: &str) -> Option<String> {
    conf.moonlight.applis.get(game).cloned().or_else(|| inv.filter(|g| g.stream).and_then(|g| g.stream_app.clone()))
}

/// Comment lancer un jeu sur l'appareil.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// ROM présente : émulateur Android.
    Local { emu: &'static AndroidEmu, uri: String },
    /// Flux depuis le PC (Moonlight → Sunshine).
    Stream { host: String, app: String, hdr: bool },
}

/// Choix du lancement : `mode` « local » ou « flux » imposé, sinon la ROM locale d'abord.
pub fn plan(game: &CatalogGame, roms: &[RomFile], conf: &AndroidConf, inv: Option<&DeviceGame>, mode: Option<&str>) -> Result<Plan, String> {
    let local = || -> Result<Plan, String> {
        let emu = emulator_for(&game.console).ok_or("Aucun émulateur Android pris en charge pour cette console")?;
        // ROM désignée par l'inventaire du PC (trouvée par nom ou par identifiant de titre), sinon par nom.
        let rom = inv
            .and_then(|i| i.rom.as_deref())
            .and_then(|r| roms.iter().find(|f| f.is_dir == emu.folder && format!("{}/{}", f.dir, f.name) == r))
            .or_else(|| find_rom(game, roms))
            .ok_or_else(|| format!("Jeu absent du dossier des ROM ({})", emu.dir))?;
        Ok(Plan::Local { emu, uri: rom.uri.clone() })
    };
    let stream = || -> Result<Plan, String> {
        let app = stream_app(conf, inv, &game.id).ok_or("Jeu non jouable en flux (aucune appli Sunshine)")?;
        let host = conf.moonlight.hote.clone().filter(|h| !h.is_empty()).ok_or("Hôte Moonlight non configuré ([moonlight] hote)")?;
        Ok(Plan::Stream { host, app, hdr: conf.moonlight.hdr })
    };
    match mode {
        Some("local") => local(),
        Some("flux") => stream(),
        _ => local().or_else(|local_err| stream().map_err(|_| local_err)),
    }
}

/// Version Android publiée (`apkVerifier` du plugin Kotlin).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApkVersion {
    /// Version installée (`0.6.6`).
    pub installee: String,
    pub code: i64,
    /// Version publiée.
    pub version: String,
    pub version_code: i64,
    pub notes: String,
    pub disponible: bool,
}

/// Écran du haut de la Thor : pousse l'état du jeu choisi ; rend `true` si les deux écrans sont actifs.
#[tauri::command(rename = "ecranHaut")]
pub async fn ecran_haut(app: tauri::AppHandle, etat: serde_json::Value) -> Result<bool, String> {
    #[cfg(target_os = "android")]
    return bridge::push_haut(&app, etat);
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, etat);
        Ok(false)
    }
}

/// Mise à jour Android : version publiée comparée à la version installée.
#[tauri::command(rename = "apkVerifier")]
pub async fn apk_verifier(app: tauri::AppHandle) -> Result<ApkVersion, String> {
    #[cfg(target_os = "android")]
    return tauri::async_runtime::spawn_blocking(move || bridge::apk_verifier(&app)).await.map_err(|e| e.to_string())?;
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Err("Mise à jour Android sur Android seulement".into())
    }
}

/// Mise à jour Android : téléchargement vérifié puis installeur d'Android (événement `ludo-apk`).
#[tauri::command(rename = "apkInstaller")]
pub async fn apk_installer(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "android")]
    return tauri::async_runtime::spawn_blocking(move || bridge::apk_installer(&app)).await.map_err(|e| e.to_string())?;
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Err("Mise à jour Android sur Android seulement".into())
    }
}

#[cfg(target_os = "android")]
pub use bridge::*;

#[cfg(target_os = "android")]
mod bridge {
    use super::*;
    use std::path::PathBuf;
    use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
    use tauri::{Manager, Wry};

    /// Plugin Kotlin `LudoPlugin` (gen/android/app/src/main/java/fr/srvdreamer/ludotheque).
    pub struct Bridge(PluginHandle<Wry>);

    pub fn plugin() -> TauriPlugin<Wry> {
        Builder::new("ludo")
            .setup(|app, api| {
                let handle = api.register_android_plugin("fr.srvdreamer.ludotheque", "LudoPlugin")?;
                app.manage(Bridge(handle));
                Ok(())
            })
            .build()
    }

    fn bridge(app: &tauri::AppHandle) -> Result<tauri::State<'_, Bridge>, String> {
        app.try_state::<Bridge>().ok_or_else(|| "Pont Android indisponible".to_string())
    }

    #[derive(Serialize)]
    struct ListArgs {
        dirs: Vec<String>,
    }

    /// ROM des dossiers lus par l'app ; `granted = false` tant que l'arbre n'est pas accordé.
    pub fn list_roms(app: &tauri::AppHandle) -> Result<RomList, String> {
        bridge(app)?.0.run_mobile_plugin("listRoms", ListArgs { dirs: rom_dirs() }).map_err(|e| e.to_string())
    }

    #[derive(Deserialize)]
    struct Picked {
        #[allow(dead_code)]
        uri: String,
    }

    /// Demande l'accès au dossier des ROM (sélecteur Android, une seule fois).
    pub fn pick_roms_tree(app: &tauri::AppHandle) -> Result<(), String> {
        bridge(app)?.0.run_mobile_plugin::<Picked>("pickRomsTree", ()).map(|_| ()).map_err(|e| e.to_string())
    }

    /// ROM lisibles ; l'accès est demandé d'abord s'il manque.
    pub fn list_roms_or_ask(app: &tauri::AppHandle) -> Result<RomList, String> {
        let list = list_roms(app)?;
        if list.granted {
            return Ok(list);
        }
        pick_roms_tree(app)?;
        list_roms(app)
    }

    #[derive(Serialize)]
    struct UriArgs<'a> {
        uri: &'a str,
    }

    #[derive(Deserialize)]
    struct Found {
        uri: Option<String>,
    }

    /// Intent de l'émulateur ; jeu en dossier (Wii U) : son `code/*.rpx`.
    pub fn local_intent(app: &tauri::AppHandle, emu: &AndroidEmu, uri: &str) -> Result<StartActivity, String> {
        if !emu.folder {
            return Ok(emulator_intent(emu, uri));
        }
        let found: Found = bridge(app)?.0.run_mobile_plugin("findRpx", UriArgs { uri }).map_err(|e| e.to_string())?;
        let rpx = found.uri.ok_or("Fichier .rpx introuvable dans le dossier du jeu (code/)")?;
        Ok(emulator_intent(emu, &rpx))
    }

    #[derive(Deserialize)]
    struct Actif {
        actif: bool,
    }

    pub fn push_haut(app: &tauri::AppHandle, etat: serde_json::Value) -> Result<bool, String> {
        let r: Actif = bridge(app)?.0.run_mobile_plugin("ecranHaut", etat).map_err(|e| e.to_string())?;
        Ok(r.actif)
    }

    #[derive(Serialize)]
    struct MajArgs {
        url: String,
    }

    pub fn apk_verifier(app: &tauri::AppHandle) -> Result<ApkVersion, String> {
        bridge(app)?.0.run_mobile_plugin("apkVerifier", MajArgs { url: conf().maj.url() }).map_err(plugin_error)
    }

    pub fn apk_installer(app: &tauri::AppHandle) -> Result<(), String> {
        bridge(app)?.0.run_mobile_plugin::<serde_json::Value>("apkInstaller", MajArgs { url: conf().maj.url() }).map(|_| ()).map_err(plugin_error)
    }

    /// Message du plugin Kotlin (déjà en français), sans l'enveloppe technique de l'erreur.
    fn plugin_error(e: tauri::plugin::mobile::PluginInvokeError) -> String {
        match e {
            tauri::plugin::mobile::PluginInvokeError::InvokeRejected(r) => r.message.unwrap_or_else(|| "Mise à jour impossible".into()),
            other => other.to_string(),
        }
    }

    pub fn start(app: &tauri::AppHandle, intent: &StartActivity) -> Result<(), String> {
        bridge(app)?.0.run_mobile_plugin::<serde_json::Value>("startActivity", intent).map(|_| ()).map_err(|e| e.to_string())
    }

    /// Nom du paquet (avec le suffixe `.debug` d'une version de développement).
    fn package() -> String {
        std::fs::read("/proc/self/cmdline")
            .ok()
            .and_then(|b| b.split(|c| *c == 0).next().map(|s| String::from_utf8_lossy(s).into_owned()))
            .map(|s| s.split(':').next().unwrap_or(&s).to_string())
            .unwrap_or_else(|| "fr.srvdreamer.ludotheque".into())
    }

    /// Dossier que le PC peut écrire par adb : `/sdcard/Android/data/<paquet>/files`.
    pub fn external_dir() -> PathBuf {
        PathBuf::from(format!("/storage/emulated/0/Android/data/{}/files", package()))
    }

    /// Prépare l'environnement de `ludo-client` : `HOME` = données privées de l'app (journal),
    /// `LUDO_CONFIG` = `config.toml` du dossier externe (écrit par le PC, sinon modèle par défaut).
    pub fn prepare_env(app: &tauri::AppHandle) {
        if let Ok(dir) = app.path().app_data_dir() {
            let _ = std::fs::create_dir_all(&dir);
            std::env::set_var("HOME", &dir);
        }
        let config = external_dir().join("config.toml");
        if !config.exists() {
            let _ = std::fs::create_dir_all(external_dir());
            let _ = std::fs::write(&config, DEFAULT_CONFIG);
        }
        std::env::set_var("LUDO_CONFIG", &config);
    }

    /// Réglages Android du fichier de configuration (absents ou illisibles : valeurs par défaut).
    pub fn conf() -> AndroidConf {
        let path = ludo_client::config::Config::default_path();
        std::fs::read_to_string(path).map(|t| AndroidConf::parse(&t)).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str, console: &str, path: &str) -> CatalogGame {
        serde_json::from_value(serde_json::json!({
            "id": id, "title": "Jeu", "short": "Jeu", "console": console,
            "art": { "motif": "lune", "from": "#000000", "to": "#ffffff" }, "catalogued": false,
            "files": [{ "id": 1, "role": "base", "path": path, "format": "nsp", "size": 1, "mtime": 0 }]
        }))
        .unwrap()
    }

    fn rom(dir: &str, name: &str) -> RomFile {
        RomFile { dir: dir.into(), name: name.into(), is_dir: false, uri: format!("content://x/{dir}/{name}") }
    }

    #[test]
    fn rom_trouvee_par_nom_ou_identite_switch() {
        let roms = [
            rom("switch", "Super Mario Party [010036B0034E4000][v0].nsp"),
            rom("switch", "Pokemon Brilliant Diamond [0100000011D90000][v0](nsw2u.xyz).xci"),
            rom("n3ds", "New Super Mario Bros. 2 (Europe).3ds"),
        ];
        let smp = game("switch-010036b0034e4000", "switch", "switch/Super Mario Party [010036B0034E4000][v0].nsz");
        assert_eq!(find_rom(&smp, &roms).unwrap().name, roms[0].name);
        let bd = game("switch-0100000011d90000", "switch", "switch/Pokemon Diamant Etincelant [0100000011D90000].nsp");
        assert_eq!(find_rom(&bd, &roms).unwrap().name, roms[1].name);
        let nsmb = game("3ds-nsmb2", "3ds", "3ds/New Super Mario Bros. 2 (Europe).3ds");
        assert_eq!(find_rom(&nsmb, &roms).unwrap().dir, "n3ds");
        // Même nom dans un autre dossier de console : pas de confusion.
        let wrong = game("ds-x", "ds", "ds/New Super Mario Bros. 2 (Europe).3ds");
        assert!(find_rom(&wrong, &roms).is_none());
    }

    #[test]
    fn intents_emulateurs_et_moonlight() {
        let eden = emulator_for("switch").unwrap();
        let i = emulator_intent(eden, "content://rom");
        assert_eq!((i.package_name.as_str(), i.action.as_deref(), i.data.as_deref()), ("dev.eden.eden_emulator", None, Some("content://rom")));
        let melon = emulator_intent(emulator_for("ds").unwrap(), "content://rom");
        assert_eq!(melon.extras.get("uri").map(String::as_str), Some("content://rom"));
        assert_eq!(melon.action.as_deref(), Some("me.magnum.melondualds.LAUNCH_ROM"));
        let m = serde_json::to_value(moonlight_intent("UUID-1", "Mario Kart 8 Deluxe · Ludothèque", false)).unwrap();
        assert_eq!(m["activity"], "com.limelight.ShortcutTrampoline");
        assert_eq!(m["extras"]["AppName"], "Mario Kart 8 Deluxe · Ludothèque");
        assert_eq!(m["boolExtras"]["HDR"], false);
    }

    #[test]
    fn dolphin_et_cemu() {
        let tp = game("gc-zelda-twilight-princess", "gc", "gc/Zelda Twilight Princess (EUR) [GZ2P01].rvz");
        let ww = game("wiiu-the-legend-of-zelda-the-wind-waker-hd", "wiiu", "wiiu/The Legend of Zelda The Wind Waker HD");
        let roms = [
            rom("gc", "Zelda Twilight Princess (EUR) [GZ2P01].rvz"),
            RomFile { dir: "wiiu".into(), name: "The Legend of Zelda The Wind Waker HD".into(), is_dir: true, uri: "content://x/wiiu/ww".into() },
            rom("wiiu", "The Legend of Zelda The Wind Waker HD"),
        ];
        assert_eq!(find_rom(&tp, &roms).unwrap().dir, "gc");
        // Wii U : le DOSSIER du jeu, pas un fichier homonyme.
        let found = find_rom(&ww, &roms).unwrap();
        assert!(found.is_dir, "{found:?}");
        let i = emulator_intent(emulator_for("gc").unwrap(), "content://rom");
        assert_eq!(i.extras.get("AutoStartFile").map(String::as_str), Some("content://rom"));
        assert_eq!(i.data.as_deref(), Some("content://rom"));
        assert_eq!(emulator_for("wiiu").unwrap().package, "info.cemu.cemu");
        assert!(rom_dirs().contains(&"gc".to_string()) && rom_dirs().contains(&"wiiu".to_string()));
    }

    #[test]
    fn adresse_de_mise_a_jour() {
        assert_eq!(AndroidConf::parse("").maj.url(), MAJ_URL);
        assert_eq!(AndroidConf::parse("[maj]\nandroid = \"http://127.0.0.1:8765/android.json\"\n").maj.url(), "http://127.0.0.1:8765/android.json");
    }

    #[test]
    fn plan_local_puis_flux() {
        let conf = AndroidConf::parse("serveur = \"x\"\nappareil = \"thor\"\n[moonlight]\nhote = \"H\"\n");
        let mk8 = game("switch-mk8", "switch", "switch/Mario Kart 8 Deluxe [0100152000022000][v0].nsp");
        let inv = DeviceGame { game: "switch-mk8".into(), installed: false, stream: true, save: true, stream_app: Some("MK8 · Ludothèque".into()), rom: None };
        assert_eq!(plan(&mk8, &[], &conf, Some(&inv), None), Ok(Plan::Stream { host: "H".into(), app: "MK8 · Ludothèque".into(), hdr: false }));
        let roms = [rom("switch", "Mario Kart 8 Deluxe [0100152000022000][US][v0].nsp")];
        assert!(matches!(plan(&mk8, &roms, &conf, Some(&inv), None), Ok(Plan::Local { .. })));
        assert!(matches!(plan(&mk8, &roms, &conf, Some(&inv), Some("flux")), Ok(Plan::Stream { .. })));
        // Ni ROM ni flux : l'erreur du lancement local est rendue.
        let err = plan(&mk8, &[], &conf, None, None).unwrap_err();
        assert!(err.starts_with("Jeu absent"), "{err}");
        // Appli imposée par la configuration.
        let conf = AndroidConf::parse("[moonlight]\nhote = \"H\"\napplis = { switch-mk8 = \"Autre\" }\n");
        assert_eq!(stream_app(&conf, Some(&inv), "switch-mk8").as_deref(), Some("Autre"));
    }

    /// LM2 : nom du catalogue et nom sur la Thor différents (même identifiant de titre, rapprochés par
    /// le PC) ; l'app lance la ROM que l'inventaire désigne.
    #[test]
    fn rom_designee_par_l_inventaire() {
        let conf = AndroidConf::parse("serveur = \"x\"\nappareil = \"thor\"\n");
        let lm2 = game("3ds-3ds0239-luigis-mansion-2", "3ds", "3ds/3DS0239 - Luigis Mansion 2(Europe)(En,Fr,Ge,It,Es,Nl,Pt,Ru).cxi");
        let roms = [rom("n3ds", "Luigi's Mansion - Dark Moon (Europe) (En,Fr,De,Es,It,Nl,Pt,Ru).cxi")];
        assert!(plan(&lm2, &roms, &conf, None, None).is_err(), "le nom seul ne la trouve pas");
        let inv = DeviceGame {
            game: lm2.id.clone(),
            installed: true,
            stream: false,
            save: false,
            stream_app: None,
            rom: Some("n3ds/Luigi's Mansion - Dark Moon (Europe) (En,Fr,De,Es,It,Nl,Pt,Ru).cxi".into()),
        };
        assert!(matches!(plan(&lm2, &roms, &conf, Some(&inv), None), Ok(Plan::Local { uri, .. }) if uri == roms[0].uri));
    }
}
