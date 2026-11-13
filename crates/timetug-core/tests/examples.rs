//! Runs the example in `examples/next_takeover.rs` as a test, so the code shown in `docs/api.md` is proven by `cargo test`.
#![allow(clippy::print_stdout)]

#[path = "../examples/next_takeover.rs"]
mod next_takeover;

#[test]
fn the_example_runs() {
    next_takeover::main();
}

/// `docs/api.md` shows this example verbatim; fail when the two drift apart.
#[test]
fn the_api_reference_shows_the_example_as_it_is() {
    let doc = include_str!("../../../docs/api.md");
    let source = include_str!("../examples/next_takeover.rs");
    assert!(doc.contains(source.trim_end()));
}
