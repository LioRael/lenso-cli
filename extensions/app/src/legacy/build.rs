use std::path::{Component, Path, PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=lenso.code.toml");
    let config: toml::Value = toml::from_str(&std::fs::read_to_string("lenso.code.toml")?)?;
    let root = config
        .get("paths")
        .and_then(|v| v.get("modules"))
        .and_then(toml::Value::as_str)
        .ok_or("Missing paths.modules")?;
    if !root.starts_with("src/")
        || Path::new(root)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Modules must stay below src".into());
    }
    println!("cargo:rerun-if-changed={root}");
    let project = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?).canonicalize()?;
    let mut entries = match std::fs::read_dir(root) {
        Ok(entries) => entries.collect::<Result<Vec<_>, _>>()?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error.into()),
    };
    entries.sort_by_key(|entry| entry.file_name());
    let mut code = String::new();
    let mut mounts = String::new();
    for (index, entry) in entries.iter().enumerate() {
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let file = entry.path().join("assembly/host.rs");
        if !file.is_file() {
            continue;
        }
        let file = file.canonicalize()?;
        if !file.starts_with(&project) {
            return Err("Module host escapes project".into());
        }
        println!("cargo:rerun-if-changed={}", file.display());
        code.push_str(&format!(
            "#[path={:?}] pub mod module_{index};\n",
            file.to_str().ok_or("Invalid module path")?
        ));
        mounts.push_str(&format!(
            "router=router.merge(module_{index}::router(_migrate).await?);\n"
        ));
    }
    code.push_str(&format!("async fn mount_modules(_migrate:bool)->Result<axum::Router,Box<dyn std::error::Error>>{{\nlet router=axum::Router::new();\n{}Ok(router)}}",if mounts.is_empty(){String::new()}else{format!("let mut router=router;\n{mounts}")}));
    std::fs::write(
        PathBuf::from(std::env::var("OUT_DIR")?).join("modules.rs"),
        code,
    )?;
    Ok(())
}
