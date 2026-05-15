//! Structural architecture tests.
//!
//! Encodes the invariant from `docs/design-docs/core-beliefs.md`. The
//! domains under `src/domains/` are stratified:
//!
//!   - **Strict leaves** (`explorer`, `subscriptions`) — own a boundary
//!     (API client or SQLite repo for their own data) and must NOT import
//!     from any other domain. Enforced here.
//!   - **Persistence** (`state`) — accepts typed events from API domains
//!     (`EventRow` from `explorer::types`) so it can persist them. Allowed
//!     to import from `explorer` for type signatures only.
//!   - **Formatters** (`notify`) — consume types from leaves to produce
//!     output. Not checked here.
//!   - **Composers** (`scheduler`, `commands`) — orchestrate everything.
//!     Not checked here.
//!
//! Cross-domain orchestration that doesn't fit any single domain lives in
//! `src/seed.rs` (and any future siblings under `src/`), NOT inside a
//! domain directory.

use std::fs;
use std::path::{Path, PathBuf};

const ALL_DOMAINS: &[&str] = &[
    "commands",
    "explorer",
    "notify",
    "scheduler",
    "state",
    "subscriptions",
];

const STRICT_LEAF_DOMAINS: &[&str] = &["explorer", "subscriptions"];

#[test]
fn strict_leaf_domains_do_not_cross_import() {
    let domains_root = Path::new("src/domains");
    let mut violations = Vec::new();

    for leaf in STRICT_LEAF_DOMAINS {
        let dir = domains_root.join(leaf);
        for file in collect_rs_files(&dir) {
            let body = fs::read_to_string(&file).expect("read source");
            // Strip line and block comments cheaply so doc-comments mentioning
            // `crate::domains::X` don't trip the test.
            let stripped = strip_comments(&body);
            for other in ALL_DOMAINS {
                if other == leaf {
                    continue;
                }
                let needle = format!("crate::domains::{other}");
                if stripped.contains(&needle) {
                    violations.push(format!(
                        "{} imports from `crate::domains::{}` (strict leaves must not cross-import)",
                        file.display(),
                        other,
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Architecture violation(s):\n  - {}",
        violations.join("\n  - ")
    );
}

/// `state` is the persistence layer. It's allowed to import event-shaped
/// types from `explorer::types` (the data flow is API → DB), but nothing
/// else — no other domain, no submodule deeper than `types`.
#[test]
fn state_only_imports_explorer_types() {
    let dir = Path::new("src/domains/state");
    let mut violations = Vec::new();

    for file in collect_rs_files(dir) {
        let body = fs::read_to_string(&file).expect("read source");
        let stripped = strip_comments(&body);
        for other in ALL_DOMAINS {
            if other == &"state" {
                continue;
            }
            let needle = format!("crate::domains::{other}");
            if !stripped.contains(&needle) {
                continue;
            }
            // Allow `crate::domains::explorer::types` exactly; reject anything
            // else under explorer or any other domain at all.
            let only_allowed = other == &"explorer"
                && !stripped.contains("crate::domains::explorer::client")
                && !stripped.contains("crate::domains::explorer::generated");
            if !only_allowed {
                violations.push(format!(
                    "{} imports from `crate::domains::{}` (state may only import `explorer::types`)",
                    file.display(),
                    other,
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Architecture violation(s):\n  - {}",
        violations.join("\n  - ")
    );
}

fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            out.extend(collect_rs_files(&p));
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
    out
}

/// Best-effort comment stripper. Removes everything from `//` to end of
/// line and everything inside `/* … */` (non-nested). String literals are
/// not preserved exactly, but the patterns we scan for
/// (`crate::domains::X`) are vanishingly unlikely to appear inside a real
/// string literal, so this is fine.
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}
