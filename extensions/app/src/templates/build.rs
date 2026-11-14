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
    let mut mounts = Vec::new();
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
        let descriptor = file.parent().ok_or("Missing host directory")?.join("host.toml");
        println!("cargo:rerun-if-changed={}",descriptor.display());
        let metadata: toml::Table = if descriptor.exists() { toml::from_str(&std::fs::read_to_string(&descriptor)?)? } else { toml::Table::new() };
        let modern = metadata.get("protocol").and_then(toml::Value::as_integer)==Some(2);
        if !metadata.is_empty() && !modern {return Err("Unsupported native module host protocol".into());}
        if modern {
            mounts.push((entry.file_name().to_string_lossy().into_owned(), format!("let mounted=module_{index}::mount(_migrate,&services).await?;\nif let Some(identity)=mounted.identity{{if services.identity.is_some(){{return Err(\"Multiple application identity providers\".into());}}services.identity=Some(identity);}}\nrouter=router.merge(mounted.router);\n"), metadata.get("requires_identity").and_then(toml::Value::as_bool).unwrap_or(false)));
        } else {
            mounts.push((entry.file_name().to_string_lossy().into_owned(), format!("router=router.merge(module_{index}::router(_migrate).await?);\n"), false));
        }
    }
    mounts.sort_by_key(|(_,_,requires)| *requires);
    let mut body=String::new();
    for (name,mount,requires) in mounts {
        if requires { body.push_str(&format!("if services.identity.is_none(){{return Err({:?}.into());}}\n",format!("Module {name} requires an identity provider"))); }
        body.push_str(&mount);
    }
    code.push_str(&format!("async fn mount_modules(_migrate:bool)->Result<axum::Router,Box<dyn std::error::Error>>{{\n#[allow(unused_mut)] let mut router=axum::Router::new();\n#[allow(unused_mut)] let mut services=crate::lenso_host::Services::default();\n{body}Ok(router)}}"));
    std::fs::write(
        PathBuf::from(std::env::var("OUT_DIR")?).join("modules.rs"),
        code,
    )?;
    Ok(())
}
