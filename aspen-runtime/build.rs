use std::env;
use std::path::PathBuf;

fn main() {
    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let output_file = crate_dir
        .parent()
        .unwrap()
        .join("target")
        .join(env::var("PROFILE").unwrap())
        .join("libaspenrt.h")
        .display()
        .to_string();

    cbindgen::generate(crate_dir)
        .expect("Unable to generate bindings")
        .write_to_file(output_file);
}
