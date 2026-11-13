use lenso_codegen::{
    Dependency, Generator, GeneratorInput, GeneratorSpec, Scope, TemplateGenerator, relative,
    symbol,
};
use lenso_composition::{Catalog, Evaluation, Request};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Document {
    schema_version: u32,
    modules: BTreeMap<String, lenso_composition::Module>,
    generator: GeneratorSpec,
    samples: BTreeMap<String, Request>,
}
fn fixture(sample: &str) -> (GeneratorSpec, BTreeMap<String, String>, GeneratorInput) {
    let document: Document = toml::from_str(include_str!(
        "../../../examples/codegen-demo/author/catalog.toml"
    ))
    .unwrap();
    let catalog = Catalog {
        schema_version: document.schema_version,
        modules: document.modules,
    };
    let request = document.samples[sample].clone();
    let resolution = match catalog.evaluate(&request).unwrap() {
        Evaluation::Complete { resolution } => resolution,
        other => panic!("{other:?}"),
    };
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/codegen-demo/author");
    let templates = document
        .generator
        .files
        .iter()
        .map(|file| {
            (
                file.template.clone(),
                std::fs::read_to_string(root.join(&file.template)).unwrap(),
            )
        })
        .collect();
    (
        document.generator,
        templates,
        GeneratorInput {
            schema_version: 1,
            request,
            resolution,
        },
    )
}
#[test]
fn deterministic_factory_generation_uses_scoped_bindings_and_typed_configuration() {
    for sample in ["memory", "disk", "shared"] {
        let (spec, templates, input) = fixture(sample);
        let generator = TemplateGenerator {
            spec: &spec,
            templates: &templates,
        };
        let output = generator.generate(&input).unwrap();
        assert_eq!(output, generator.generate(&input).unwrap());
        assert!(
            output
                .files
                .iter()
                .any(|file| file.path == "assembly/app.rs")
        );
        assert!(output.files.iter().all(|file| !file.owners.is_empty()));
        if sample == "disk" {
            assert!(
                output
                    .files
                    .iter()
                    .find(|file| file.path == "instances/primary/factory.rs")
                    .unwrap()
                    .content
                    .contains("\"Welcome\"")
            );
        }
        if sample == "shared" {
            assert!(
                output
                    .files
                    .iter()
                    .any(|file| file.path == "instances/shared/factory.rs")
            );
            assert_eq!(
                output
                    .files
                    .iter()
                    .filter(|file| file.path == "instances/shared/factory.rs")
                    .count(),
                1
            );
        }
    }
}
#[test]
fn strict_undefined_and_bounded_rendering_reject_invalid_authors() {
    let (spec, mut templates, input) = fixture("memory");
    templates.insert(
        "templates/greeter.rs.j2".into(),
        "{{ instance.configuration.missing }}".into(),
    );
    assert!(
        TemplateGenerator {
            spec: &spec,
            templates: &templates
        }
        .generate(&input)
        .is_err()
    );
    templates.insert(
        "templates/greeter.rs.j2".into(),
        "{% for n in range(1000000) %}x{% endfor %}".into(),
    );
    assert!(
        TemplateGenerator {
            spec: &spec,
            templates: &templates
        }
        .generate(&input)
        .is_err()
    );
    templates.insert(
        "templates/greeter.rs.j2".into(),
        "{{ 'x' * 1048577 }}".into(),
    );
    assert!(
        TemplateGenerator {
            spec: &spec,
            templates: &templates
        }
        .generate(&input)
        .is_err()
    );
}
#[test]
fn portable_paths_filters_and_static_output_conflicts_are_checked() {
    for path in [
        "../business.rs",
        "/absolute",
        "C:/escape",
        "a/../b",
        "a\\b",
        "a/.git/c",
        "CON.rs",
        "a/file.",
    ] {
        assert!(relative(path).is_err(), "{path}");
    }
    assert_ne!(symbol("a.b"), symbol("a-b"));
    let (mut spec, templates, mut input) = fixture("memory");
    input
        .resolution
        .instances
        .get_mut("primary")
        .unwrap()
        .configuration
        .insert("prefix".into(), json!("a\"; panic!(); //\n\u{0008}"));
    let output = TemplateGenerator {
        spec: &spec,
        templates: &templates,
    }
    .generate(&input)
    .unwrap();
    assert!(
        output
            .files
            .iter()
            .find(|file| file.path == "instances/primary/factory.rs")
            .unwrap()
            .content
            .contains("\\\"; panic!(); //\\n\\u{8}")
    );
    spec.files.push(spec.files[0].clone());
    assert!(
        TemplateGenerator {
            spec: &spec,
            templates: &templates
        }
        .generate(&input)
        .is_err()
    );
    spec.files.pop();
    spec.files[0].scope = Scope::Module;
    spec.files[0].module = Some("missing".into());
    assert!(
        spec.validate(&["memory".into(), "greeter".into()].into())
            .is_err()
    );
}
#[test]
fn native_dependency_proposals_follow_selected_modules() {
    let (mut spec, templates, input) = fixture("memory");
    spec.dependencies = vec![
        Dependency {
            options: BTreeMap::new(),
            ecosystem: "cargo".into(),
            package: "store-driver".into(),
            version: "1".into(),
            module: Some("memory".into()),
        },
        Dependency {
            options: BTreeMap::new(),
            ecosystem: "cargo".into(),
            package: "store-driver".into(),
            version: "2".into(),
            module: Some("disk".into()),
        },
    ];
    spec.validate(&["memory".into(), "disk".into(), "greeter".into()].into())
        .unwrap();
    let output = TemplateGenerator {
        spec: &spec,
        templates: &templates,
    }
    .generate(&input)
    .unwrap();
    assert_eq!(output.dependencies.len(), 1);
    assert_eq!(output.dependencies[0].version, "1");
    spec.dependencies[1].module = Some("greeter".into());
    assert_eq!(
        TemplateGenerator {
            spec: &spec,
            templates: &templates
        }
        .generate(&input)
        .unwrap()
        .dependencies
        .len(),
        2
    );
}
