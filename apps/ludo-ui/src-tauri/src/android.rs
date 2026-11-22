//! Appareil Android (Ayn Thor) : configuration, ROM accessibles par l'arbre SAF accordé, lancement
//! d'un émulateur Android ou de Moonlight par intent explicite.
//!
//! Sur Android, l'app ne synchronise aucune sauvegarde : les émulateurs les gardent dans
//! `Android/data`, illisible pour une app. Le PC s'en charge par adb (`ludo adb-watch`), et publie
//! l'inventaire de l'appareil (jeux présents, jouables en flux) sur le serveur.
//!
//! Les fonctions de ce module sans pont Android (rapprochement des ROM, choix de l'émulateur, intents)
//! se compilent partout, pour être testées sur le PC.

use ludo_client::local::{loose_key, match_key, title_key};
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
    /// Jeu rangé en dossier (Wii U : `<jeu>/code/*.rpx`) : l'émulateur reçoit le `.rpx` du dossier,
    /// au format de SA bibliothèque (voir `folder_game_uri`) et avec l'arbre des ROM accordé, sinon il
    /// ne voit pas les dossiers `meta/` et `content/` voisins.
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

/// ROM de ce jeu dans l'arbre, dans le sous-dossier de sa console seulement (pour une console à jeux
/// en dossier, Wii U, le dossier du jeu), dans cet ordre :
/// 1. même nom (formats équivalents `nsz`→`nsp`, `xcz`→`xci`) ;
/// 2. même clé souple (`local::loose_key` : numérotation « 3DS0080 - » retirée, espaces et
///    ponctuation ramenés) ET même extension : la règle de l'inventaire publié par le PC
///    (`ludo-client` `rom_for`), sans quoi un jeu marqué « installé » ne se lance pas (Resident Evil
///    Revelations, 22/11 : serveur « 3DS0080 - Resident Evil Revelations (Europe)(En,…).cxi »,
///    Thor « Resident Evil Revelations (Europe) (En,…).cxi ») ;
/// 3. même identité Switch `[<titre>]`, même version d'abord, sinon toute version (Thor « Pokemon
///    Lets Go Pikachu [010003F003A34000].xci » sans `[v…]`, serveur `[v1.0.2]`) : c'est le même jeu
///    pour l'émulateur, la version ne compte que pour la mise à jour.
pub fn find_rom<'a>(game: &CatalogGame, files: &'a [RomFile]) -> Option<&'a RomFile> {
    let emu = emulator_for(&game.console)?;
    let bases: Vec<&str> = game.files.iter().filter(|f| f.role == FileRole::Base).map(|f| f.path.as_str()).collect();
    let in_dir = || files.iter().filter(|r| r.dir == emu.dir && r.is_dir == emu.folder);
    let tid = |name: &str| title_key(name).map(|(t, _)| t);
    // Identité Switch : seulement sur un fichier de jeu (un `.sav`, une image ou un `.bak` voisin portant
    // le même `[titre]` n'est jamais pris pour la ROM).
    let switch_rom = |r: &&RomFile| matches!(match_key(&r.name).1.as_str(), "nsp" | "xci");
    in_dir()
        .find(|r| bases.iter().any(|b| match_key(b) == match_key(&r.name)))
        .or_else(|| {
            in_dir().find(|r| {
                let key = loose_key(&r.name);
                !key.is_empty() && bases.iter().any(|b| loose_key(b) == key && match_key(b).1 == match_key(&r.name).1)
            })
        })
        .or_else(|| in_dir().filter(switch_rom).find(|r| title_key(&r.name).is_some_and(|t| bases.iter().any(|b| title_key(b).as_ref() == Some(&t)))))
        .or_else(|| in_dir().filter(switch_rom).find(|r| tid(&r.name).is_some_and(|t| bases.iter().any(|b| tid(b).as_ref() == Some(&t)))))
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
    /// Arbre des ROM accordé à l'app lancée (lecture, préfixe) : fichiers voisins de la ROM lisibles.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_tree: Option<String>,
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
        grant_tree: None,
    }
}

/// Jeu en dossier (Wii U) : URI du `.rpx` telle que Cemu l'écrit dans sa bibliothèque
/// (`title_list_cache.xml`, relevé du 22/11) — arbre = dossier de la console, identifiant de document
/// avec des `/` littéraux — et arbre des ROM à lui accorder. Avec l'URI d'arbre ordinaire
/// (`…%2Fcode%2Fcking.rpx`), Cemu 0.5.1 écrit « Launching executable in standalone mode due to incorrect
/// layout or missing meta files », prend un identifiant fictif et le jeu plante ; au format de sa
/// bibliothèque il charge le titre (TitleId 00050000-10143500, Wind Waker HD, essai adb du 22/11).
pub fn folder_game_intent(emu: &AndroidEmu, rpx_uri: &str) -> Option<StartActivity> {
    let rest = rpx_uri.strip_prefix("content://")?;
    let (authority, rest) = rest.split_once("/tree/")?;
    let (tree_enc, doc_enc) = rest.split_once("/document/")?;
    let (tree, doc) = (percent_decode(tree_enc)?, percent_decode(doc_enc)?);
    let console_tree = format!("{}/{}", tree.trim_end_matches('/'), emu.dir);
    if !doc.starts_with(&format!("{console_tree}/")) {
        return None;
    }
    let uri = format!("content://{authority}/tree/{}/document/{}", uri_encode(&console_tree, false), uri_encode(&doc, true));
    let mut intent = emulator_intent(emu, &uri);
    // Dossier de la console seulement (préfixe de l'URI des données), pas tout l'arbre des ROM.
    intent.grant_tree = Some(format!("content://{authority}/tree/{}", uri_encode(&console_tree, false)));
    Some(intent)
}

/// Comme `Uri.encode` d'Android (caractères non réservés gardés), `/` gardé si demandé.
fn uri_encode(s: &str, keep_slash: bool) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"_-!.~'()*".contains(&b) || (keep_slash && b == b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3).filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
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
        grant_tree: None,
    }
}

/// Identifiant Moonlight d'une appli Sunshine, lu dans le raccourci que Cocoon écrit pour elle
/// (`Roms/moonlight/<appli>.txt` : lignes `[moonlight_uuid] <hôte>` et `[moonlight_id] <nombre>`). Rendu
/// seulement pour le même hôte. Avec l'extra `AppId`, `ShortcutTrampoline` lance l'appli sans chercher
/// son nom dans le cache de Moonlight (code de Moonlight lu le 22/11) : c'est ce cache qui refusait une
/// appli récente (« application non valide », 21/11). Cocoon lance ses flux ainsi (`launch_debug.log` :
/// « Intent extras: UUID, AppId »).
pub fn cocoon_app_id(text: &str, host: &str) -> Option<String> {
    let field = |k: &str| text.lines().find_map(|l| l.trim().strip_prefix(k).map(|v| v.trim().to_string()));
    let uuid = field("[moonlight_uuid]")?;
    let id = field("[moonlight_id]")?;
    (uuid.eq_ignore_ascii_case(host) && !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}

/// Nom du raccourci Cocoon d'une appli Sunshine.
pub fn cocoon_file(app_name: &str) -> String {
    format!("{app_name}.txt")
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
        let rom = find_rom(game, roms).ok_or_else(|| format!("Jeu absent du dossier des ROM ({})", emu.dir))?;
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
        folder_game_intent(emu, &rpx).ok_or_else(|| format!("Adresse du jeu inattendue : {rpx}"))
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

    #[derive(Deserialize)]
    struct Texte {
        text: Option<String>,
    }

    /// Flux : intent Moonlight, avec l'identifiant `AppId` du raccourci Cocoon s'il existe pour cette appli
    /// et cet hôte (sinon le nom seul, cherché par Moonlight dans son cache).
    pub fn stream_intent(app: &tauri::AppHandle, host: &str, name: &str, hdr: bool) -> StartActivity {
        let mut intent = moonlight_intent(host, name, hdr);
        let file = cocoon_file(name);
        let id = list_roms_in(app, &["moonlight"])
            .ok()
            .and_then(|l| l.files.into_iter().find(|f| !f.is_dir && f.name == file))
            .and_then(|f| bridge(app).ok()?.0.run_mobile_plugin::<Texte>("readText", UriArgs { uri: &f.uri }).ok()?.text)
            .and_then(|t| cocoon_app_id(&t, host));
        if let Some(id) = id {
            intent.extras.insert("AppId".into(), id);
        }
        intent
    }

    fn list_roms_in(app: &tauri::AppHandle, dirs: &[&str]) -> Result<RomList, String> {
        bridge(app)?.0.run_mobile_plugin("listRoms", ListArgs { dirs: dirs.iter().map(|d| d.to_string()).collect() }).map_err(|e| e.to_string())
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
    fn rom_renumerotee_ou_sans_version_trouvee() {
        // Retour de Guilhem (22/11) : « certains jeux ne se lancent pas, comme Resident Evil Revelations ».
        let re = game("3ds-3ds0080-resident-evil-revelations", "3ds", "3ds/3DS0080 - Resident Evil Revelations (Europe)(En,Fr,Ge,It,Es,jp).cxi");
        let lgp = game("switch-010003f003a34000", "switch", "switch/Pokemon Lets Go Pikachu [010003F003A34000][v1.0.2].xci");
        let roms = [
            rom("n3ds", "Resident Evil Revelations (Europe) (En,Fr,Ge,It,Es,jp).cxi"),
            rom("n3ds", "Resident Evil Revelations (Europe) (En,Fr,Ge,It,Es,jp).sav"),
            rom("switch", "Pokemon Lets Go Eevee [0100187003A36000][v0].xci"),
            rom("switch", "Pokemon Lets Go Pikachu [010003F003A34000].xci"),
        ];
        assert_eq!(find_rom(&re, &roms).unwrap().name, roms[0].name);
        assert_eq!(find_rom(&lgp, &roms).unwrap().name, roms[3].name);
        // Même titre, autre extension (fichier voisin) : jamais pris pour la ROM.
        assert!(find_rom(&re, &roms[1..]).is_none());
        // Identité Switch portée par un fichier qui n'est pas un jeu : refusé.
        let voisins = [rom("switch", "Pokemon Lets Go Pikachu [010003F003A34000].sav"), rom("switch", "capture [010003F003A34000].jpg")];
        assert!(find_rom(&lgp, &voisins).is_none());
        // Même version préférée quand plusieurs copies existent.
        let maj = game("switch-010003f003a34000", "switch", "switch/Pokemon Lets Go Pikachu [010003F003A34000][v65536].xci");
        let two = [rom("switch", "X [010003F003A34000][v0].xci"), rom("switch", "Y [010003F003A34000][v65536].xci")];
        assert_eq!(find_rom(&maj, &two).unwrap().name, two[1].name);
    }

    /// Audit du catalogue de la Thor sur données réelles (relevé du 22/11 : `ls Documents/Roms` par
    /// adb, `/api/v1/devices`, `/api/v1/games`) : tout jeu que l'inventaire du PC dit installé a un
    /// lancement local (émulateur de sa console) ; tout jeu en flux seulement part en flux.
    #[test]
    fn audit_catalogue_thor_reel() {
        #[derive(Deserialize)]
        struct Fx {
            roms: Vec<FxRom>,
            games: Vec<FxGame>,
        }
        #[derive(Deserialize)]
        struct FxRom {
            dir: String,
            name: String,
            is_dir: bool,
        }
        #[derive(Deserialize)]
        struct FxGame {
            id: String,
            console: String,
            installed: bool,
            stream: bool,
            stream_app: Option<String>,
            bases: Vec<String>,
        }
        let fx: Fx = serde_json::from_str(include_str!("fixtures/thor-2026-11-22.json")).unwrap();
        let roms: Vec<RomFile> =
            fx.roms.iter().map(|r| RomFile { dir: r.dir.clone(), name: r.name.clone(), is_dir: r.is_dir, uri: format!("content://t/{}/{}", r.dir, r.name) }).collect();
        let conf = AndroidConf::parse("[moonlight]\nhote = \"H\"\n");
        let mut fails = Vec::new();
        for g in &fx.games {
            let files: Vec<_> = g
                .bases
                .iter()
                .enumerate()
                .map(|(i, p)| serde_json::json!({ "id": i, "role": "base", "path": p, "format": "x", "size": 1, "mtime": 0 }))
                .collect();
            let cg: CatalogGame = serde_json::from_value(serde_json::json!({
                "id": g.id, "title": g.id, "short": g.id, "console": g.console,
                "art": { "motif": "lune", "from": "#000000", "to": "#ffffff" }, "catalogued": false, "files": files
            }))
            .unwrap();
            let inv = DeviceGame { game: g.id.clone(), installed: g.installed, stream: g.stream, save: false, stream_app: g.stream_app.clone() };
            match (g.installed, plan(&cg, &roms, &conf, Some(&inv), None)) {
                (true, Ok(Plan::Local { emu, uri })) if uri.starts_with(&format!("content://t/{}/", emu.dir)) && emulator_for(&g.console) == Some(emu) => {}
                (false, Ok(Plan::Stream { app, .. })) if Some(&app) == g.stream_app.as_ref() => {}
                (_, other) => fails.push(format!("{} : {other:?}", g.id)),
            }
        }
        assert!(fails.is_empty(), "{} jeux sans lancement correct :\n{}", fails.len(), fails.join("\n"));
        assert!(fx.games.iter().any(|g| g.installed) && fx.games.iter().any(|g| !g.installed), "relevé incomplet");
    }

    #[test]
    fn cemu_recoit_le_rpx_au_format_de_sa_bibliotheque() {
        let cemu = emulator_for("wiiu").unwrap();
        // URI rendue par findRpx (buildDocumentUriUsingTree sur l'arbre accordé à la Ludothèque).
        let rpx = "content://com.android.externalstorage.documents/tree/primary%3ADocuments%2FRoms/document/primary%3ADocuments%2FRoms%2Fwiiu%2FThe%20Legend%20of%20Zelda%20The%20Wind%20Waker%20HD%2Fcode%2Fcking.rpx";
        let i = folder_game_intent(cemu, rpx).unwrap();
        // Exactement l'URI essayée par adb le 22/11 (Cemu charge 00050000-10143500).
        assert_eq!(
            i.data.as_deref(),
            Some("content://com.android.externalstorage.documents/tree/primary%3ADocuments%2FRoms%2Fwiiu/document/primary%3ADocuments/Roms/wiiu/The%20Legend%20of%20Zelda%20The%20Wind%20Waker%20HD/code/cking.rpx")
        );
        assert_eq!(i.grant_tree.as_deref(), Some("content://com.android.externalstorage.documents/tree/primary%3ADocuments%2FRoms%2Fwiiu"));
        assert!(i.data.as_deref().unwrap().starts_with(i.grant_tree.as_deref().unwrap()), "l'URI des données est couverte par l'arbre accordé");
        // `%` mal formé : refusé.
        assert_eq!(percent_decode("a%+1"), None);
        assert_eq!(percent_decode("%C3%A9t%C3%A9"), Some("été".into()));
        assert_eq!(uri_encode("Pokémon : l'été/x", true), "Pok%C3%A9mon%20%3A%20l'%C3%A9t%C3%A9/x");
        assert_eq!((i.package_name.as_str(), i.activity.as_str()), ("info.cemu.cemu", "info.cemu.cemu.emulation.EmulationActivity"));
        let v = serde_json::to_value(&i).unwrap();
        assert!(v.get("grantTree").is_some());
        // Les autres émulateurs ne reçoivent pas l'arbre.
        assert!(serde_json::to_value(emulator_intent(emulator_for("3ds").unwrap(), "content://x")).unwrap().get("grantTree").is_none());
        // Fichier hors du dossier de la console : refusé plutôt qu'une adresse fausse.
        assert!(folder_game_intent(cemu, "content://a/tree/primary%3AR/document/primary%3AR%2Fgc%2Fx.rpx").is_none());
    }

    #[test]
    fn identifiant_moonlight_du_raccourci_cocoon() {
        // Fichier réel de la Thor (Documents/Roms/moonlight/Mario Kart 8 Deluxe · Ludothèque.txt, 22/11).
        let txt = "# Stream Sunshine : Mario Kart 8 Deluxe · Ludothèque\n[moonlight_uuid] D062BBAF-CA0B-2224-D212-D3CE6FEF9502\n[moonlight_id] 1083797917\n";
        assert_eq!(cocoon_app_id(txt, "D062BBAF-CA0B-2224-D212-D3CE6FEF9502").as_deref(), Some("1083797917"));
        assert_eq!(cocoon_app_id(txt, "d062bbaf-ca0b-2224-d212-d3ce6fef9502").as_deref(), Some("1083797917"));
        // Autre hôte, identifiant absent ou non numérique : pas d'AppId (nom seul).
        assert_eq!(cocoon_app_id(txt, "AUTRE-HOTE"), None);
        assert_eq!(cocoon_app_id("[moonlight_uuid] H\n", "H"), None);
        assert_eq!(cocoon_app_id("[moonlight_uuid] H\n[moonlight_id] 12a\n", "H"), None);
        assert_eq!(cocoon_file("Mario Kart 8 Deluxe · Ludothèque"), "Mario Kart 8 Deluxe · Ludothèque.txt");
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
        let inv = DeviceGame { game: "switch-mk8".into(), installed: false, stream: true, save: true, stream_app: Some("MK8 · Ludothèque".into()) };
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
}
