fn main() {
    println!("cargo:rerun-if-changed=exports.def");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let directory = std::env::var("CARGO_MANIFEST_DIR").expect("manifest directory");
        println!("cargo:rustc-cdylib-link-arg=/DEF:{directory}/exports.def");
    }
}
