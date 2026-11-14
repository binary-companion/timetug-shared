import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / 'check-no-clock.py'
REPO = SCRIPT.parents[2]

# A pure module: the time arrives as an argument, and the forbidden names appear only where they do not count.
CLEAN = '''//! Never calls `Timestamp::now()` or reads `std::env`; see `SystemTime`.
use jiff::Timestamp;

/// Takes `now: Timestamp`; never `Instant::now()`.
pub fn due(now: Timestamp, at: Timestamp) -> bool {
    /* tokio::time::sleep is not used here */
    let note = "Zoned::now and std::env::var in a string are only text";
    let raw = r#"SystemTime"#;
    let quote = '"';
    now >= at && !note.is_empty() && !raw.is_empty() && quote == '"'
}

#[cfg(test)]
mod tests {
    #[test]
    fn tests_may_read_the_clock() {
        let _ = jiff::Timestamp::now();
        let _ = std::env::var("X");
    }
}
'''

# One offence per forbidden name, each on its own line (lines 4 to 11).
OFFENDING = '''use jiff::Timestamp;

pub fn bad() {
    let _ = Timestamp::now();
    let _ = std::time::SystemTime::UNIX_EPOCH;
    let _ = std::time::Instant::now();
    let _ = jiff::Zoned::now();
    tokio::time::sleep;
    let _ = std::env::var("HOME");
    let _ = Timestamp :: now();
}
'''
BRACED = 'use std::{env, fmt};\nuse tokio::{sync, time};\n'


def run(files):
    """Runs the script on a temporary repository holding `files` (relative path -> text)."""
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        for relative, text in files.items():
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        result = subprocess.run([sys.executable, str(SCRIPT), '--root', str(root)], capture_output=True, text=True)
    return result.returncode, result.stdout + result.stderr


class NoClockTests(unittest.TestCase):
    def test_the_clean_fixture_passes(self):
        code, output = run({'crates/timetug-core/src/clean.rs': CLEAN})
        self.assertEqual(code, 0, output)

    def test_the_offending_fixture_fails_naming_every_token_and_line(self):
        code, output = run({'crates/timetug-core/src/offending.rs': OFFENDING})
        self.assertEqual(code, 1, output)
        for token in ['Timestamp::now', 'SystemTime', 'Instant::now', 'Zoned::now', 'tokio::time', 'std::env']:
            self.assertIn(f'`{token}`', output)
        for line in [4, 5, 6, 7, 8, 9, 10]:
            self.assertIn(f'offending.rs:{line}:', output)
        self.assertNotIn('offending.rs:1:', output)

    def test_braced_imports_are_caught(self):
        code, output = run({'crates/timetug-bridge/src/imports.rs': BRACED})
        self.assertEqual(code, 1, output)
        self.assertIn('`std::env`', output)
        self.assertIn('`tokio::time`', output)

    def test_the_bridge_is_scanned_too(self):
        code, output = run({'crates/timetug-bridge/src/lib.rs': OFFENDING})
        self.assertEqual(code, 1, output)
        self.assertIn('crates/timetug-bridge/src/lib.rs', output)

    def test_other_crates_are_not_scanned(self):
        code, output = run({'crates/timetug-engine-protocol/src/lib.rs': OFFENDING, 'crates/timetug-engine-c/src/lib.rs': OFFENDING})
        self.assertEqual(code, 0, output)

    def test_the_engine_is_scanned_with_the_same_rules(self):
        code, output = run({'crates/timetug-engine/src/accounts/manager.rs': OFFENDING})
        self.assertEqual(code, 1, output)
        for token in ['Timestamp::now', 'SystemTime', 'Instant::now', 'Zoned::now', 'tokio::time', 'std::env']:
            self.assertIn(f'`{token}`', output)
        self.assertIn('EngineClock', output)

    def test_only_the_system_clock_file_of_the_engine_may_use_tokio_time_and_the_wall_clock(self):
        system_clock = (
            'use std::time::Duration;\n'
            'pub async fn sleep(deadline: jiff::Timestamp) {\n'
            '    let _ = jiff::Timestamp::now();\n'
            '    tokio::time::sleep(Duration::from_secs(1)).await;\n'
            '    let _ = deadline;\n'
            '}\n'
        )
        code, output = run({'crates/timetug-engine/src/clock.rs': system_clock})
        self.assertEqual(code, 0, output)
        # The same code anywhere else in the engine is refused ...
        code, output = run({'crates/timetug-engine/src/engine/mod.rs': system_clock})
        self.assertEqual(code, 1, output)
        self.assertIn('`tokio::time`', output)
        self.assertIn('`Timestamp::now`', output)
        # ... and so is the same file name in the pure crates.
        code, output = run({'crates/timetug-core/src/clock.rs': system_clock, 'crates/timetug-bridge/src/clock.rs': system_clock})
        self.assertEqual(code, 1, output)
        self.assertIn('crates/timetug-core/src/clock.rs', output)
        self.assertIn('crates/timetug-bridge/src/clock.rs', output)

    def test_the_system_clock_file_may_not_read_the_environment_or_other_clocks(self):
        text = (
            'pub fn bad() {\n'
            '    let _ = std::env::var("X");\n'
            '    let _ = std::time::SystemTime::UNIX_EPOCH;\n'
            '    let _ = std::time::Instant::now();\n'
            '    let _ = jiff::Zoned::now();\n'
            '}\n'
        )
        code, output = run({'crates/timetug-engine/src/clock.rs': text})
        self.assertEqual(code, 1, output)
        for token in ['std::env', 'SystemTime', 'Instant::now', 'Zoned::now']:
            self.assertIn(f'`{token}`', output)
        self.assertNotIn('`tokio::time`', output)

    def test_tests_are_excluded(self):
        files = {
            'crates/timetug-core/src/lib.rs': CLEAN,
            'crates/timetug-core/src/tests/clock.rs': OFFENDING,
            'crates/timetug-core/src/tests.rs': OFFENDING,
            'crates/timetug-core/src/inline.rs': '#[cfg(test)]\nfn t() -> [u8; 2] {\n    let _ = std::env::var("X");\n    [0; 2]\n}\n#[cfg(test)]\nuse std::time::SystemTime;\n',
        }
        code, output = run(files)
        self.assertEqual(code, 0, output)

    def test_code_after_a_test_module_is_still_checked(self):
        text = '#[cfg(test)]\nmod tests { fn t() {} }\n\npub fn bad() { let _ = jiff::Timestamp::now(); }\n'
        code, output = run({'crates/timetug-core/src/late.rs': text})
        self.assertEqual(code, 1, output)
        self.assertIn('late.rs:4:', output)

    def test_cfg_test_on_a_field_or_variant_does_not_hide_later_code(self):
        field = 'pub struct S {\n    #[cfg(test)]\n    pub seen: std::collections::HashMap<u8, u8>,\n    pub n: u8,\n}\n\npub fn bad() { let _ = jiff::Timestamp::now(); }\n'
        variant = 'pub enum E {\n    A,\n    #[cfg(test)]\n    B(u8, u8),\n}\n\npub fn bad() { let _ = std::time::SystemTime::UNIX_EPOCH; }\n'
        last_field = 'pub struct S {\n    pub n: u8,\n    #[cfg(test)]\n    pub seen: u8\n}\n\npub fn bad() { let _ = std::env::var("X"); }\n'
        for name, text, line in [('field', field, 7), ('variant', variant, 7), ('last_field', last_field, 7)]:
            code, output = run({f'crates/timetug-core/src/{name}.rs': text})
            self.assertEqual(code, 1, f'{name}: {output}')
            self.assertIn(f'{name}.rs:{line}:', output)
        # The test-only field itself may read the clock.
        code, output = run({'crates/timetug-core/src/ok.rs': 'pub struct S {\n    #[cfg(test)]\n    pub at: std::time::SystemTime,\n    pub n: u8,\n}\n'})
        self.assertEqual(code, 0, output)

    def test_missing_crate_directories_pass(self):
        code, output = run({})
        self.assertEqual(code, 0, output)

    def test_the_repository_passes(self):
        result = subprocess.run([sys.executable, str(SCRIPT), '--root', str(REPO)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == '__main__':
    unittest.main()
