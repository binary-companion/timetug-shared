//! Port of `ScenarioTests.swift` and `ScenarioRunner.swift`: the conformance scenarios (`Conformance/Scenarios/*.json`)
//! through the four C functions, with a scripted host answering the engine's service requests. The C# runner implements
//! the same format and semantics. The functions are called through their Rust symbols (`rust_host`), on the library
//! this crate builds.
//!
//! Format:
//! - `name`; `config` (the `tt_engine_create` config without `dataDirectory`, which the runner supplies: a fresh
//!   temporary directory per scenario, kept across `restart`); `files` (written into the data directory before the first
//!   create: a JSON value is written as JSON, a string as text).
//! - `host.http`: rules tried in order; the first whose `match` is a prefix of the request URL (and whose optional
//!   `method` matches) answers with `status` (default 200), `headers` and a body from `body` (any JSON, sent as JSON
//!   text), `bodyText` or `bodyFile` (under `Scenarios/data/`). `"noReply": true` never answers. A request no rule
//!   matches gets a 404.
//! - `host.credentials.seed`: connection id to secrets, the starting contents of the host's credential store.
//! - `host.auth`: `redirectURI` (default `http://127.0.0.1:53682/`), `code` (default `test-code`), and
//!   `authorize: "never"` to leave the browser step unanswered. The authorize answer echoes the request's `state`.
//! - `host.prompt`: the values a credential form answers with.
//! - `host.tokens` (host-supplied access tokens, `service.token.*`): `signIn` and `get` are each one scripted answer
//!   that repeats, or `"never"` (the request is left unanswered). An answer is `{"status": ..., ...}` as the wire reply's
//!   result (`expiresInSeconds` is turned into `expiresAt` relative to the scenario's manual clock, which the host
//!   follows through `test.advanceClock`), or `{"error": {"kind": ...}}` as a wire error reply. An unscripted request is
//!   answered `{"status":"unavailable"}`. `service.token.cancelSignIn` is always answered with `{}`.
//! - `steps`, run in order:
//!   - `{"send": {...}}` a command envelope (`v` defaults to 1); `{"sendRaw": "..."}` any text;
//!   - `{"expect": {"events": [...]}}` each listed object must subset-match, in order, a subsequence of the messages
//!     received since the previous `events` expectation (all messages count: events, requests, `ready`, `state`);
//!   - `{"expect": {"state": {...}}}` the latest `state` payload must subset-match;
//!   - `{"expect": {"noMoreMessagesFor": seconds}}` nothing at all arrives for that long;
//!   - `{"expect": {"absent": [...]}}` no message received so far matches any of the listed patterns;
//!   - `{"expect": {"diagnosticsExclude": ["text", ...]}}` none of the texts appears in an `event.diagnostic` message
//!     received since the previous `diagnosticsExclude` expectation (a secret must never reach a diagnostic);
//!   - `{"setTokens": {...}}` replaces the `signIn` and/or `get` answers of `host.tokens`;
//!   - `{"restart": true}` destroy, then create again on the same data directory (and the same host);
//!   - `{"destroy": true}`; `{"replyToPending": true}` answers every request the host has left unanswered.
//!
//! Subset match: every member an expected object names must match (others are ignored); arrays must have the same
//! length and match element by element; other values must be equal (numbers compare as numbers). Each wait for an
//! expectation gives up after `timeout` seconds of real time; the engine runs on a manual clock, so a wait is only for
//! delivery.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    non_snake_case
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use TimeTugEngineC::c_interface::rust_host::{self, Handle};
use base64::Engine as _;
use serde_json::{Map, Value, json};
use uuid::Uuid;

/// `TT_BASELINE_SCENARIOS=<dir>` runs an earlier release's scenarios (and their `data/`) instead: an old front end
/// against this backend. The `compat` CI job exports the baseline tag's `Conformance/Scenarios` there.
fn baseline_directory() -> Option<PathBuf> {
    std::env::var("TT_BASELINE_SCENARIOS")
        .ok()
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

/// The shared conformance files: `Conformance/` at the repository root.
fn conformance_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Conformance")
}

fn scenario_directory() -> PathBuf {
    baseline_directory().unwrap_or_else(|| conformance_root().join("Scenarios"))
}

/// Scenario files, sorted by name.
fn scenario_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(scenario_directory())
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
                .collect()
        })
        .unwrap_or_default();
    paths.sort_by_key(|path| path.file_name().map(std::ffi::OsStr::to_os_string));
    paths
}

fn scenario_names() -> Vec<String> {
    scenario_paths()
        .iter()
        .filter_map(|path| path.file_stem()?.to_str().map(str::to_string))
        .collect()
}

fn temporary_directory() -> PathBuf {
    let path = std::env::temp_dir().join(format!("tt-c-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn text(value: &Value) -> String {
    // `serde_json` keeps keys sorted and does not escape slashes: Swift's `.sortedKeys, .withoutEscapingSlashes`.
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn numbers_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

/// Subset match; see the module documentation.
fn matches(expected: &Value, actual: Option<&Value>) -> bool {
    match expected {
        Value::Object(members) => {
            let Some(Value::Object(actual_members)) = actual else {
                return false;
            };
            members.iter().all(|(key, value)| {
                if value.is_null() {
                    actual_members.get(key).is_none_or(Value::is_null)
                } else {
                    matches(value, actual_members.get(key))
                }
            })
        }
        Value::Array(elements) => {
            let Some(Value::Array(values)) = actual else {
                return false;
            };
            values.len() == elements.len()
                && elements
                    .iter()
                    .zip(values)
                    .all(|(element, value)| matches(element, Some(value)))
        }
        _ => actual.is_some_and(|actual| numbers_equal(expected, actual)),
    }
}

/// Index after the last matched message, or `None` when the patterns do not all match in order.
fn subsequence(expected: &[Value], messages: &[Value], start: usize) -> Option<usize> {
    let mut index = start;
    for pattern in expected {
        let found = messages
            .get(index..)?
            .iter()
            .position(|message| matches(pattern, Some(message)))?;
        index += found + 1;
    }
    Some(index)
}

struct RunResult {
    failures: Vec<String>,
}

struct ScenarioRunner {
    scenario: Value,
    timeout: f64,
}

impl ScenarioRunner {
    fn from_path(path: &Path) -> ScenarioRunner {
        ScenarioRunner {
            scenario: serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap(),
            timeout: 5.0,
        }
    }

    fn inline(scenario: Value, timeout: f64) -> ScenarioRunner {
        ScenarioRunner { scenario, timeout }
    }

    fn wait_until(&self, mut condition: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs_f64(self.timeout);
        while !condition() {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        true
    }

    fn write_files(&self, directory: &Path) -> std::io::Result<()> {
        let Some(Value::Object(files)) = self.scenario.get("files") else {
            return Ok(());
        };
        for (file, content) in files {
            let data = match content {
                Value::String(text) => text.clone(),
                other => text(other),
            };
            std::fs::write(directory.join(file), data)?;
        }
        Ok(())
    }

    fn run(&self) -> RunResult {
        let directory = temporary_directory();
        let mut failures: Vec<String> = Vec::new();
        if let Err(error) = self.write_files(&directory) {
            return RunResult {
                failures: vec![format!("cannot write the scenario files: {error}")],
            };
        }
        let mut config: Map<String, Value> = match self.scenario.get("config") {
            Some(Value::Object(given)) => given.clone(),
            _ => Map::new(),
        };
        let clock_start = config
            .get("testClockStart")
            .and_then(Value::as_str)
            .and_then(|start| start.parse::<jiff::Timestamp>().ok())
            .unwrap_or_else(jiff::Timestamp::now);
        let script = self
            .scenario
            .get("host")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let host = ScriptedHost::new(script, clock_start);
        config.insert(
            "dataDirectory".into(),
            json!(directory.to_str().unwrap().to_string()),
        );
        let config_json = text(&Value::Object(config));

        let create = |failures: &mut Vec<String>| -> bool {
            match rust_host::create(Some(&config_json), Some(host.sink())) {
                Some(handle) => {
                    host.attach(handle);
                    true
                }
                None => {
                    failures.push("tt_engine_create returned NULL".to_string());
                    false
                }
            }
        };
        if !create(&mut failures) {
            return RunResult { failures };
        }

        let mut cursor = 0;
        let mut diagnostics_cursor = 0;
        let Some(Value::Array(steps)) = self.scenario.get("steps") else {
            return RunResult {
                failures: vec!["the scenario has no steps".to_string()],
            };
        };
        for (index, step) in steps.iter().enumerate() {
            let label = format!("step {}", index + 1);
            if let Some(command) = step.get("send").filter(|c| c.is_object()) {
                let mut envelope = command.as_object().cloned().unwrap_or_default();
                envelope.entry("v").or_insert(json!(1));
                host.send(&text(&Value::Object(envelope)));
            } else if let Some(raw) = step.get("sendRaw").and_then(Value::as_str) {
                host.send(raw);
            } else if step.get("restart") == Some(&json!(true)) {
                host.destroy();
                if !create(&mut failures) {
                    break;
                }
            } else if let Some(tokens) = step.get("setTokens") {
                host.set_tokens(tokens);
            } else if step.get("destroy") == Some(&json!(true)) {
                host.destroy();
            } else if step.get("replyToPending") == Some(&json!(true)) {
                host.reply_to_pending();
            } else if let Some(expectation) = step.get("expect") {
                if let Some(Value::Array(events)) = expectation.get("events") {
                    let mut next = None;
                    self.wait_until(|| {
                        next = subsequence(events, &host.messages(), cursor);
                        next.is_some()
                    });
                    match next {
                        Some(next) => cursor = next,
                        None => {
                            failures.push(format!(
                                "{label}: expected events {} in order; received {}",
                                text(&Value::Array(events.clone())),
                                host.summary(cursor)
                            ));
                            break;
                        }
                    }
                }
                if let Some(state) = expectation.get("state") {
                    let matched = self.wait_until(|| matches(state, host.latest_state().as_ref()));
                    if !matched {
                        failures.push(format!(
                            "{label}: expected state {}; latest state {}; received since the last events expectation: {}",
                            text(state),
                            host.latest_state().as_ref().map_or("none".to_string(), text),
                            host.summary(cursor)
                        ));
                        break;
                    }
                }
                if let Some(Value::Array(patterns)) = expectation.get("absent") {
                    let found = host.messages().into_iter().find(|message| {
                        patterns
                            .iter()
                            .any(|pattern| matches(pattern, Some(message)))
                    });
                    if let Some(found) = found {
                        failures.push(format!(
                            "{label}: a message that should be absent was received: {}",
                            text(&found)
                        ));
                        break;
                    }
                }
                if let Some(Value::Array(excluded)) = expectation.get("diagnosticsExclude") {
                    let texts: Vec<&str> = excluded.iter().filter_map(Value::as_str).collect();
                    let messages = host.messages();
                    let leak = messages
                        .iter()
                        .skip(diagnostics_cursor)
                        .filter(|message| message["type"] == "event.diagnostic")
                        .map(text)
                        .find(|line| texts.iter().any(|excluded| line.contains(excluded)));
                    diagnostics_cursor = host.message_count();
                    if let Some(leak) = leak {
                        failures.push(format!(
                            "{label}: a diagnostic contains an excluded text: {leak}"
                        ));
                        break;
                    }
                }
                if let Some(seconds) = expectation.get("noMoreMessagesFor").and_then(Value::as_f64)
                {
                    let before = host.message_count();
                    std::thread::sleep(Duration::from_secs_f64(seconds));
                    if host.message_count() != before {
                        failures.push(format!(
                            "{label}: {} message(s) arrived: {}",
                            host.message_count() - before,
                            host.summary(before)
                        ));
                        break;
                    }
                }
            } else {
                failures.push(format!("{label}: unknown step {}", text(step)));
                break;
            }
        }
        host.destroy();
        let _ = std::fs::remove_dir_all(&directory);
        RunResult { failures }
    }
}

struct HostState {
    handle: Option<Handle>,
    /// The most recent engine, kept after destroy so `replyToPending` can answer a destroyed engine.
    last_handle: Option<Handle>,
    received: Vec<Value>,
    latest: Option<Value>,
    credentials: BTreeMap<String, (BTreeMap<String, String>, String)>,
    unanswered: Vec<u64>,
    redirects: u32,
    /// The scripted `host.tokens` answers (replaced by `setTokens`).
    tokens: Map<String, Value>,
    clock_offset: f64,
}

/// The scripted host: the callback context of a scenario's engine. It records every message and answers service requests
/// from the script, from inside the callback (which the ABI allows).
struct ScriptedHost {
    script: Value,
    clock_start: jiff::Timestamp,
    state: Mutex<HostState>,
}

fn string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

fn result_reply(value: Value) -> Value {
    json!({"v": 1, "result": value})
}

fn error_reply(kind: &str, message: &str) -> Value {
    json!({"v": 1, "error": {"kind": kind, "message": message}})
}

fn not_found() -> Value {
    result_reply(json!({"status": 404, "headers": {}, "body": ""}))
}

/// Percent-encodes what a query value may not hold.
fn encode_query(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

impl ScriptedHost {
    fn new(script: Value, clock_start: jiff::Timestamp) -> Arc<ScriptedHost> {
        let tokens = script
            .get("tokens")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let mut credentials = BTreeMap::new();
        if let Some(Value::Object(seed)) = script.get("credentials").and_then(|c| c.get("seed")) {
            for (id, secrets) in seed {
                let Value::Object(values) = secrets else {
                    continue;
                };
                let secrets = values
                    .iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                    .collect();
                credentials.insert(
                    id.clone(),
                    (secrets, Uuid::new_v4().to_string().to_uppercase()),
                );
            }
        }
        Arc::new(ScriptedHost {
            script,
            clock_start,
            state: Mutex::new(HostState {
                handle: None,
                last_handle: None,
                received: Vec::new(),
                latest: None,
                credentials,
                unanswered: Vec::new(),
                redirects: 0,
                tokens,
                clock_offset: 0.0,
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, HostState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn sink(self: &Arc<Self>) -> Arc<rust_host::Sink> {
        let host = self.clone();
        Arc::new(move |message: &str| host.receive(message))
    }

    fn attach(&self, handle: Handle) {
        let mut state = self.lock();
        state.handle = Some(handle);
        state.last_handle = Some(handle);
        state.clock_offset = 0.0; // a new engine starts its manual clock again
    }

    fn set_tokens(&self, answers: &Value) {
        let Value::Object(given) = answers else {
            return;
        };
        let mut state = self.lock();
        for (key, value) in given {
            state.tokens.insert(key.clone(), value.clone());
        }
    }

    fn messages(&self) -> Vec<Value> {
        self.lock().received.clone()
    }

    fn message_count(&self) -> usize {
        self.lock().received.len()
    }

    fn latest_state(&self) -> Option<Value> {
        self.lock().latest.clone()
    }

    fn summary(&self, start: usize) -> String {
        let all = self.messages();
        if start >= all.len() {
            return "nothing".to_string();
        }
        all[start..]
            .iter()
            .map(|message| {
                let Some(kind) = message["type"].as_str() else {
                    return "?".to_string();
                };
                if kind == "event.diagnostic"
                    && let Some(text) = message["payload"]["message"].as_str()
                {
                    return format!("{kind}({text})");
                }
                if kind == "service.http.send"
                    && let Some(url) = message["payload"]["url"].as_str()
                {
                    return format!("{kind}({url})");
                }
                kind.to_string()
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn send(&self, json_text: &str) {
        let Some(handle) = self.lock().handle else {
            return;
        };
        if let Ok(command) = serde_json::from_str::<Value>(json_text)
            && command["type"] == "test.advanceClock"
            && let Some(seconds) = command["payload"]["seconds"].as_f64()
        {
            self.lock().clock_offset += seconds;
        }
        rust_host::send(handle, Some(json_text));
    }

    fn destroy(&self) {
        let handle = self.lock().handle.take();
        if let Some(handle) = handle {
            rust_host::destroy(handle);
        }
    }

    /// Answers every request left unanswered (after a destroy the engine must ignore these).
    fn reply_to_pending(&self) {
        let (handle, ids) = {
            let mut state = self.lock();
            (state.last_handle, std::mem::take(&mut state.unanswered))
        };
        // Replies go to the engine the requests came from, even when it has been destroyed meanwhile.
        let Some(handle) = handle else { return };
        for id in ids {
            rust_host::reply(handle, id, Some(r#"{"v":1,"result":{}}"#));
        }
    }

    fn receive(&self, message_text: &str) {
        let Ok(message) = serde_json::from_str::<Value>(message_text) else {
            return;
        };
        let handle = {
            let mut state = self.lock();
            state.received.push(message.clone());
            if message["type"] == "state" {
                state.latest = Some(message["payload"].clone());
            }
            state.handle
        };
        let Some(kind) = message["type"]
            .as_str()
            .filter(|k| k.starts_with("service."))
        else {
            return;
        };
        let (Some(id), Some(handle)) = (message["id"].as_u64(), handle) else {
            return;
        };
        match self.answer(kind, message.get("payload").unwrap_or(&Value::Null)) {
            Some(reply) => rust_host::reply(handle, id, Some(&text(&reply))),
            None => self.lock().unanswered.push(id),
        }
    }

    /// The scripted answer, or `None` to leave the request unanswered.
    fn answer(&self, kind: &str, payload: &Value) -> Option<Value> {
        let connection = string(payload.get("connectionID")).unwrap_or_default();
        match kind {
            "service.http.send" => self.http(payload),
            "service.credentials.get" => {
                let secrets = self
                    .lock()
                    .credentials
                    .get(&connection)
                    .map(|e| e.0.clone());
                Some(result_reply(match secrets {
                    Some(secrets) => json!({"secrets": secrets}),
                    None => json!({}),
                }))
            }
            "service.credentials.set" => {
                let secrets: BTreeMap<String, String> = payload["secrets"]
                    .as_object()
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                            .collect()
                    })
                    .unwrap_or_default();
                self.lock().credentials.insert(
                    connection,
                    (secrets, Uuid::new_v4().to_string().to_uppercase()),
                );
                Some(result_reply(json!({})))
            }
            "service.credentials.remove" => {
                self.lock().credentials.remove(&connection);
                Some(result_reply(json!({})))
            }
            "service.credentials.snapshot" => {
                let entry = self.lock().credentials.get(&connection).cloned();
                Some(result_reply(match entry {
                    Some((secrets, revision)) => {
                        json!({"snapshot": {"secrets": secrets, "revision": revision}})
                    }
                    None => json!({}),
                }))
            }
            "service.credentials.updateRefreshToken" => {
                let expected = string(payload.get("expectedRevision"));
                let token = string(payload.get("token"));
                let updated = {
                    let mut state = self.lock();
                    match state.credentials.get_mut(&connection) {
                        Some(entry) if Some(&entry.1) == expected.as_ref() => {
                            if let Some(token) = token {
                                entry.0.insert("refresh_token".to_string(), token);
                            }
                            true
                        }
                        _ => false,
                    }
                };
                Some(result_reply(json!({"updated": updated})))
            }
            "service.auth.beginRedirect" => {
                let number = {
                    let mut state = self.lock();
                    state.redirects += 1;
                    state.redirects
                };
                Some(result_reply(json!({
                    "sessionID": format!("session-{number}"),
                    "redirectURI": string(self.script.get("auth").and_then(|a| a.get("redirectURI")))
                        .unwrap_or_else(|| "http://127.0.0.1:53682/".to_string()),
                })))
            }
            "service.auth.authorize" => {
                if string(self.script.get("auth").and_then(|a| a.get("authorize"))).as_deref()
                    == Some("never")
                {
                    return None;
                }
                let url = string(payload.get("url")).unwrap_or_default();
                let link = calendar_core::url_string::UrlString::parse(&url);
                let state = link.as_ref().and_then(|l| l.query_value("state"));
                let redirect = link.as_ref().and_then(|l| l.query_value("redirect_uri"));
                let (Some(state), Some(redirect)) = (state, redirect) else {
                    return Some(error_reply(
                        "failed",
                        "no state or redirect in the authorization URL",
                    ));
                };
                let base = redirect.split(['?', '#']).next().unwrap_or_default();
                let code = string(self.script.get("auth").and_then(|a| a.get("code")))
                    .unwrap_or_else(|| "test-code".to_string());
                Some(result_reply(json!({
                    "url": format!("{base}?code={}&state={}", encode_query(&code), encode_query(&state)),
                })))
            }
            "service.auth.closeRedirect" | "service.signIn.prepare" => {
                Some(result_reply(json!({})))
            }
            "service.auth.promptCredentials" => match self.script.get("prompt") {
                Some(Value::Object(values)) => Some(result_reply(json!({"values": values}))),
                _ => Some(error_reply("cancelled", "no form answer")),
            },
            "service.signIn.lastNonSecretValues" => Some(result_reply(json!({}))),
            "service.text.availability" => Some(result_reply(
                json!({"kind": "unavailable", "reason": "scripted host"}),
            )),
            "service.token.get" => {
                let scripted = self.lock().tokens.get("get").cloned();
                self.token(scripted)
            }
            "service.token.signIn" => {
                let scripted = self.lock().tokens.get("signIn").cloned();
                self.token(scripted)
            }
            "service.token.cancelSignIn" => Some(result_reply(json!({}))),
            other => Some(error_reply(
                "notFound",
                &format!("the scripted host has no {other}"),
            )),
        }
    }

    /// A scripted token answer as a wire reply; `None` leaves the request unanswered (`"never"`).
    fn token(&self, scripted: Option<Value>) -> Option<Value> {
        let unavailable = || Some(result_reply(json!({"status": "unavailable"})));
        let scripted = scripted?;
        if scripted == json!("never") {
            return None;
        }
        let Value::Object(mut answer) = scripted else {
            return unavailable();
        };
        if let Some(failure) = answer.get("error") {
            return Some(json!({"v": 1, "error": failure}));
        }
        if let Some(seconds) = answer.remove("expiresInSeconds").and_then(|v| v.as_f64()) {
            let offset = self.lock().clock_offset;
            let now = calendar_core::time::add_seconds(self.clock_start, offset + seconds);
            answer.insert(
                "expiresAt".into(),
                json!(timetug_engine_protocol::wire_types::WireDates::string(now)),
            );
        }
        Some(result_reply(Value::Object(answer)))
    }

    fn http(&self, payload: &Value) -> Option<Value> {
        let url = string(payload.get("url")).unwrap_or_default();
        let method = string(payload.get("method")).unwrap_or_else(|| "GET".to_string());
        let Some(Value::Array(rules)) = self.script.get("http") else {
            return Some(not_found());
        };
        for rule in rules {
            let Some(prefix) = string(rule.get("match")) else {
                continue;
            };
            if !url.starts_with(&prefix) {
                continue;
            }
            if let Some(wanted) = string(rule.get("method"))
                && wanted != method
            {
                continue;
            }
            if rule.get("noReply") == Some(&json!(true)) {
                return None;
            }
            let body: Vec<u8> = if let Some(body) = string(rule.get("bodyText")) {
                body.into_bytes()
            } else if let Some(file) = string(rule.get("bodyFile")) {
                std::fs::read(scenario_directory().join("data").join(file)).unwrap_or_default()
            } else if let Some(value) = rule.get("body") {
                text(value).into_bytes()
            } else {
                Vec::new()
            };
            let mut reply = Map::new();
            reply.insert(
                "status".into(),
                rule.get("status").cloned().unwrap_or(json!(200)),
            );
            reply.insert(
                "body".into(),
                json!(base64::engine::general_purpose::STANDARD.encode(body)),
            );
            reply.insert(
                "headers".into(),
                rule.get("headers")
                    .cloned()
                    .unwrap_or_else(|| json!({"content-type": "application/json"})),
            );
            return Some(result_reply(Value::Object(reply)));
        }
        Some(not_found())
    }
}

// MARK: - Tests

#[test]
fn scenario_passes() {
    let names = scenario_names();
    assert!(
        !names.is_empty(),
        "no scenario files in {:?}",
        scenario_directory()
    );
    let mut failed = Vec::new();
    for (name, path) in names.iter().zip(scenario_paths()) {
        let result = ScenarioRunner::from_path(&path).run();
        if !result.failures.is_empty() {
            failed.push(format!("{name}: {}", result.failures.join("\n")));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n\n"));
}

/// The exact scenario set of the spec's Testing section: deleting or renaming a scenario fails the build. Not in
/// baseline mode, where the set is the baseline's.
#[test]
fn scenario_inventory() {
    if baseline_directory().is_some() {
        return;
    }
    let mut names = scenario_names();
    names.sort();
    assert_eq!(
        names,
        vec![
            "add-account-brokered",
            "add-account-oauth",
            "brokered-cancel",
            "brokered-needs-reconnect",
            "destroy-while-signin-pending",
            "http-timeout",
            "invalid-command",
            "late-takeover-after-wake",
            "settings-persist-across-restart",
            "snooze-refire",
            "start-ready",
            "takeover-after-clock",
            "time-zone-change",
        ]
    );
}

#[test]
fn runner_reports_a_mismatch() {
    let bad = json!({
        "name": "bad",
        "config": {"v": 1, "platform": "linux", "testing": true},
        "steps": [
            {"send": {"type": "start"}},
            {"expect": {"events": [{"type": "nope"}]}},
        ],
    });
    assert_eq!(ScenarioRunner::inline(bad, 0.5).run().failures.len(), 1);
}

#[test]
fn runner_reports_a_state_mismatch_and_stray_messages() {
    let bad = json!({
        "config": {"v": 1, "platform": "linux", "testing": true},
        "steps": [
            {"send": {"type": "start"}},
            {"expect": {"state": {"takeoverVisible": true}}},
        ],
    });
    assert_eq!(ScenarioRunner::inline(bad, 0.5).run().failures.len(), 1);
    let stray = json!({
        "config": {"v": 1, "platform": "linux", "testing": true},
        "steps": [
            {"send": {"type": "start"}},
            {"expect": {"noMoreMessagesFor": 0.3}},
        ],
    });
    assert_eq!(ScenarioRunner::inline(stray, 0.5).run().failures.len(), 1);
}

#[test]
fn subset_matching() {
    let actual = json!({"type": "state", "payload": {"a": 1, "list": [1, 2]}});
    assert!(matches(&json!({"type": "state"}), Some(&actual)));
    assert!(matches(&json!({"payload": {"a": 1}}), Some(&actual)));
    assert!(!matches(&json!({"payload": {"a": 2}}), Some(&actual)));
    assert!(!matches(&json!({"payload": {"list": [1]}}), Some(&actual)));
    assert!(matches(&json!({"missing": null}), Some(&actual)));
    assert!(!matches(&json!({"missing": 1}), Some(&actual)));
    assert!(
        matches(&json!({"payload": {"a": 1.0}}), Some(&actual)),
        "numbers compare as numbers"
    );
    assert!(
        !matches(&json!({"payload": {"a": true}}), Some(&actual)),
        "a boolean is not a number"
    );
    let messages = vec![
        json!({"type": "a"}),
        json!({"type": "b"}),
        json!({"type": "a"}),
    ];
    assert_eq!(
        subsequence(&[json!({"type": "b"}), json!({"type": "a"})], &messages, 0),
        Some(3)
    );
    assert_eq!(
        subsequence(&[json!({"type": "b"}), json!({"type": "b"})], &messages, 0),
        None
    );
}
