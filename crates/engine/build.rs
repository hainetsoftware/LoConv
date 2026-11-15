fn main() {
    println!("cargo:rustc-check-cfg=cfg(loconv_local_development)");
    // PROFILE remains release even if a packager enables debug assertions there.
    if std::env::var("PROFILE").as_deref() == Ok("debug") {
        println!("cargo:rustc-cfg=loconv_local_development");
    }
}
