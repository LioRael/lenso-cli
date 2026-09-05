fn main() {
    let target = std::env::var("TARGET").expect("Cargo always sets TARGET for build scripts");
    println!("cargo:rustc-env=LENSO_CLI_BUILD_TARGET={target}");
}
