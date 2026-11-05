// Compiles every i18n/<code>.toml into the binary as CATALOGS.
use std::{env, fs, path::Path};

fn main() {
    let dir = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("i18n");
    println!("cargo:rerun-if-changed={}", dir.display());

    let mut codes: Vec<String> = fs::read_dir(&dir)
        .expect("i18n/ directory")
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            name.strip_suffix(".toml").map(str::to_owned)
        })
        .collect();
    codes.sort();

    let mut out = String::from("pub static CATALOGS: &[(&str, &str)] = &[\n");
    for code in &codes {
        let path = dir.join(format!("{code}.toml"));
        println!("cargo:rerun-if-changed={}", path.display());
        out.push_str(&format!("    ({code:?}, include_str!({:?})),\n", path.display().to_string()));
    }
    out.push_str("];\n");
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("catalogs.rs"), out).unwrap();
}
