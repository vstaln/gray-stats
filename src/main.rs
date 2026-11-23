//! gray-telemetry — one sidecar for every local event ledger.
//!
//! Merges gray-observe, gray-stamp and gray-usage. Each source lives
//! verbatim in `mods/` with its own state files and slash commands — only
//! the wire plumbing is shared.
//!
//! Fan-out:
//! - `event/notify`   → observe (JSONL log) + stamp (timing ledger) + usage
//!   (live turn_end counters); notifications, never answered.
//! - `prompt/context` → stamp (last-turn timing annotation).
//! - `command/run`    → routed by `params.name` to the owning module.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{Value, json};

mod parts;

#[allow(dead_code)]
#[path = "mods/observe.rs"]
mod observe;
#[allow(dead_code)]
#[path = "mods/stamp.rs"]
mod stamp;
#[allow(dead_code)]
#[path = "mods/usage.rs"]
mod usage;

const PARTS: &[parts::Part] = &[
    parts::Part {
        name: "observe",
        default: true,
        description: "JSONL log of every turn and tool event",
    },
    parts::Part {
        name: "stamp",
        default: true,
        description: "per-session timing ledger and last-turn annotation",
    },
    parts::Part {
        name: "usage",
        default: true,
        description: "token and cost counters for the current turn",
    },
];

fn manifest(parts: &parts::Parts) -> Value {
    let mut commands: Vec<&str> = vec!["/telemetry"];
    if parts.on("observe") {
        commands.push("/observe");
    }
    if parts.on("stamp") {
        commands.push("/stamps");
    }
    if parts.on("usage") {
        commands.push("/usage");
        commands.push("/tokens");
    }
    let mut completion: Vec<&str> = Vec::new();
    if parts.on("usage") {
        completion.extend(["all", "reset"]);
    }
    let mut hooks: Vec<&str> = Vec::new();
    if parts.on("stamp") {
        hooks.push("prompt/context");
    }
    json!({
        "name": "telemetry",
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": "1.1",
        "tools": [],
        "description": "turn timing, tokens and usage figures",
        "commands": commands,
        "completion": completion,
        "hooks": hooks,
        "parts": parts.manifest(PARTS),
    })
}

/// Per-process module state: observe's log dir and stamp's in-flight timers.
struct Telemetry {
    observe_dir: PathBuf,
    stamp: stamp::State,
}

impl Telemetry {
    fn new() -> Self {
        Self {
            observe_dir: observe::observe_dir(),
            stamp: Mutex::new(HashMap::new()),
        }
    }
}

/// `/telemetry` — merged-plugin overview.
fn telemetry_command() -> String {
    format!(
        "gray-telemetry {} — one sidecar, three local event ledgers:\n\
         \x20 /observe [tail N]     JSONL turn/tool events under ~/.gray/observe\n\
         \x20 /stamps [clear|path]  per-session timing ledger under ~/.gray/stamps\n\
         \x20 /usage, /tokens [all|reset]  token + cost report, ~/.gray/usage",
        env!("CARGO_PKG_VERSION")
    )
}

fn handle(req: &Value, t: &Telemetry, parts: &parts::Parts) -> (Option<Value>, bool) {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let Some(id) = id else {
        if method == "event/notify" {
            // All three ledgers observe the same events; none reply.
            if parts.on("observe") {
                let _ = observe::handle_observe(req, &t.observe_dir);
            }
            if parts.on("stamp") {
                let _ = stamp::handle_stamp(req, &t.stamp);
            }
            if parts.on("usage") {
                let _ = usage::handle_usage(req);
            }
        }
        return (None, method == "plugin/shutdown");
    };
    let result = match method {
        "plugin/manifest" => manifest(parts),
        "prompt/context" => {
            if !parts.on("stamp") {
                return (Some(json!({ "id": id, "result": {} })), false);
            }
            let (reply, _) = stamp::handle_stamp(req, &t.stamp);
            return (reply, false);
        }
        "command/run" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let reply = match name {
                "/telemetry" => {
                    return (
                        Some(json!({ "id": id, "result": { "text": telemetry_command() } })),
                        false,
                    );
                }
                "/observe" if parts.on("observe") => observe::handle_observe(req, &t.observe_dir).0,
                "/stamps" if parts.on("stamp") => stamp::handle_stamp(req, &t.stamp).0,
                "/usage" | "/tokens" if parts.on("usage") => usage::handle_usage(req).0,
                _ => None,
            };
            match reply {
                Some(r) => r
                    .get("result")
                    .cloned()
                    .unwrap_or_else(|| json!({ "text": format!("{name}: error") })),
                None => json!({ "text": format!("unknown command {name} — try /telemetry") }),
            }
        }
        "plugin/shutdown" => return (Some(json!({ "id": id, "result": {} })), true),
        _ => {
            let error = json!({ "code": -32601, "message": "method not found" });
            return (Some(json!({ "id": id, "error": error })), false);
        }
    };
    (Some(json!({ "id": id, "result": result })), false)
}

fn main() -> std::io::Result<()> {
    let parts = parts::Parts::load("telemetry", PARTS);
    if std::env::args().nth(1).as_deref() == Some("manifest") {
        println!("{}", manifest(&parts));
        return Ok(());
    }
    let t = Telemetry::new();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        let Ok(req) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let (reply, exit) = handle(&req, &t, &parts);
        if let Some(reply) = reply {
            writeln!(stdout, "{reply}")?;
            stdout.flush()?;
        }
        if exit {
            break;
        }
    }
    Ok(())
}
