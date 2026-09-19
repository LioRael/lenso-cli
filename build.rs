fn main() {
    println!("cargo:rustc-check-cfg=cfg(generated_native_host)");
    let target = std::env::var("TARGET").expect("Cargo always sets TARGET for build scripts");
    println!("cargo:rustc-env=LENSO_CLI_BUILD_TARGET={target}");
}
