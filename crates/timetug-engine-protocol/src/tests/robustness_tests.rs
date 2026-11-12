//! Review Focus 3: malformed input from the host never panics. The envelope, config and reply parsers and the command
//! decoder see 2,000 pseudo-random byte strings (random bytes, mutated fixtures, truncated fixtures, deep nesting);
//! the test passes when nothing panics, and checks that the parsers also accept what they should.

use std::path::PathBuf;

use timetug_engine::state::EngineState;

use crate::command_mapping::{CommandDecoder, CommandOutcome};
use crate::config::WireConfig;
use crate::envelope::Envelope;
use crate::service_messages::WireReply;
use crate::wire_types::WireDates;

/// The linear congruential generator of the vector exporter (`state = state * 6364136223846793005 + 1442695040888963407`,
/// returning `state >> 33`), so the sequence is the same on every run.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn golden_command_bytes() -> Vec<Vec<u8>> {
    let dir: PathBuf = super::support::conformance::current_golden_directory();
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    names.sort();
    names.iter().map(|p| std::fs::read(p).unwrap()).collect()
}

fn feed(bytes: &[u8]) {
    // The envelope parser, on bytes and on the lossy text the C interface would build from a C string.
    let from_bytes = Envelope::from_slice(bytes);
    let text = String::from_utf8_lossy(bytes);
    let from_text = Envelope::parse(&text);
    // Valid UTF-8 gives the same answer both ways.
    if std::str::from_utf8(bytes).is_ok() {
        assert_eq!(from_bytes.is_ok(), from_text.is_ok());
    }
    // The decoder takes whatever parsed, with and without test commands.
    for envelope in [from_bytes, from_text].into_iter().flatten() {
        for testing in [false, true] {
            let _ = CommandDecoder::new(testing).decode(&envelope, &EngineState::empty());
        }
        let _ = envelope.to_json();
    }
    // The config and the reply parsers.
    let _ = WireConfig::parse(&text);
    let _ = WireReply::parse(&text);
    let _ = WireDates::date(&text);
}

#[test]
fn two_thousand_pseudo_random_byte_strings_never_panic() {
    let fixtures = golden_command_bytes();
    assert!(fixtures.len() > 50);
    let mut rng = Lcg(0x5245_5355_4C54_0001);
    let mut parsed_envelopes = 0;
    for round in 0..2000 {
        let bytes: Vec<u8> = match round % 5 {
            // Pure noise, up to 200 bytes.
            0 => (0..rng.below(200)).map(|_| rng.next() as u8).collect(),
            // JSON-looking noise: only the characters JSON is made of.
            1 => {
                const ALPHABET: &[u8] = b"{}[]\":,0123456789.-+eEtruefalsnv \\/xyz";
                (0..rng.below(120))
                    .map(|_| ALPHABET[rng.below(ALPHABET.len() as u64) as usize])
                    .collect()
            }
            // A fixture with a few bytes replaced.
            2 => {
                let mut b = fixtures[rng.below(fixtures.len() as u64) as usize].clone();
                for _ in 0..=rng.below(4) {
                    let at = rng.below(b.len() as u64) as usize;
                    b[at] = rng.next() as u8;
                }
                b
            }
            // A fixture cut short.
            3 => {
                let b = &fixtures[rng.below(fixtures.len() as u64) as usize];
                b[..rng.below(b.len() as u64) as usize].to_vec()
            }
            // A fixture with a run of bytes deleted or invalid UTF-8 inserted.
            _ => {
                let mut b = fixtures[rng.below(fixtures.len() as u64) as usize].clone();
                let at = rng.below(b.len() as u64) as usize;
                if rng.below(2) == 0 {
                    b.drain(at..(at + rng.below(10) as usize).min(b.len()));
                } else {
                    b.splice(at..at, [0xC3, 0x28, 0xFF, 0xFE]);
                }
                b
            }
        };
        if Envelope::from_slice(&bytes).is_ok() {
            parsed_envelopes += 1;
        }
        feed(&bytes);
    }
    // The unmutated fixtures keep the test honest: it is not only feeding garbage.
    for fixture in &fixtures {
        feed(fixture);
    }
    assert!(
        parsed_envelopes > 0,
        "no mutated input parsed: the corpus does not reach the decoder"
    );
}

#[test]
fn hostile_shapes_are_errors_not_panics() {
    let deep = format!("{}1{}", "[".repeat(100_000), "]".repeat(100_000));
    let deep_object = format!(
        r#"{{"v":1,"type":"x","payload":{}1{}}}"#,
        r#"{"a":"#.repeat(100_000),
        "}".repeat(100_000)
    );
    for text in [
        "",
        "null",
        "[]",
        "7",
        "\"x\"",
        "{",
        r#"{"v":1}"#,
        r#"{"type":"start"}"#,
        r#"{"v":"1","type":"start"}"#,
        r#"{"v":1,"type":7}"#,
        r#"{"v":1,"type":"start","id":-1}"#,
        r#"{"v":1,"type":"start","id":1.5}"#,
        r#"{"v":1,"type":"start","id":true}"#,
        r#"{"v":1,"type":"start","id":{}}"#,
        r#"{"v":1e999,"type":"start"}"#,
        r#"{"v":99999999999999999999,"type":"start"}"#,
        r#"{"v":1,"type":"start","id":18446744073709551616}"#,
        deep.as_str(),
        deep_object.as_str(),
    ] {
        assert!(
            Envelope::parse(text).is_err(),
            "{:?}",
            &text[..text.len().min(60)]
        );
        let _ = WireConfig::parse(text);
        let _ = WireReply::parse(text);
    }
}

#[test]
fn the_decoder_rejects_instead_of_panicking_on_every_wrong_payload_shape() {
    let kinds = [
        "account.add",
        "account.reconnect",
        "account.remove",
        "settings.apply",
        "countdown.visible",
        "takeover.presented",
        "takeover.closed",
        "takeover.join",
        "takeover.dismiss",
        "takeover.snooze",
        "takeover.test",
        "dedup.merge",
        "dedup.unmerge",
        "dedup.separate",
        "system.event",
        "test.advanceClock",
    ];
    let payloads = [
        serde_json::json!(null),
        serde_json::json!(1),
        serde_json::json!("x"),
        serde_json::json!([]),
        serde_json::json!([1, 2]),
        serde_json::json!({}),
        serde_json::json!({"eventID": 1, "seconds": "x", "eventIDs": "x", "memberIDs": [1]}),
        serde_json::json!({"eventIDs": [1, 2]}),
        serde_json::json!({"kind": 1, "timeZone": 2}),
    ];
    for kind in kinds {
        for payload in &payloads {
            for testing in [false, true] {
                let envelope = Envelope::new(kind).with_payload(payload.clone());
                let outcome = CommandDecoder::new(testing).decode(&envelope, &EngineState::empty());
                // Every one of these is either rejected or (for the all-optional settings patch and a payload
                // without required members) an accepted command; none panics.
                if let CommandOutcome::Rejected(d) = outcome {
                    assert_eq!(d.category, "command");
                }
            }
        }
    }
}
