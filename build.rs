//! Build-time codegen of the explorer API client from
//! `docs/generated/openapi.json`. The generated module is included by
//! `src/domains/explorer/generated.rs`.
//!
//! The explorer ships an OpenAPI 3.1 spec, but the `openapiv3` crate (which
//! `progenitor` depends on) only supports 3.0.x. We preprocess the spec
//! in-memory to downgrade the two 3.1-only patterns we actually use:
//!
//!   - `"type": ["X", "null"]`   →   `"type": "X", "nullable": true`
//!   - the spec's `"openapi": "3.1.0"`   →   `"openapi": "3.0.3"`
//!
//! See `docs/exec-plans/active/002-progenitor-codegen.md`.

use std::{env, fs, path::PathBuf};

use serde_json::Value;

fn main() {
    let spec_path = "docs/generated/openapi.json";
    println!("cargo:rerun-if-changed={spec_path}");
    println!("cargo:rerun-if-changed=build.rs");

    let spec_str = fs::read_to_string(spec_path).expect("read openapi.json");
    let mut spec_json: Value = serde_json::from_str(&spec_str).expect("parse openapi.json as JSON");

    // 3.1 → 3.0 downgrade so openapiv3 can parse.
    if let Some(obj) = spec_json.as_object_mut() {
        obj.insert("openapi".into(), Value::String("3.0.3".into()));
    }
    downgrade_type_arrays(&mut spec_json);
    collapse_nullable_oneof(&mut spec_json);

    // The explorer's operationIds collide across paths (e.g. `list` on both
    // /events and /delegators). Progenitor requires unique IDs to derive
    // unique method names, so we synthesize a path-derived ID for every
    // operation. Stable across spec updates as long as paths don't change.
    rewrite_operation_ids(&mut spec_json);

    let spec: openapiv3::OpenAPI =
        serde_json::from_value(spec_json).expect("parse downgraded spec");

    let mut generator = progenitor::Generator::default();
    let tokens = generator
        .generate_tokens(&spec)
        .expect("progenitor codegen");
    let ast = syn::parse2(tokens).expect("parse generated tokens");
    let content = prettyplease::unparse(&ast);

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(out_dir.join("explorer_generated.rs"), content).expect("write generated file");
}

/// Collapses 3.1-style nullable unions written as `oneOf: [{type: null}, X]`
/// into a single schema `X + nullable: true`. Recursive. progenitor's
/// `openapiv3 → typify` path does not handle `{type: null}` as a oneOf
/// member.
fn collapse_nullable_oneof(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for v in map.values_mut() {
                collapse_nullable_oneof(v);
            }
            let collapse_other = match map.get("oneOf") {
                Some(Value::Array(arr)) if arr.len() == 2 => {
                    let null_idx = arr
                        .iter()
                        .position(|v| v.get("type").and_then(|t| t.as_str()) == Some("null"));
                    let other_idx = arr.iter().position(|v| {
                        v.get("type").and_then(|t| t.as_str()) != Some("null")
                            && !v.as_object().is_none_or(|o| o.is_empty())
                    });
                    match (null_idx, other_idx) {
                        (Some(_), Some(o)) => Some(arr[o].clone()),
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(other) = collapse_other {
                map.remove("oneOf");
                if let Some(other_obj) = other.as_object() {
                    for (k, v) in other_obj {
                        map.insert(k.clone(), v.clone());
                    }
                }
                map.insert("nullable".into(), Value::Bool(true));
            }
        }
        Value::Array(arr) => {
            for v in arr {
                collapse_nullable_oneof(v);
            }
        }
        _ => {}
    }
}

fn rewrite_operation_ids(spec: &mut Value) {
    let Some(paths) = spec.get_mut("paths").and_then(|v| v.as_object_mut()) else {
        return;
    };
    for (path, item) in paths.iter_mut() {
        let Some(item_obj) = item.as_object_mut() else {
            continue;
        };
        for (method, op) in item_obj.iter_mut() {
            if !matches!(method.as_str(), "get" | "post" | "put" | "delete" | "patch") {
                continue;
            }
            let Some(op_obj) = op.as_object_mut() else {
                continue;
            };
            let new_id = synthesize_op_id(method, path);
            op_obj.insert("operationId".into(), Value::String(new_id));
        }
    }
}

fn synthesize_op_id(method: &str, path: &str) -> String {
    let cleaned: String = path
        .trim_matches('/')
        .replace(['/', '-'], "_")
        .replace(['{', '}'], "");
    if cleaned.is_empty() {
        method.to_string()
    } else {
        format!("{method}_{cleaned}")
    }
}

/// Walks the JSON spec and rewrites every occurrence of
/// `"type": [..., "null"]` (OpenAPI 3.1 nullable syntax) into the 3.0
/// equivalent `"type": "X", "nullable": true`. Recursive — handles
/// nested schemas under `properties`, `items`, `additionalProperties`,
/// `oneOf`, `anyOf`, etc.
fn downgrade_type_arrays(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let rewrite = match map.get("type") {
                Some(Value::Array(arr)) if arr.len() == 2 => {
                    let has_null = arr
                        .iter()
                        .any(|v| matches!(v, Value::String(s) if s == "null"));
                    let other = arr.iter().find_map(|v| match v {
                        Value::String(s) if s != "null" => Some(s.clone()),
                        _ => None,
                    });
                    if has_null {
                        other
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(t) = rewrite {
                map.insert("type".into(), Value::String(t));
                map.insert("nullable".into(), Value::Bool(true));
            }
            for v in map.values_mut() {
                downgrade_type_arrays(v);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                downgrade_type_arrays(v);
            }
        }
        _ => {}
    }
}
