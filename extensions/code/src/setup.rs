use crate::{Result, error, question};
use extism_pdk::Json;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub enum Preparation {
    Ready {
        context: Value,
        selection: Option<Value>,
    },
    Question(Json<Value>),
}

pub fn prepare(ctx: &Value) -> Result<Preparation> {
    let options = &ctx["capabilities"]["extension.options"];
    let input = ctx["input"]
        .as_object()
        .ok_or_else(|| error("setup input must be an object"))?;
    let mut defaults = options["setup"].as_object().cloned().unwrap_or_default();
    let native_only = input.contains_key("native")
        && ![
            "repo",
            "author_root",
            "language",
            "target",
            "catalog",
            "ref",
            "format",
            "runtime",
        ]
        .iter()
        .any(|key| input.contains_key(*key))
        && options["registries"].is_object();
    if native_only {
        return Ok(Preparation::Ready {
            context: ctx.clone(),
            selection: None,
        });
    }
    let mut selection = None;
    if let Some(variants) = options["variants"].as_object() {
        if variants.is_empty() || variants.len() > 16 {
            return Err(error("configure between 1 and 16 target variants"));
        }
        let question_id = format!(
            "code:language:{:x}",
            Sha256::digest(serde_json::to_vec(variants)?)
        );
        let language = input
            .get("language")
            .or_else(|| ctx["answers"].get(&question_id))
            .or_else(|| options["selection"].get("language"));
        let Some(language) = language else {
            let details = variants
                .iter()
                .map(|(name, v)| {
                    format!(
                        "{name}: {}",
                        v["runtime"].as_str().unwrap_or("runtime not specified")
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            return Ok(Preparation::Question(question(
                &question_id,
                &format!("Choose the target language for this project ({details})"),
                json!({"type":"select","options":variants.keys().collect::<Vec<_>>()}),
            )));
        };
        let language = language
            .as_str()
            .ok_or_else(|| error("language must be a supported variant ID"))?;
        let variant = variants.get(language).ok_or_else(|| {
            error(format!(
                "unsupported target language {language}; choose {}",
                variants.keys().cloned().collect::<Vec<_>>().join(", ")
            ))
        })?;
        let runtime = variant["runtime"]
            .as_str()
            .ok_or_else(|| error("target variant runtime is missing"))?;
        if input
            .get("runtime")
            .is_some_and(|value| value.as_str() != Some(runtime))
        {
            return Err(error(format!(
                "unsupported runtime for {language}; this variant requires {runtime}"
            )));
        }
        let proposed = json!({"language":language,"runtime":runtime});
        if options["selection"].is_object() && options["selection"] != proposed {
            return Err(error(
                "changing language/runtime requires a new project target; existing generated code was not changed",
            ));
        }
        let setup = variant["setup"]
            .as_object()
            .ok_or_else(|| error("target variant setup is missing"))?;
        defaults.extend(setup.clone());
        selection = Some(proposed);
    } else if !defaults.contains_key("repo")
        && !defaults.contains_key("author_root")
        && !input.contains_key("repo")
        && !input.contains_key("author_root")
        && let Some(profiles) = options["registries"].as_object().filter(|p| p.len() == 1)
    {
        let value = profiles
            .values()
            .next()
            .ok_or_else(|| error("profile missing"))?;
        let p: crate::Profile = serde_json::from_value(value.clone())?;
        if let Some(repo) = p.source.strip_prefix("github-repo:") {
            defaults.insert("repo".into(), json!(repo));
        } else if let Some(root) = p.source.strip_prefix("project:") {
            defaults.insert("author_root".into(), json!(root));
        } else {
            return Err(error("unsupported preset profile source"));
        }
        defaults.insert("ref".into(), json!(p.r#ref));
        defaults.insert("catalog".into(), json!(p.path));
        defaults.insert("format".into(), json!(p.format));
        defaults.insert(
            "target".into(),
            json!(if p.target.is_empty() {
                "src".into()
            } else {
                p.target
            }),
        );
        if let Some(native) = options.get("native") {
            defaults.insert("native".into(), native.clone());
        }
    }
    defaults.extend(input.clone());
    let mut context = ctx.clone();
    context["input"] = Value::Object(defaults);
    Ok(Preparation::Ready { context, selection })
}

pub fn finish(original: &Value, selection: Option<Value>, result: &mut Value) {
    if result["status"] != "complete" {
        return;
    }
    let options = &original["capabilities"]["extension.options"];
    let installed = &mut result["output"]["installation"]["options"];
    for key in ["setup", "variants"] {
        if let Some(value) = options.get(key) {
            installed[key] = value.clone();
        }
    }
    if let Some(selection) = selection {
        installed["selection"] = selection;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context() -> Value {
        json!({"input":{},"answers":{},"capabilities":{"extension.options":{"setup":{"repo":"acme/modules","target":"src"},"variants":{"typescript":{"runtime":"node26","setup":{"catalog":"auth.typescript.toml"}},"rust":{"runtime":"tokio","setup":{"catalog":"auth.rust.toml"}}}}}})
    }
    #[test]
    fn language_is_an_explicit_domain_choice() {
        let mut ctx = context();
        ctx["input"]["yes"] = json!(true);
        let Preparation::Question(q) = prepare(&ctx).unwrap() else {
            panic!("language was silently selected")
        };
        assert!(
            q.0["question"]["id"]
                .as_str()
                .unwrap()
                .starts_with("code:language:")
        );
        assert_eq!(
            q.0["question"]["kind"]["options"],
            json!(["rust", "typescript"])
        );
    }
    #[test]
    fn selection_defaults_and_explicit_overrides_are_preserved() {
        let mut ctx = context();
        ctx["input"] = json!({"language":"rust","target":"custom"});
        let Preparation::Ready { context, selection } = prepare(&ctx).unwrap() else {
            panic!("missing preparation")
        };
        assert_eq!(context["input"]["repo"], "acme/modules");
        assert_eq!(context["input"]["catalog"], "auth.rust.toml");
        assert_eq!(context["input"]["target"], "custom");
        assert_eq!(selection.unwrap()["runtime"], "tokio");
    }
    #[test]
    fn existing_language_cannot_be_overwritten() {
        let mut ctx = context();
        ctx["input"]["language"] = json!("rust");
        ctx["capabilities"]["extension.options"]["selection"] =
            json!({"language":"typescript","runtime":"node26"});
        assert!(prepare(&ctx).is_err());
    }
    #[test]
    fn unsupported_language_has_no_fallback() {
        let mut ctx = context();
        ctx["input"]["language"] = json!("go");
        assert!(prepare(&ctx).is_err());
    }
    #[test]
    fn preset_profile_is_a_source_default() {
        let ctx = json!({"input":{},"answers":{},"capabilities":{"extension.options":{"registries":{"main":{"source":"github-repo:acme/modules","ref":"v1","path":"catalog.toml","target":"src"}}}}});
        let Preparation::Ready { context, .. } = prepare(&ctx).unwrap() else {
            panic!("source was asked again")
        };
        assert_eq!(context["input"]["repo"], "acme/modules");
        assert_eq!(context["input"]["ref"], "v1");
    }
}
