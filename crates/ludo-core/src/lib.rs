//! Cœur de la ludothèque, partagé par le démon Linux et l'app Android.
//!
//! - [`sync`] : la décision de synchronisation (pure, sans entrées-sorties) ;
//! - [`snapshot`] : empreinte, archivage et restauration atomique d'un dossier de sauvegarde ;
//! - [`fiche`] : les fiches TOML qui décrivent chaque émulateur.

pub mod fiche;
pub mod rom;
pub mod snapshot;
pub mod sync;
