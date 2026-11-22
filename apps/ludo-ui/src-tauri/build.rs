fn main() {
  placeholder_dist();
  tauri_build::build()
}

/// `tauri::generate_context!` exige le dossier `../dist` (front construit par `npm run build`).
/// Pour `cargo test` / `clippy` en débogage sans front construit, une page de remplacement est posée
/// (avertissement de compilation). Jamais en release : l'AppImage et l'APK passent par
/// `beforeBuildCommand` (vrai front) et une release sans front doit échouer, pas embarquer ce substitut.
fn placeholder_dist() {
  let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
  let dist = manifest.join("../dist");
  println!("cargo:rerun-if-changed={}", dist.display());
  if dist.exists() || std::env::var("PROFILE").as_deref() != Ok("debug") {
    return;
  }
  if std::fs::create_dir_all(&dist).is_ok()
    && std::fs::write(
      dist.join("index.html"),
      "<!doctype html><meta charset=\"utf-8\"><title>Ludothèque</title><p>Interface non construite : <code>npm run build</code> dans apps/ludo-ui.</p>\n",
    )
    .is_ok()
  {
    println!("cargo:warning=apps/ludo-ui/dist absent : page de remplacement posée pour la compilation de débogage (npm run build pour le vrai front)");
  }
}
