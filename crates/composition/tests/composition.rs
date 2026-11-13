use lenso_composition::{
    Binding, Catalog, Choice, Evaluation, OptionKind, OptionSpec, Request, Resolution,
};
use serde_json::json;

fn catalog() -> Catalog {
    let mut document: toml::Table =
        toml::from_str(include_str!("../../../fixtures/composition/catalog.toml")).unwrap();
    document.remove("resources");
    document.try_into().unwrap()
}
fn request() -> Request {
    serde_json::from_value(json!({
        "instances":{"auth":"auth"}, "context":{"language":"typescript"},
        "bindings":{"auth.store":{"type":"module","module":"sqlite"}}
    }))
    .unwrap()
}
fn resolved(catalog: &Catalog, request: &Request) -> Resolution {
    match catalog.evaluate(request).unwrap() {
        Evaluation::Complete { resolution } => resolution,
        other => panic!("unexpected question: {other:?}"),
    }
}

#[test]
fn independent_instances_and_explicit_sharing() {
    let catalog = catalog();
    let request: Request = serde_json::from_value(json!({
        "instances":{"public":"auth","staff":"auth","shared":"argon2"},
        "context":{"language":"typescript"},
        "configuration":{"staff.store":{"connection":{"env":"STAFF_DATABASE_URL"}}},
        "bindings":{
            "public.store":{"type":"module","module":"sqlite"},
            "staff.store":{"type":"module","module":"pg"},
            "public.hasher":{"type":"instance","instance":"shared"},
            "staff.hasher":{"type":"instance","instance":"shared"}
        }
    }))
    .unwrap();
    let resolution = resolved(&catalog, &request);
    assert_eq!(resolution.instances.len(), 5);
    assert_eq!(resolution.instances["staff.store"].module, "pg");
    assert_eq!(resolution.instances["public.store"].module, "sqlite");
    assert_eq!(
        resolution.instances["staff"].bindings["hasher"].as_deref(),
        Some("shared")
    );
    assert_eq!(
        resolution.order.iter().filter(|id| *id == "shared").count(),
        1
    );
    let index = |id| {
        resolution
            .order
            .iter()
            .position(|value| value == id)
            .unwrap()
    };
    assert!(index("shared") < index("staff"));
    assert_eq!(
        resolution.digest().unwrap(),
        resolved(&catalog, &request).digest().unwrap()
    );
}

#[test]
fn choices_are_scoped_and_optional_cache_is_explicit() {
    let catalog = catalog();
    let mut request = request();
    request.bindings.clear();
    assert!(
        matches!(catalog.evaluate(&request).unwrap(), Evaluation::NeedsInput {
        choice: Choice::Provider { slot, modules, optional: false, .. }
    } if slot == "auth.store" && modules == vec!["pg", "sqlite"])
    );
    request = self::request();
    request.configuration.insert(
        "auth".into(),
        [("cache-enabled".into(), json!(true))].into(),
    );
    assert!(
        matches!(catalog.evaluate(&request).unwrap(), Evaluation::NeedsInput {
        choice: Choice::Provider { slot, optional: true, .. }
    } if slot == "auth.cache")
    );
    request
        .bindings
        .insert("auth.cache".into(), Binding::Disabled);
    assert_eq!(
        resolved(&catalog, &request).instances["auth"].bindings["cache"],
        None
    );
    request.bindings.insert(
        "auth.cache".into(),
        Binding::Module {
            module: "redis".into(),
        },
    );
    assert!(
        matches!(catalog.evaluate(&request).unwrap(), Evaluation::NeedsInput {
        choice: Choice::Configuration { instance, field, .. }
    } if instance == "auth.cache" && field == "connection")
    );
    request.configuration.insert(
        "auth.cache".into(),
        [("connection".into(), json!({"env":"REDIS_URL"}))].into(),
    );
    let enabled = resolved(&catalog, &request);
    request
        .configuration
        .get_mut("auth")
        .unwrap()
        .insert("cache-enabled".into(), json!(false));
    let disabled = resolved(&catalog, &request);
    assert!(!disabled.instances.contains_key("auth.cache"));
    assert_ne!(enabled.digest().unwrap(), disabled.digest().unwrap());
    request
        .configuration
        .get_mut("auth")
        .unwrap()
        .insert("cache-enabled".into(), json!(true));
    assert_eq!(
        enabled.digest().unwrap(),
        resolved(&catalog, &request).digest().unwrap()
    );
}

#[test]
fn incompatible_bindings_and_cycles_are_rejected() {
    let mut catalog = catalog();
    let mut request = request();
    catalog
        .modules
        .get_mut("sqlite")
        .unwrap()
        .provides
        .get_mut("example.auth-store")
        .unwrap()
        .version = "2.0.0".parse().unwrap();
    assert!(
        catalog
            .evaluate(&request)
            .unwrap_err()
            .to_string()
            .contains("auth.store")
    );
    catalog = self::catalog();
    catalog
        .modules
        .get_mut("argon2")
        .unwrap()
        .provides
        .get_mut("example.password-hash")
        .unwrap()
        .attributes
        .clear();
    assert!(
        catalog
            .evaluate(&request)
            .unwrap_err()
            .to_string()
            .contains("auth.hasher")
    );
    catalog = self::catalog();
    let cyclic = catalog.modules["auth"].slots["store"].clone();
    catalog
        .modules
        .get_mut("sqlite")
        .unwrap()
        .slots
        .insert("self".into(), cyclic);
    request.instances.insert("db".into(), "sqlite".into());
    request.bindings.insert(
        "db.self".into(),
        Binding::Instance {
            instance: "db".into(),
        },
    );
    request.bindings.insert(
        "auth.store".into(),
        Binding::Instance {
            instance: "db".into(),
        },
    );
    assert!(
        catalog
            .evaluate(&request)
            .unwrap_err()
            .to_string()
            .contains("composition cycle")
    );
}

#[test]
fn typed_inputs_context_and_graph_limits_are_enforced() {
    let catalog = catalog();
    let mut request = request();
    request.context.clear();
    assert!(matches!(
        catalog.evaluate(&request).unwrap(),
        Evaluation::NeedsInput {
            choice: Choice::Context { .. }
        }
    ));
    request.context.insert("language".into(), "rust".into());
    assert!(
        catalog
            .evaluate(&request)
            .unwrap_err()
            .to_string()
            .contains("unsupported context")
    );
    request = self::request();
    request
        .configuration
        .insert("unknown".into(), Default::default());
    assert!(
        catalog
            .evaluate(&request)
            .unwrap_err()
            .to_string()
            .contains("unknown")
    );
    let environment = OptionSpec {
        prompt: "Connection variable".into(),
        kind: OptionKind::Environment,
        default: None,
    };
    assert!(environment.accepts(&json!({"env":"DATABASE_URL"})));
    for value in [
        json!("postgres://secret"),
        json!({"env":"1BAD"}),
        json!({"env":"OK","value":"secret"}),
    ] {
        assert!(!environment.accepts(&value));
    }
    let mut recursive = catalog;
    let slot = recursive.modules["auth"].slots["store"].clone();
    recursive
        .modules
        .get_mut("sqlite")
        .unwrap()
        .slots
        .insert("next".into(), slot);
    recursive.modules.remove("pg");
    assert!(
        recursive
            .evaluate(&self::request())
            .unwrap_err()
            .to_string()
            .contains("limit")
    );
}
