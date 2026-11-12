//! Port of `LifecycleTests.swift`: the ABI rules of the spec, through the four C functions (`rust_host` calls the exported
//! functions themselves): creation, `ready`, rejected commands, destroy from outside and inside a callback, stale handles,
//! state coalescing under a slow host and engine isolation. Plus Review Focus 3: garbage from the host.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use super::engine_support::{
    MessageCollector, create_test_engine, default_config, poll_for, poll_sync, send, settle,
    temporary_directory, test_config,
};
use super::support::LockedBox;
use crate::c_interface::rust_host::{self, Handle};

fn started_engine(collector: &Arc<MessageCollector>, config: &str) -> Handle {
    let handle = create_test_engine(collector, config).unwrap();
    send(handle, r#"{"v":1,"type":"start"}"#);
    poll_sync("ready and a state", || {
        collector.types().contains(&"state".to_string())
    });
    handle
}

fn count_type(collector: &MessageCollector, message_type: &str) -> usize {
    collector
        .types()
        .iter()
        .filter(|t| *t == message_type)
        .count()
}

fn lead_time(collector: &MessageCollector) -> Value {
    collector.last_state().map_or(Value::Null, |state| {
        state["settings"]["takeover"]["leadTime"].clone()
    })
}

fn is_command_diagnostic(envelope: &Value) -> bool {
    envelope["type"] == "event.diagnostic" && envelope["payload"]["category"] == "command"
}

fn microsoft_client() -> Value {
    json!({"connectors": {"microsoft": {"clientID": "test-client"}}})
}

#[test]
fn create_returns_nil_for_bad_config() {
    let collector = MessageCollector::new();
    let sink = || Some(collector.sink());
    assert!(rust_host::create(Some("{not json"), sink()).is_none());
    assert!(rust_host::create(None, sink()).is_none());
    assert!(
        rust_host::create(
            Some(r#"{"v":2,"dataDirectory":"/tmp/x","platform":"linux"}"#),
            None
        )
        .is_none()
    );
    assert!(
        rust_host::create(
            Some(r#"{"v":1,"dataDirectory":"","platform":"linux"}"#),
            None
        )
        .is_none()
    );
    assert!(
        rust_host::create(Some(&default_config()), None).is_none(),
        "a missing callback is a bad config"
    );
    assert_eq!(collector.count(), 0, "a failed create makes no callback");
}

#[test]
fn ready_then_state_after_start() {
    let collector = MessageCollector::new();
    let handle = create_test_engine(&collector, &default_config()).unwrap();
    assert_eq!(collector.count(), 0, "nothing is delivered before start");
    send(handle, r#"{"v":1,"type":"start"}"#);
    poll_sync("a state", || {
        collector.types().contains(&"state".to_string())
    });
    let envelopes = collector.envelopes();
    assert_eq!(envelopes[0]["type"], "ready");
    assert_eq!(count_type(&collector, "ready"), 1);
    let ready = &envelopes[0]["payload"];
    assert!(!ready["engineVersion"].as_str().unwrap().is_empty());
    assert_eq!(ready["protocolVersions"], json!([1]));
    assert_eq!(ready["platform"], "linux");
    assert_eq!(
        ready["capabilities"],
        json!({"systemCalendar": false, "textGeneration": false})
    );
    // A second start is the engine's to refuse: no second ready.
    send(handle, r#"{"v":1,"type":"start","id":2}"#);
    poll_sync("the refusal", || {
        collector.types().contains(&"event.diagnostic".to_string())
    });
    assert_eq!(count_type(&collector, "ready"), 1);
    rust_host::destroy(handle);
}

#[test]
fn rejected_command_diagnostic_echoes_the_command_id() {
    let collector = MessageCollector::new();
    let handle = started_engine(&collector, &default_config());
    send(handle, r#"{"v":1,"type":"nope","id":"cmd-9"}"#);
    send(
        handle,
        r#"{"v":1,"type":"dedup.unmerge","id":42,"payload":{"eventID":"gone"}}"#,
    );
    send(handle, "{not json");
    send(handle, r#"{"v":2,"type":"refresh","id":"v2"}"#);
    poll_sync("four diagnostics", || {
        collector
            .envelopes()
            .iter()
            .filter(|e| is_command_diagnostic(e))
            .count()
            == 4
    });
    let diagnostics: Vec<Value> = collector
        .envelopes()
        .into_iter()
        .filter(is_command_diagnostic)
        .collect();
    let ids: Vec<Option<&Value>> = diagnostics.iter().map(|d| d.get("id")).collect();
    assert_eq!(
        ids,
        vec![
            Some(&json!("cmd-9")),
            Some(&json!(42)),
            None,
            Some(&json!("v2"))
        ]
    );
    assert_eq!(diagnostics[0]["payload"]["message"], "unknown command type");
    assert_eq!(diagnostics[1]["payload"]["message"], "unknown event id");
    rust_host::destroy(handle);
}

#[test]
fn test_commands_need_the_testing_flag() {
    let collector = MessageCollector::new();
    let config = test_config(None, json!({"testing": false}));
    let handle = started_engine(&collector, &config);
    send(
        handle,
        r#"{"v":1,"type":"test.advanceClock","id":"t","payload":{"seconds":60}}"#,
    );
    poll_sync("the refusal", || {
        collector.envelopes().iter().any(|e| e["id"] == "t")
    });
    let refusal = collector
        .envelopes()
        .into_iter()
        .find(|e| e["id"] == "t")
        .unwrap();
    assert_eq!(refusal["payload"]["message"], "test commands are disabled");
    rust_host::destroy(handle);
}

#[test]
fn send_from_inside_a_callback_is_processed() {
    let collector = MessageCollector::new();
    let handle = create_test_engine(&collector, &default_config()).unwrap();
    let sent = Arc::new(AtomicBool::new(false));
    collector.set_hook(move |text| {
        if !text.contains(r#""type":"ready""#) || sent.swap(true, Ordering::SeqCst) {
            return;
        }
        send(handle, r#"{"v":1,"type":"nope","id":"inside"}"#);
    });
    send(handle, r#"{"v":1,"type":"start"}"#);
    poll_sync("the diagnostic", || {
        collector.envelopes().iter().any(|e| e["id"] == "inside")
    });
    rust_host::destroy(handle);
}

#[test]
fn no_callback_after_destroy_returns() {
    let collector = MessageCollector::new();
    let handle = started_engine(&collector, &default_config());
    collector.set_hook(|_| std::thread::sleep(Duration::from_millis(50)));
    for index in 0..20 {
        send(
            handle,
            &format!(
                r#"{{"v":1,"type":"takeover.test","payload":{{"title":"t{index}","calendarTitle":"c"}}}}"#
            ),
        );
    }
    poll_sync("the slow host is busy", || {
        collector.types().contains(&"event.takeoverDue".to_string())
    });
    rust_host::destroy(handle);
    let count = collector.count();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(collector.count(), count);
}

#[test]
fn destroy_from_inside_callback_does_not_deadlock_or_call_again() {
    let collector = MessageCollector::new();
    let handle = create_test_engine(&collector, &default_config()).unwrap();
    let destroyed_at: Arc<LockedBox<Option<usize>>> = LockedBox::new(None);
    let hook_collector = collector.clone();
    let hook_destroyed = destroyed_at.clone();
    collector.set_hook(move |text| {
        if !text.contains(r#""type":"state""#) || hook_destroyed.value().is_some() {
            return;
        }
        rust_host::destroy(handle);
        let count = hook_collector.count();
        hook_destroyed.mutate(|at| *at = Some(count));
    });
    send(handle, r#"{"v":1,"type":"start"}"#);
    for _ in 0..5 {
        send(handle, r#"{"v":1,"type":"refresh"}"#);
    }
    poll_sync("destroy ran in the callback", || {
        destroyed_at.value().is_some()
    });
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        Some(collector.count()),
        destroyed_at.value(),
        "no callback after the one that destroyed"
    );
    // The handle is dead: a second destroy and a send are ignored.
    rust_host::destroy(handle);
    send(handle, r#"{"v":1,"type":"refresh"}"#);
}

#[test]
fn stale_null_and_repeated_handles_are_ignored() {
    rust_host::send(Handle::NULL, Some("{}"));
    rust_host::reply(Handle::NULL, 1, Some("{}"));
    rust_host::destroy(Handle::NULL);
    let collector = MessageCollector::new();
    let handle = create_test_engine(&collector, &default_config()).unwrap();
    rust_host::destroy(handle);
    rust_host::destroy(handle);
    send(handle, r#"{"v":1,"type":"start"}"#);
    rust_host::reply(handle, 1, Some(r#"{"v":1,"result":{}}"#));
    let made_up = Handle::from_raw(0xdead_beef);
    send(made_up, r#"{"v":1,"type":"start"}"#);
    rust_host::destroy(made_up);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(collector.count(), 0);
}

#[test]
fn reply_for_unknown_request_id_is_ignored() {
    let collector = MessageCollector::new();
    let handle = started_engine(&collector, &default_config());
    // Wait for the engine's own start-up work to finish (a long quiet window: a slow runner can pause for longer than a
    // short one between two of its start-up messages), then send the stray replies and wait for quiet again.
    settle(&collector, 1.0, 10.0);
    let before = collector.count();
    rust_host::reply(handle, 424_242, Some(r#"{"v":1,"result":{}}"#));
    rust_host::reply(handle, 424_242, None);
    rust_host::reply(handle, 0, Some("not json"));
    settle(&collector, 0.5, 10.0);
    assert_eq!(collector.count(), before);
    rust_host::destroy(handle);
}

#[test]
fn destroy_with_a_pending_sign_in_returns_and_ignores_the_late_reply() {
    let collector = MessageCollector::new();
    let handle = started_engine(&collector, &test_config(None, microsoft_client()));
    send(
        handle,
        r#"{"v":1,"type":"account.add","payload":{"kindID":"microsoft"}}"#,
    );
    poll_sync("the sign-in request", || {
        collector
            .types()
            .contains(&"service.auth.beginRedirect".to_string())
    });
    let request = collector
        .envelopes()
        .into_iter()
        .find(|e| e["type"] == "service.auth.beginRedirect")
        .unwrap();
    let id = request["id"].as_u64().expect("no request id");
    rust_host::destroy(handle);
    let count = collector.count();
    rust_host::reply(
        handle,
        id,
        Some(r#"{"v":1,"result":{"sessionID":"s","redirectURI":"http://127.0.0.1:1"}}"#),
    );
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(collector.count(), count);
}

#[test]
fn diagnostics_from_building_the_engine_follow_ready() {
    let directory = temporary_directory();
    std::fs::write(directory.join("settings.json"), b"{not json").unwrap();
    let collector = MessageCollector::new();
    let handle =
        create_test_engine(&collector, &test_config(Some(directory), Value::Null)).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(collector.count(), 0, "held until ready");
    send(handle, r#"{"v":1,"type":"start"}"#);
    poll_sync("a state", || {
        collector.types().contains(&"state".to_string())
    });
    let envelopes = collector.envelopes();
    assert_eq!(envelopes[0]["type"], "ready");
    assert_eq!(envelopes[1]["type"], "event.diagnostic");
    assert_eq!(envelopes[1]["payload"]["category"], "settings");
    assert_eq!(lead_time(&collector), json!(60));
    rust_host::destroy(handle);
}

#[test]
fn account_cancel_ends_a_pending_sign_in() {
    let collector = MessageCollector::new();
    let handle = started_engine(&collector, &test_config(None, microsoft_client()));
    send(
        handle,
        r#"{"v":1,"type":"account.add","payload":{"kindID":"microsoft"}}"#,
    );
    let is_working = || {
        collector
            .last_state()
            .map_or(Value::Null, |s| s["accountStatus"]["isWorking"].clone())
    };
    poll_sync("the sign-in runs", || {
        collector
            .types()
            .contains(&"service.auth.beginRedirect".to_string())
            && is_working() == json!(true)
    });
    // The sign-in lane is busy waiting for the host; the cancel must not queue behind it.
    send(handle, r#"{"v":1,"type":"account.cancel"}"#);
    poll_sync("the sign-in ends", || is_working() == json!(false));
    // A later sign-in starts afresh.
    send(
        handle,
        r#"{"v":1,"type":"account.add","payload":{"kindID":"microsoft"}}"#,
    );
    poll_sync("a second sign-in", || {
        count_type(&collector, "service.auth.beginRedirect") == 2
    });
    rust_host::destroy(handle);
}

#[test]
fn state_coalesces_but_events_keep_order() {
    let collector = MessageCollector::new();
    let handle = started_engine(&collector, &default_config());
    collector.set_hook(|_| std::thread::sleep(Duration::from_millis(30)));
    let total = 30;
    for index in 0..total {
        send(
            handle,
            &format!(
                r#"{{"v":1,"type":"takeover.test","payload":{{"title":"t{index}","calendarTitle":"c"}}}}"#
            ),
        );
    }
    // The last change: the newest state must show it, however many states were skipped.
    send(
        handle,
        r#"{"v":1,"type":"settings.apply","payload":{"leadTime":123}}"#,
    );
    poll_for("every takeover and the last change", 20.0, || {
        count_type(&collector, "event.takeoverDue") == total && lead_time(&collector) == json!(123)
    });
    let envelopes = collector.envelopes();
    let titles: Vec<Value> = envelopes
        .iter()
        .filter(|e| e["type"] == "event.takeoverDue")
        .map(|e| e["payload"]["event"]["title"].clone())
        .collect();
    let expected: Vec<Value> = (0..total).map(|i| json!(format!("t{i}"))).collect();
    assert_eq!(titles, expected, "events are never dropped or reordered");
    let revisions: Vec<u64> = envelopes
        .iter()
        .filter(|e| e["type"] == "state")
        .filter_map(|e| e["payload"]["revision"].as_u64())
        .collect();
    assert!(
        revisions.windows(2).all(|pair| pair[0] < pair[1]),
        "revisions only increase: {revisions:?}"
    );
    let states_after_start = envelopes
        .iter()
        .skip_while(|e| e["type"] != "event.takeoverDue")
        .filter(|e| e["type"] == "state")
        .count();
    assert!(
        states_after_start < total,
        "a slow host sees fewer states than changes ({states_after_start})"
    );
    rust_host::destroy(handle);
}

#[test]
fn two_engines_in_one_process_do_not_share_state() {
    let first = MessageCollector::new();
    let second = MessageCollector::new();
    let first_directory = temporary_directory();
    let second_directory = temporary_directory();
    let a = started_engine(
        &first,
        &test_config(Some(first_directory.clone()), Value::Null),
    );
    let b = started_engine(
        &second,
        &test_config(Some(second_directory.clone()), Value::Null),
    );
    send(
        a,
        r#"{"v":1,"type":"settings.apply","payload":{"leadTime":300}}"#,
    );
    send(
        b,
        r#"{"v":1,"type":"settings.apply","payload":{"leadTime":600}}"#,
    );
    poll_sync("both settings applied", || {
        lead_time(&first) == json!(300) && lead_time(&second) == json!(600)
    });
    rust_host::destroy(a);
    rust_host::destroy(b);
    let first_file = std::fs::read_to_string(first_directory.join("settings.json")).unwrap();
    let second_file = std::fs::read_to_string(second_directory.join("settings.json")).unwrap();
    assert!(first_file.contains("300") && !first_file.contains("600"));
    assert!(second_file.contains("600") && !second_file.contains("300"));
}

// MARK: - External tokens

/// The kinds of the first `state` that lists any an engine with `config` publishes: id to `isConfigured`.
fn account_kinds(config: &str) -> std::collections::BTreeMap<String, bool> {
    let collector = MessageCollector::new();
    let handle = started_engine(&collector, config);
    let kinds = || -> Vec<Value> {
        for envelope in collector.envelopes() {
            if envelope["type"] == "state"
                && let Some(kinds) = envelope["payload"]["availableAccountKinds"].as_array()
                && !kinds.is_empty()
            {
                return kinds.clone();
            }
        }
        Vec::new()
    };
    poll_sync("a state with account kinds", || !kinds().is_empty());
    let found = kinds();
    rust_host::destroy(handle);
    found
        .iter()
        .filter_map(|kind| {
            Some((
                kind["id"].as_str()?.to_string(),
                kind["isConfigured"].as_bool()?,
            ))
        })
        .collect()
}

#[test]
fn kind_registered_only_with_flag_and_microsoft_client_id() {
    let client = json!({"microsoft": {"clientID": "ms-client"}});
    let both = account_kinds(&test_config(
        None,
        json!({"externalTokens": true, "connectors": client}),
    ));
    assert_eq!(both.get("microsoft-brokered"), Some(&true));
    assert_eq!(both.get("microsoft"), Some(&true));

    let flag_only = account_kinds(&test_config(None, json!({"externalTokens": true})));
    assert_eq!(
        flag_only.get("microsoft-brokered"),
        Some(&false),
        "listed, but unconfigured, like microsoft"
    );
    assert_eq!(flag_only.get("microsoft"), Some(&false));

    let client_only = account_kinds(&test_config(None, json!({"connectors": client})));
    assert_eq!(
        client_only.get("microsoft-brokered"),
        None,
        "without the flag the kind is not offered"
    );
    assert_eq!(client_only.get("microsoft"), Some(&true));

    let neither = account_kinds(&default_config());
    assert_eq!(neither.get("microsoft-brokered"), None);
    assert_eq!(neither.get("microsoft"), Some(&false));
}

#[test]
fn ready_advertises_external_tokens_only_when_the_flag_is_set() {
    let flagged = MessageCollector::new();
    let first = started_engine(
        &flagged,
        &test_config(None, json!({"externalTokens": true})),
    );
    assert_eq!(
        flagged.envelopes()[0]["payload"]["capabilities"],
        json!({"systemCalendar": false, "textGeneration": false, "externalTokens": true})
    );
    rust_host::destroy(first);

    let plain = MessageCollector::new();
    let second = started_engine(&plain, &test_config(None, json!({"externalTokens": false})));
    assert_eq!(
        plain.envelopes()[0]["payload"]["capabilities"],
        json!({"systemCalendar": false, "textGeneration": false}),
        "an explicit false is the 1.7.0 ready text"
    );
    rust_host::destroy(second);
}

// MARK: - Review Focus 3: malformed input from the host

/// The linear congruential generator of the tests (never the system's random source), so a failure repeats.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn bytes(&mut self, length: usize) -> Vec<u8> {
        (0..length).map(|_| (self.next() & 0xff) as u8).collect()
    }
}

#[test]
fn two_thousand_random_byte_strings_to_send_and_reply_do_not_crash_the_engine() {
    let collector = MessageCollector::new();
    let handle = started_engine(&collector, &test_config(None, microsoft_client()));
    // A pending request, so replies reach a real id.
    send(
        handle,
        r#"{"v":1,"type":"account.add","payload":{"kindID":"microsoft"}}"#,
    );
    poll_sync("a pending request", || {
        collector
            .types()
            .contains(&"service.auth.beginRedirect".to_string())
    });
    let mut random = Lcg(0x5eed_1234);
    let seeds: [&[u8]; 4] = [
        br#"{"v":1,"type":"refresh","id":1}"#,
        br#"{"v":1,"type":"settings.apply","payload":{"leadTime":90}}"#,
        br#"{"v":1,"result":{"sessionID":"s","redirectURI":"http://127.0.0.1:1"}}"#,
        br#"{"v":1,"error":{"kind":"denied","message":"no"}}"#,
    ];
    for round in 0..2_000u64 {
        let length = (random.next() % 300) as usize;
        let mut bytes = random.bytes(length);
        match round % 4 {
            // Pure noise.
            0 => {}
            // A valid message with random bytes overwritten.
            1 => {
                bytes = seeds[(round / 4 % 4) as usize].to_vec();
                for _ in 0..=(random.next() % 4) {
                    let at = (random.next() as usize) % bytes.len();
                    bytes[at] = (random.next() & 0xff) as u8;
                }
            }
            // A valid message cut short.
            2 => {
                bytes = seeds[(round / 4 % 4) as usize].to_vec();
                bytes.truncate((random.next() as usize) % bytes.len());
            }
            // Noise behind a valid prefix.
            _ => {
                let mut prefix = b"{\"v\":1,\"type\":".to_vec();
                prefix.extend(bytes);
                bytes = prefix;
            }
        }
        rust_host::send_bytes(handle, &bytes);
        rust_host::reply_bytes(handle, random.next() % 8, &bytes);
    }
    // Invalid UTF-8 inside otherwise valid commands and replies.
    rust_host::send_bytes(
        handle,
        b"{\"v\":1,\"type\":\"\xff\xfe\",\"id\":\"bad-utf8\"}",
    );
    rust_host::send_bytes(handle, b"\xc3\x28");
    rust_host::send_bytes(handle, b"\xe2\x82");
    rust_host::reply_bytes(handle, 1, b"{\"v\":1,\"result\":\"\xff\"}");
    rust_host::reply_bytes(handle, 1, b"\xf0\x28\x8c\x28");
    rust_host::send(handle, None);
    rust_host::reply(handle, 1, None);
    poll_sync("the diagnostic for the invalid UTF-8 type", || {
        collector.envelopes().iter().any(|e| e["id"] == "bad-utf8")
    });
    // The engine still works after all of it: a fresh command is answered.
    send(handle, r#"{"v":1,"type":"nope","id":"still-alive"}"#);
    poll_sync("the engine still answers", || {
        collector
            .envelopes()
            .iter()
            .any(|e| e["id"] == "still-alive")
    });
    rust_host::destroy(handle);
}
