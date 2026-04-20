use std::env;
use std::path::PathBuf;

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let output_path = PathBuf::from(&crate_dir).join("sd1disk.h");
    let config_path = PathBuf::from(&crate_dir).join("cbindgen.toml");

    cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(cbindgen::Config::from_file(config_path).unwrap())
        .generate()
        .expect("cbindgen failed to generate bindings")
        .write_to_file(output_path);
}
