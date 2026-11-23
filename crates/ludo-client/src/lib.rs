//! Client de la ludothèque : moteur de synchronisation, adaptateurs de rangement par
//! émulateur, configuration d'appareil. Le binaire `ludo` (main.rs) s'appuie dessus.

pub mod adb;
pub mod api;
pub mod config;
pub mod detect;
pub mod engine;
pub mod install;
pub mod journal;
pub mod launch;
pub mod local;
pub mod watch;
pub mod layout;
pub mod shizuku;
