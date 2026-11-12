use lenso_project::{FileChange, Plan};
use std::fs;

fn file(path: &str, content: &str) -> FileChange {
    FileChange {
        path: path.into(),
        content: content.into(),
    }
}

#[test]
fn read_inputs_and_external_commands_are_bound_into_approval()
-> Result<(), Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("config.json"), "original")?;
    let mut evidence = lenso_runtime::Evidence::default();
    evidence.reads.insert(
        "config.json".into(),
        Some(format!("{:x}", Sha256::digest(b"original"))),
    );
    let plan = Plan::build(root.path(), vec![file("new.txt", "new")])?.with_evidence(evidence)?;
    let id = plan.id()?;
    let with_command = plan.clone().with_commands(vec![lenso_runtime::ProcessSpec {
        program: "npm".into(),
        args: vec!["install".into()],
    }]);
    assert!(
        with_command.apply(&id).is_err(),
        "adding a command must invalidate old approval"
    );
    fs::write(root.path().join("config.json"), "changed")?;
    assert!(
        plan.apply(&id).is_err(),
        "read-only inputs must also be checked after approval"
    );
    assert!(!root.path().join("new.txt").exists());
    Ok(())
}

#[test]
fn preview_is_read_only_and_stale_apply_writes_nothing() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("existing.txt"), "original\n")?;
    let plan = Plan::build(
        root.path(),
        vec![
            file("new.txt", "new\n"),
            file("existing.txt", "replacement\n"),
        ],
    )?;
    let id = plan.id()?;
    assert!(plan.preview().contains("+replacement"));
    assert!(!root.path().join("new.txt").exists());
    fs::write(root.path().join("existing.txt"), "changed after review\n")?;
    assert!(plan.apply(&id).is_err());
    assert!(
        !root.path().join("new.txt").exists(),
        "preflight must check every destination before writing"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("existing.txt"))?,
        "changed after review\n"
    );
    Ok(())
}

#[test]
fn exact_approval_is_required_and_new_files_can_be_applied()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let plan = Plan::build(root.path(), vec![file("src/hello.txt", "hello\n")])?;
    assert!(plan.apply("different approval").is_err());
    plan.apply(&plan.id()?)?;
    assert_eq!(
        fs::read_to_string(root.path().join("src/hello.txt"))?,
        "hello\n"
    );
    assert!(
        plan.apply(&plan.id()?).is_err(),
        "a consumed create plan must become stale"
    );
    Ok(())
}

#[test]
fn unsafe_paths_are_rejected_before_preview() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    for path in [
        "../escape",
        "/absolute",
        ".git/config",
        ".lenso/project.json",
        "",
    ] {
        assert!(
            Plan::build(root.path(), vec![file(path, "unsafe")]).is_err(),
            "must reject {path}"
        );
    }
    assert!(Plan::build(root.path(), vec![file("same", "a"), file("same", "b")]).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlink_destinations_and_newly_introduced_symlinks_are_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    symlink(outside.path(), root.path().join("link"))?;
    assert!(Plan::build(root.path(), vec![file("link/escape", "unsafe")]).is_err());
    let plan = Plan::build(root.path(), vec![file("future/file", "unsafe")])?;
    symlink(outside.path(), root.path().join("future"))?;
    assert!(plan.apply(&plan.id()?).is_err());
    assert!(!outside.path().join("file").exists());
    Ok(())
}
