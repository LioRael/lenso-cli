use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use lenso_cli::{
    Cli,
    serde_json::{Value, json},
};
use lenso_runtime::{LoadMode, LoadOptions, load_project_with_options};
use sha2::{Digest, Sha256};

struct Server {
    address: std::net::SocketAddr,
    files: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    requests: Arc<Mutex<Vec<String>>>,
    stopped: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let files = Arc::new(Mutex::new(BTreeMap::<String, Vec<u8>>::new()));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stopped = Arc::new(AtomicBool::new(false));
        let (bodies, observed, stop) = (files.clone(), requests.clone(), stopped.clone());
        let thread = std::thread::spawn(move || {
            for connection in listener.incoming() {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let mut connection = connection.unwrap();
                connection
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut header = Vec::new();
                let mut byte = [0];
                while !header.ends_with(b"\r\n\r\n") {
                    if connection.read(&mut byte).unwrap_or(0) == 0 {
                        break;
                    }
                    header.push(byte[0]);
                }
                let header = String::from_utf8(header).unwrap();
                let path = header.split_whitespace().nth(1).unwrap().to_owned();
                observed.lock().unwrap().push(path.clone());
                let bytes = bodies.lock().unwrap().get(&path).cloned();
                let status = if bytes.is_some() {
                    "200 OK"
                } else {
                    "404 Not Found"
                };
                let body = bytes.unwrap_or_default();
                write!(
                    connection,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                connection.write_all(&body).unwrap();
            }
        });
        Self {
            address,
            files,
            requests,
            stopped,
            thread: Some(thread),
        }
    }

    fn set(&self, path: &str, bytes: Vec<u8>) {
        self.files.lock().unwrap().insert(path.into(), bytes);
    }
    fn count(&self, path: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|value| value.as_str() == path)
            .count()
    }
    fn source(&self) -> String {
        format!("http://{}/extension.json", self.address)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

fn module() -> Vec<u8> {
    let output = br#"{"status":"complete","output":{"message":"downloaded"}}"#;
    let stores = output
        .iter()
        .enumerate()
        .map(|(offset, byte)| {
            format!("local.get $p i64.const {offset} i64.add i32.const {byte} call $store")
        })
        .collect::<Vec<_>>()
        .join("\n");
    wat::parse_str(format!(
        r#"(module
        (import "extism:host/env" "alloc" (func $alloc (param i64) (result i64)))
        (import "extism:host/env" "store_u8" (func $store (param i64 i32)))
        (import "extism:host/env" "output_set" (func $output (param i64 i64)))
        (func (export "run") (result i32) (local $p i64)
            i64.const {} call $alloc local.set $p
            {stores}
            local.get $p i64.const {} call $output i32.const 0))"#,
        output.len(),
        output.len()
    ))
    .unwrap()
}

fn manifest(bytes: &[u8], version: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"schema_version":1,"id":"example","version":version,
        "handlers":{"example/run":{"type":"wasm","module":"guest.wasm","function":"run","sha256":format!("{:x}",Sha256::digest(bytes))}},
        "workflows":[{"id":"example/run","steps":[{"id":"run","handler":"example/run"}]}],
        "commands":[{"path":["demo"],"workflow":"example/run","about":"Remote demo","provider":"example"}]
    })).unwrap()
}

fn project(root: &Path, source: &str) {
    std::fs::write(root.join("lenso.toml"),format!("[cli]\nschema_version=1\n[[cli.extensions]]\nid=\"example\"\nsource={source:?}\nversion=\"^0.1\"\n")).unwrap();
}

fn invoke(root: &Path, options: &LoadOptions) -> Result<Value, lenso_cli::Error> {
    let mut cli = Cli::new("test");
    load_project_with_options(&mut cli, root, options)?;
    let mut session =
        cli.engine_mut()
            .session("example/run", root, json!({}), Default::default())?;
    let report = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(session.advance(cli.engine_mut()))?;
    Ok(report.outputs["run"].clone())
}

#[test]
fn remote_first_execution_shares_cache_recovers_corruption_and_keeps_pins_offline() {
    let server = Server::new();
    let bytes = module();
    server.set("/extension.json", manifest(&bytes, "0.1.0"));
    server.set("/guest.wasm", bytes.clone());
    let cache = tempfile::tempdir().unwrap();
    let options = LoadOptions {
        cache_dir: Some(cache.path().into()),
        ..Default::default()
    };
    let first = tempfile::tempdir().unwrap();
    project(first.path(), &server.source());
    let mut help = Cli::new("test");
    load_project_with_options(
        &mut help,
        first.path(),
        &LoadOptions {
            mode: LoadMode::Discover,
            ..options.clone()
        },
    )
    .unwrap();
    assert_eq!(server.count("/extension.json"), 1);
    assert_eq!(
        server.count("/guest.wasm"),
        0,
        "help must only fetch metadata"
    );
    assert!(!first.path().join("lenso.lock").exists());
    assert_eq!(
        invoke(first.path(), &options).unwrap()["message"],
        "downloaded"
    );
    let lock = std::fs::read(first.path().join("lenso.lock")).unwrap();
    assert!(!first.path().join(".lenso").exists());
    assert_eq!(server.count("/guest.wasm"), 1);
    let second = tempfile::tempdir().unwrap();
    project(second.path(), &server.source());
    assert_eq!(
        invoke(second.path(), &options).unwrap()["message"],
        "downloaded"
    );
    assert_eq!(
        server.count("/guest.wasm"),
        1,
        "identical bytes must be shared across projects"
    );
    let blob = cache
        .path()
        .join("v1/blobs/sha256")
        .join(format!("{:x}", Sha256::digest(&bytes)));
    std::fs::write(&blob, b"corrupt").unwrap();
    invoke(first.path(), &options).unwrap();
    assert_eq!(server.count("/guest.wasm"), 2);
    server.set("/extension.json", manifest(&bytes, "0.1.1"));
    invoke(first.path(), &options).unwrap();
    assert_eq!(
        std::fs::read(first.path().join("lenso.lock")).unwrap(),
        lock,
        "warm execution must not silently update"
    );
    let offline = LoadOptions {
        offline: true,
        locked: true,
        ..options.clone()
    };
    invoke(first.path(), &offline).unwrap();
    let update = LoadOptions {
        mode: LoadMode::Update,
        ..options.clone()
    };
    invoke(first.path(), &update).unwrap();
    let updated: Value =
        serde_json::from_slice(&std::fs::read(first.path().join("lenso.lock")).unwrap()).unwrap();
    assert_eq!(updated["extensions"][0]["definition"]["version"], "0.1.1");
    std::fs::remove_file(&blob).unwrap();
    assert!(
        invoke(first.path(), &offline)
            .unwrap_err()
            .to_string()
            .contains("offline")
    );
    server.set("/guest.wasm", b"wrong digest".to_vec());
    assert!(
        invoke(first.path(), &options)
            .unwrap_err()
            .to_string()
            .contains("locked source module changed")
    );
    assert!(
        !blob.exists(),
        "failed downloads must not become valid cache entries"
    );
    server.set("/guest.wasm", bytes);
    let mut threads = Vec::new();
    let before = server.count("/guest.wasm");
    for _ in 0..4 {
        let options = options.clone();
        let project = first.path().to_owned();
        threads.push(std::thread::spawn(move || {
            invoke(&project, &options).unwrap()
        }));
    }
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(
        server.count("/guest.wasm"),
        before + 1,
        "concurrent misses must download only once"
    );
}

#[test]
fn locked_and_failed_resolution_leave_project_files_unchanged() {
    let server = Server::new();
    let bytes = module();
    server.set("/extension.json", manifest(&bytes, "0.1.0"));
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    project(root.path(), &server.source());
    let options = LoadOptions {
        locked: true,
        cache_dir: Some(cache.path().into()),
        ..Default::default()
    };
    let mut cli = Cli::new("test");
    assert!(
        load_project_with_options(&mut cli, root.path(), &options)
            .unwrap_err()
            .to_string()
            .contains("--locked")
    );
    assert_eq!(server.count("/extension.json"), 0);
    assert!(!root.path().join("lenso.lock").exists());
    server.set("/extension.json", manifest(&bytes, "1.0.0"));
    assert!(
        load_project_with_options(
            &mut cli,
            root.path(),
            &LoadOptions {
                locked: false,
                ..options.clone()
            }
        )
        .unwrap_err()
        .to_string()
        .contains("does not satisfy")
    );
    assert!(!root.path().join("lenso.lock").exists());
    assert_eq!(
        cli.engine_mut().workflows().count(),
        0,
        "failed loading must preserve host registry"
    );
    server.set("/extension.json", manifest(&bytes, "0.1.0"));
    let unlocked = LoadOptions {
        locked: false,
        ..options.clone()
    };
    load_project_with_options(&mut cli, root.path(), &unlocked).unwrap();
    let before = std::fs::read(root.path().join("lenso.lock")).unwrap();
    let config = root.path().join("lenso.toml");
    std::fs::write(
        &config,
        std::fs::read_to_string(&config)
            .unwrap()
            .replace("^0.1", "^0.2"),
    )
    .unwrap();
    server.set("/extension.json", manifest(&bytes, "0.2.0"));
    assert!(load_project_with_options(&mut Cli::new("test"), root.path(), &options).is_err());
    assert_eq!(
        std::fs::read(root.path().join("lenso.lock")).unwrap(),
        before
    );
    load_project_with_options(&mut Cli::new("test"), root.path(), &unlocked).unwrap();
    let before = std::fs::read(root.path().join("lenso.lock")).unwrap();
    let pin: Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(
        pin["extensions"][0]["definition"]["version"], "0.2.0",
        "changed declarations must refresh source metadata"
    );
    let mut conflicting: Value = serde_json::from_slice(&manifest(&bytes, "0.2.0")).unwrap();
    conflicting["handlers"]["example/other"] = conflicting["handlers"]["example/run"].clone();
    conflicting["handlers"]["example/other"]["sha256"] = json!("0".repeat(64));
    server.set("/extension.json", serde_json::to_vec(&conflicting).unwrap());
    let update = LoadOptions {
        mode: LoadMode::Update,
        ..unlocked
    };
    assert!(
        load_project_with_options(&mut Cli::new("test"), root.path(), &update)
            .unwrap_err()
            .to_string()
            .contains("conflicting SHA-256")
    );
    assert_eq!(
        std::fs::read(root.path().join("lenso.lock")).unwrap(),
        before
    );
}

fn registry_project(root: &Path, server: &Server) {
    std::fs::write(root.join("lenso.toml"), format!("[cli]\nschema_version=1\nregistries={{\"@test\"=\"http://{}/index/\"}}\n[[cli.extensions]]\nid=\"example\"\nsource=\"registry:@test/example\"\nversion=\"^0.1\"\n", server.address)).unwrap();
}

fn releases(server: &Server, versions: &[(&str, bool)]) -> Value {
    let bytes = module();
    let releases: Vec<_> = versions.iter().map(|(version, yanked)| {
        let manifest = manifest(&bytes, version);
        server.set(&format!("/packages/{version}/extension.json"), manifest.clone());
        server.set(&format!("/packages/{version}/guest.wasm"), bytes.clone());
        json!({"version":version,"manifest":format!("../packages/{version}/extension.json"),"sha256":format!("{:x}",Sha256::digest(&manifest)),"yanked":yanked})
    }).collect();
    json!({"schema_version":1,"name":"@test/example","releases":releases})
}

#[test]
fn registry_names_resolve_versions_pin_sources_share_cache_and_update_explicitly() {
    let server = Server::new();
    server.set(
        "/index/example.json",
        serde_json::to_vec(&releases(
            &server,
            &[
                ("0.1.0", false),
                ("0.1.2", false),
                ("0.1.3", true),
                ("0.1.4-beta.1", false),
                ("0.2.0", false),
            ],
        ))
        .unwrap(),
    );
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let options = LoadOptions {
        cache_dir: Some(cache.path().into()),
        ..Default::default()
    };
    registry_project(root.path(), &server);
    let mut cli = Cli::new("test");
    load_project_with_options(
        &mut cli,
        root.path(),
        &LoadOptions {
            mode: LoadMode::Discover,
            ..options.clone()
        },
    )
    .unwrap();
    assert!(!root.path().join("lenso.lock").exists());
    assert_eq!(server.count("/packages/0.1.2/guest.wasm"), 0);
    assert_eq!(
        invoke(root.path(), &options).unwrap()["message"],
        "downloaded"
    );
    let pinned = std::fs::read(root.path().join("lenso.lock")).unwrap();
    let lock: Value = serde_json::from_slice(&pinned).unwrap();
    assert_eq!(lock["extensions"][0]["source"], "registry:@test/example");
    assert_eq!(lock["extensions"][0]["definition"]["version"], "0.1.2");
    assert!(
        lock["extensions"][0]["resolved_source"]
            .as_str()
            .unwrap()
            .ends_with("/packages/0.1.2/extension.json")
    );
    assert_eq!(server.count("/index/example.json"), 1);
    let second = tempfile::tempdir().unwrap();
    registry_project(second.path(), &server);
    invoke(second.path(), &options).unwrap();
    assert_eq!(server.count("/packages/0.1.2/guest.wasm"), 1);
    server.set(
        "/index/example.json",
        serde_json::to_vec(&releases(&server, &[("0.1.4", false)])).unwrap(),
    );
    invoke(root.path(), &options).unwrap();
    invoke(
        root.path(),
        &LoadOptions {
            offline: true,
            locked: true,
            ..options.clone()
        },
    )
    .unwrap();
    assert_eq!(
        std::fs::read(root.path().join("lenso.lock")).unwrap(),
        pinned
    );
    assert_eq!(server.count("/index/example.json"), 1);
    invoke(
        root.path(),
        &LoadOptions {
            mode: LoadMode::Update,
            ..options.clone()
        },
    )
    .unwrap();
    let updated: Value =
        serde_json::from_slice(&std::fs::read(root.path().join("lenso.lock")).unwrap()).unwrap();
    assert_eq!(updated["extensions"][0]["definition"]["version"], "0.1.4");
    assert_eq!(server.count("/index/example.json"), 2);
    let declaration = root.path().join("lenso.toml");
    std::fs::write(
        &declaration,
        std::fs::read_to_string(&declaration)
            .unwrap()
            .replace("/index/", "/other/"),
    )
    .unwrap();
    assert!(
        invoke(
            root.path(),
            &LoadOptions {
                locked: true,
                ..options
            }
        )
        .unwrap_err()
        .to_string()
        .contains("declarations changed")
    );
}

#[test]
fn invalid_registry_metadata_never_replaces_an_existing_lock_or_executes_modules() {
    let server = Server::new();
    let valid = releases(&server, &[("0.1.0", false)]);
    server.set("/index/example.json", serde_json::to_vec(&valid).unwrap());
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let options = LoadOptions {
        cache_dir: Some(cache.path().into()),
        ..Default::default()
    };
    registry_project(root.path(), &server);
    invoke(root.path(), &options).unwrap();
    let pinned = std::fs::read(root.path().join("lenso.lock")).unwrap();
    let mut bad_identity = valid.clone();
    bad_identity["name"] = json!("@other/example");
    let mut bad_digest = valid.clone();
    bad_digest["releases"][0]["sha256"] = json!("0".repeat(64));
    let mut bad_origin = valid.clone();
    bad_origin["releases"][0]["manifest"] = json!("https://other.example/extension.json");
    let mut yanked = valid.clone();
    yanked["releases"][0]["yanked"] = json!(true);
    let mut duplicate = valid.clone();
    duplicate["releases"]
        .as_array_mut()
        .unwrap()
        .push(valid["releases"][0].clone());
    let mut version = valid.clone();
    version["releases"][0]["version"] = json!("0.1.1");
    for bad in [
        bad_identity,
        bad_digest,
        bad_origin,
        yanked,
        duplicate,
        version,
    ] {
        server.set("/index/example.json", serde_json::to_vec(&bad).unwrap());
        assert!(
            invoke(
                root.path(),
                &LoadOptions {
                    mode: LoadMode::Update,
                    ..options.clone()
                }
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read(root.path().join("lenso.lock")).unwrap(),
            pinned
        );
    }
    assert_eq!(server.count("/packages/0.1.0/guest.wasm"), 1);
    let unknown = tempfile::tempdir().unwrap();
    std::fs::write(unknown.path().join("lenso.toml"),"[cli]\nschema_version=1\n[[cli.extensions]]\nid=\"example\"\nsource=\"registry:@missing/example\"\n").unwrap();
    assert!(
        invoke(unknown.path(), &options)
            .unwrap_err()
            .to_string()
            .contains("not configured")
    );
    assert!(!unknown.path().join("lenso.lock").exists());
}

#[test]
fn dependency_solver_backtracks_locks_edges_and_rejects_cycles_without_writes() {
    let server = Server::new();
    let root = tempfile::tempdir().unwrap();
    let base = format!("http://{}/registry/", server.address);
    let put = |id: &str, versions: Vec<(&str, Value)>| {
        let releases=versions.into_iter().map(|(version,deps)|{
            let body=serde_json::to_vec(&json!({"schema_version":1,"id":id,"version":version,"dependencies":deps,"handlers":{},"workflows":[],"commands":[]})).unwrap();
            let path=format!("/packages/{id}/{version}.json");
            server.set(&path,body.clone());
            json!({"version":version,"manifest":path,"sha256":format!("{:x}",Sha256::digest(&body))})
        }).collect::<Vec<_>>();
        server.set(
            &format!("/registry/{id}.json"),
            serde_json::to_vec(
                &json!({"schema_version":1,"name":format!("@test/{id}"),"releases":releases}),
            )
            .unwrap(),
        );
    };
    put(
        "a",
        vec![
            (
                "1.1.0",
                json!({"shared":{"source":"registry:@test/shared","version":"^2"}}),
            ),
            (
                "1.0.0",
                json!({"shared":{"source":"registry:@test/shared","version":"^1"}}),
            ),
        ],
    );
    put("shared", vec![("1.0.0", json!({})), ("2.0.0", json!({}))]);
    put(
        "z",
        vec![(
            "1.0.0",
            json!({"shared":{"source":"registry:@test/shared","version":"^1"}}),
        )],
    );
    let config:lenso_runtime::Project=serde_json::from_value(json!({"cli":{"schema_version":1,"registries":{"@test":base},"extensions":[{"id":"a","source":"registry:@test/a","version":"^1"},{"id":"z","source":"registry:@test/z","version":"^1"}]}})).unwrap();
    std::fs::write(
        root.path().join("lenso.toml"),
        toml::to_string(&config).unwrap(),
    )
    .unwrap();
    let mut options = LoadOptions {
        cache_dir: Some(root.path().join("cache")),
        ..Default::default()
    };
    lenso_runtime::prepare_project(root.path(), &options).unwrap();
    let before = std::fs::read(root.path().join("lenso.lock")).unwrap();
    let lock: Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(lock["extensions"].as_array().unwrap().len(), 3);
    assert_eq!(lock["extensions"][0]["definition"]["version"], "1.0.0");
    assert_eq!(
        lock["extensions"][0]["definition"]["dependencies"]["shared"]["version"],
        "^1"
    );
    options.offline = true;
    options.locked = true;
    lenso_runtime::prepare_project(root.path(), &options).unwrap();
    assert_eq!(
        std::fs::read(root.path().join("lenso.lock")).unwrap(),
        before
    );
    put(
        "a",
        vec![(
            "1.2.0",
            json!({"shared":{"source":"registry:@test/shared","version":"^1"}}),
        )],
    );
    let mut changed = config.clone();
    changed.cli.extensions.push(
        lenso_runtime::Installation::new(
            "shared".into(),
            "registry:@test/shared".into(),
            Some("^1".into()),
        )
        .unwrap(),
    );
    std::fs::write(
        root.path().join("lenso.toml"),
        toml::to_string(&changed).unwrap(),
    )
    .unwrap();
    options.offline = false;
    options.locked = false;
    lenso_runtime::prepare_project(root.path(), &options).unwrap();
    let before = std::fs::read(root.path().join("lenso.lock")).unwrap();
    let pinned: Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(
        pinned["extensions"][0]["definition"]["version"], "1.0.0",
        "adding another root must preserve compatible pins"
    );
    put(
        "shared",
        vec![(
            "1.0.0",
            json!({"a":{"source":"registry:@test/a","version":"^1"}}),
        )],
    );
    options.offline = false;
    options.locked = false;
    options.mode = LoadMode::Update;
    let error = lenso_runtime::prepare_project(root.path(), &options)
        .unwrap_err()
        .to_string();
    assert!(error.contains("cycle"), "{error}");
    assert_eq!(
        std::fs::read(root.path().join("lenso.lock")).unwrap(),
        before
    );
}
