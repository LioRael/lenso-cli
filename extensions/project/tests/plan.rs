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

#[test]
fn deletion_is_previewed_bound_to_consent_and_guarded_by_preimages()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("old.txt"), "generated\n")?;
    let plan = Plan::build_operations(
        root.path(),
        vec![file("receipt.json", "new receipt")],
        vec!["old.txt".into()],
    )?;
    assert!(plan.preview().contains("Delete file: old.txt"));
    assert!(root.path().join("old.txt").exists());
    assert!(plan.apply("wrong approval").is_err());
    fs::write(root.path().join("old.txt"), "local edit\n")?;
    assert!(plan.apply(&plan.id()?).is_err());
    assert!(!root.path().join("receipt.json").exists());
    let plan = Plan::build_operations(
        root.path(),
        vec![file("receipt.json", "new receipt")],
        vec!["old.txt".into()],
    )?;
    let completed = plan.apply(&plan.id()?)?;
    assert_eq!(
        completed,
        vec![std::path::PathBuf::from("old.txt"), "receipt.json".into()]
    );
    assert!(!root.path().join("old.txt").exists());
    assert!(root.path().join("receipt.json").exists());
    assert!(
        Plan::build_operations(root.path(), vec![file("same", "new")], vec!["same".into()])
            .is_err()
    );
    for path in ["../escape", ".git/config", ".lenso/state", "/absolute"] {
        assert!(Plan::build_operations(root.path(), vec![], vec![path.into()]).is_err());
    }
    fs::create_dir(root.path().join("directory"))?;
    assert!(Plan::build_operations(root.path(), vec![], vec!["directory".into()]).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn deletion_rejects_a_symlink_introduced_after_review() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    fs::write(root.path().join("old"), "same")?;
    fs::write(outside.path().join("protected"), "same")?;
    let plan = Plan::build_operations(root.path(), vec![], vec!["old".into()])?;
    fs::remove_file(root.path().join("old"))?;
    symlink(outside.path().join("protected"), root.path().join("old"))?;
    assert!(plan.apply(&plan.id()?).is_err());
    assert!(outside.path().join("protected").exists());
    Ok(())
}

#[test]
fn completion_records_are_reviewed_and_written_only_in_the_second_phase()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let plan = Plan::build(
        root.path(),
        vec![
            file("manifest", "new dependency"),
            file("receipt", "pending"),
        ],
    )?
    .with_finalizations(vec![file("receipt", "installed")])?;
    assert!(plan.preview().contains("After successful commands"));
    let id = plan.id()?;
    plan.apply(&id)?;
    assert_eq!(fs::read_to_string(root.path().join("receipt"))?, "pending");
    assert!(plan.finalize("wrong").is_err());
    fs::write(root.path().join("manifest"), "modified during command")?;
    assert!(plan.finalize(&id).is_err());
    assert_eq!(fs::read_to_string(root.path().join("receipt"))?, "pending");
    fs::write(root.path().join("manifest"), "new dependency")?;
    plan.finalize(&id)?;
    assert_eq!(
        fs::read_to_string(root.path().join("receipt"))?,
        "installed"
    );
    assert!(
        Plan::build(root.path(), vec![file("manifest", "next")])?
            .with_finalizations(vec![file("unreviewed", "unsafe")])
            .is_err()
    );
    Ok(())
}
