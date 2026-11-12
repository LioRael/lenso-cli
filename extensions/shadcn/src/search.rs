use extism_pdk::Json;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::layout::{file, normalize};
use crate::registry::Reference;
use crate::{Result, complete, error, get, namespace_request, question, read, workspace_paths};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    total: usize,
    offset: usize,
    limit: usize,
    has_more: bool,
}

fn number(ctx: &Value, name: &str, default: usize, min: usize, max: usize) -> Result<usize> {
    let value = &ctx["input"][name];
    let parsed = if value.is_null() {
        default
    } else if let Some(text) = value.as_str() {
        text.parse()
            .map_err(|_| error(format!("{name} must be an integer")))?
    } else {
        value
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| error(format!("{name} must be an integer")))?
    };
    if !(min..=max).contains(&parsed) {
        return Err(error(format!("{name} must be between {min} and {max}")));
    }
    Ok(parsed)
}

fn text<'a>(ctx: &'a Value, name: &str) -> Result<&'a str> {
    match ctx["input"].get(name) {
        None => Ok(""),
        Some(value) => value
            .as_str()
            .ok_or_else(|| error(format!("{name} must be a string"))),
    }
}

fn config(ctx: &Value) -> Result<std::result::Result<String, Json<Value>>> {
    let mut candidates = Vec::new();
    for directory in std::iter::once(String::new()).chain(workspace_paths()?) {
        if read(&file(&directory, "components.json"))?.is_some() {
            candidates.push(directory);
        }
    }
    let requested = ctx["input"]["cwd"]
        .as_str()
        .or_else(|| ctx["answers"]["workspace"].as_str());
    if let Some(requested) = requested {
        let directory = normalize("", requested)?;
        if !candidates.contains(&directory) {
            return Err(error("selected workspace needs components.json"));
        }
        return Ok(Ok(file(&directory, "components.json")));
    }
    if let Some(cwd) = ctx["capabilities"]["host.cwd"].as_str()
        && let Some(directory) = candidates
            .iter()
            .filter(|dir| {
                !dir.is_empty() && (cwd == dir.as_str() || cwd.starts_with(&format!("{dir}/")))
            })
            .max_by_key(|dir| dir.len())
    {
        return Ok(Ok(file(directory, "components.json")));
    }
    if candidates.contains(&String::new()) {
        return Ok(Ok("components.json".into()));
    }
    if candidates.len() == 1 {
        return Ok(Ok(file(&candidates[0], "components.json")));
    }
    if candidates.is_empty() {
        return Err(error("namespace search needs components.json"));
    }
    Ok(Err(question(
        "workspace",
        "Choose the app or workspace Registry configuration".into(),
        json!({"type":"select","options":candidates}),
    )))
}

pub fn search(ctx: &Value) -> Result<Json<Value>> {
    let registries: Vec<String> = match ctx["input"].get("registry") {
        None => vec!["@shadcn".into()],
        Some(Value::String(value)) => vec![value.clone()],
        Some(value) => serde_json::from_value(value.clone())?,
    };
    if registries.is_empty() || registries.len() > 8 {
        return Err(error("search between 1 and 8 registries"));
    }
    let query = text(ctx, "query")?;
    let types = text(ctx, "type")?;
    if query.len() > 1024 || types.len() > 256 {
        return Err(error("search query or type is too long"));
    }
    let limit = number(ctx, "limit", 100, 1, 1000)?;
    let offset = number(ctx, "offset", 0, 0, 10_000)?;
    let multiple = registries.len() > 1;
    let requested_limit = if multiple { offset + limit } else { limit };
    let requested_offset = if multiple { 0 } else { offset };
    let needs_config = registries
        .iter()
        .any(|name| name.starts_with('@') && name != "@shadcn");
    let config = if needs_config {
        match config(ctx)? {
            Ok(path) => path,
            Err(question) => return Ok(question),
        }
    } else {
        String::new()
    };
    let mut results = Vec::new();
    let mut total: usize = 0;
    let mut more = false;
    for registry in registries {
        let (catalog, display, base) = if registry == "@shadcn" {
            (
                get("https://ui.shadcn.com/r/index.json")?,
                registry.clone(),
                None,
            )
        } else if registry.starts_with('@') {
            Reference::parse(&format!("{registry}/registry"), None)?;
            (
                namespace_request(
                    json!({"config":config,"registry":registry,"name":"registry","search":{"query":query,"type":types,"limit":requested_limit,"offset":requested_offset}}),
                )?,
                registry.clone(),
                None,
            )
        } else {
            let Reference::Remote(mut url) = Reference::parse(&registry, None)? else {
                return Err(error("search expects @namespace or a catalog URL"));
            };
            if url
                .query_pairs()
                .any(|(key, _)| matches!(key.as_ref(), "q" | "type" | "limit" | "offset"))
            {
                return Err(error("catalog URL already contains search parameters"));
            }
            let mut base = url.clone();
            base.set_query(None);
            let display = base.to_string();
            url.query_pairs_mut()
                .append_pair("q", query)
                .append_pair("type", types)
                .append_pair("limit", &requested_limit.to_string())
                .append_pair("offset", &requested_offset.to_string());
            (get(url.as_str())?, display, Some(base))
        };
        let items = catalog
            .as_array()
            .or_else(|| catalog["items"].as_array())
            .ok_or_else(|| error("invalid Registry catalog: expected items"))?;
        if items.len() > 20_000 {
            return Err(error("Registry catalog exceeds 20000 items"));
        }
        let pagination = catalog
            .get("pagination")
            .map(|value| serde_json::from_value::<Pagination>(value.clone()))
            .transpose()?;
        let dynamic = pagination.is_some();
        if let Some(page) = &pagination
            && (page.offset != requested_offset
                || page.limit == 0
                || page.limit > requested_limit
                || items.len() > page.limit
                || (!items.is_empty() && page.total < page.offset.saturating_add(items.len())))
        {
            return Err(error("invalid Registry search pagination"));
        }
        let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut rows = Vec::new();
        for item in items {
            let name = item["name"]
                .as_str()
                .ok_or_else(|| error("Registry catalog item needs a name"))?;
            Reference::parse(&format!("@catalog/{name}"), None)?;
            if name.len() > 256 {
                return Err(error("Registry item name is too long"));
            }
            let kind = item["type"].as_str().unwrap_or("registry:ui");
            let description = item["description"].as_str().unwrap_or("");
            let title = item["title"].as_str().unwrap_or("");
            if kind.len() > 128 || description.len() > 4096 || title.len() > 512 {
                return Err(error("Registry catalog text is too long"));
            }
            if !dynamic {
                let text =
                    format!("{name} {title} {description} {}", item["categories"]).to_lowercase();
                if !terms.iter().all(|term| text.contains(term))
                    || (!types.is_empty() && !types.split(',').any(|t| t.trim() == kind))
                {
                    continue;
                }
            }
            let add = if display == "@shadcn" {
                name.to_owned()
            } else if let Some(base) = &base {
                base.join(&format!("{name}.json"))?.to_string()
            } else {
                format!("{display}/{name}")
            };
            rows.push(json!({"name":name,"type":kind,"title":title,"description":description,"registry":display,"add":add}));
        }
        if let Some(page) = pagination {
            total = total
                .checked_add(page.total)
                .ok_or_else(|| error("Registry search total is too large"))?;
            more |= page.has_more;
            results.extend(rows);
        } else {
            total = total
                .checked_add(rows.len())
                .ok_or_else(|| error("Registry search total is too large"))?;
            more |= rows.len() > requested_offset + requested_limit;
            results.extend(
                rows.into_iter()
                    .skip(requested_offset)
                    .take(requested_limit),
            );
        }
    }
    if multiple {
        results = results.into_iter().skip(offset).take(limit).collect();
    }
    more |= offset + results.len() < total;
    Ok(complete(
        json!({"results":results,"pagination":{"total":total,"offset":offset,"limit":limit,"hasMore":more}}),
    ))
}
