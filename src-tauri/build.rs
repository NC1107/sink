use std::fmt::Write as _;
use std::path::Path;

/// Embed the community EQ presets (../presets/eq/*.json) into the binary:
/// generate `OUT_DIR/eq_presets_generated.rs` with one include_str! per
/// file. Compile-time embedding means zero packaging changes (the deb/rpm/
/// AUR/COPR payloads stay identical) and presets can never be missing at
/// runtime. Dropping a .json into presets/eq/ is picked up automatically.
fn embed_eq_presets() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let out_dir = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    let presets_dir = Path::new(&manifest_dir).join("../presets/eq");
    println!("cargo:rerun-if-changed={}", presets_dir.display());

    let mut entries: Vec<(String, String)> = Vec::new();
    if let Ok(dir) = std::fs::read_dir(&presets_dir) {
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            println!("cargo:rerun-if-changed={}", path.display());
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            entries.push((stem, path.display().to_string()));
        }
    }
    entries.sort();

    let mut code = String::from(
        "/// (file stem, raw JSON) of every bundled community EQ preset.\n\
         pub static BUNDLED_EQ_PRESET_SOURCES: &[(&str, &str)] = &[\n",
    );
    for (stem, path) in &entries {
        let _ = writeln!(code, "    ({stem:?}, include_str!({path:?})),");
    }
    code.push_str("];\n");

    std::fs::write(Path::new(&out_dir).join("eq_presets_generated.rs"), code)
        .expect("write eq presets codegen");
}

fn main() {
    embed_eq_presets();
    tauri_build::build()
}
