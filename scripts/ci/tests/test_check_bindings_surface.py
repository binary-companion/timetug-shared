import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / 'check-bindings-surface.py'

LIB = '''
mirror_record!(CalendarEvent <=> eng::CalendarEvent { id: String });
mirror_record!(forward_only EngineState <=> engine::EngineState { revision: u64 });
mirror_unit_enum!(EventStatus <=> eng::EventStatus { Confirmed });
mirror_unit_enum!(UidScope <=> eng::UidScope { Global });
pub enum SeriesInfo { NotRecurring }
pub struct Attendee { pub name: String }
field_update!(FieldUpdateString, String, String);
#[uniffi::export(foreign)]
pub trait CredentialStore: Send + Sync {}
uniffi::custom_type!(Zone, String, { remote });
uniffi::custom_newtype!(Opaque, i64);
#[uniffi::export]
pub fn join_url_is_allowed(url: UrlString) -> bool { true }
// pub struct InAComment {}
'''

DOCS = '''# Bindings

## Names that differ

| Swift name | Generated name | Shim |
|---|---|---|
| `UIDScope` | `UidScope` | typealias |

## Not exposed

| Swift name | Reason |
|---|---|
| `GoogleConnectorKind` | replaced by `EngineConfig` |

## Exposed as functions

| Swift name | Functions |
|---|---|
| `JoinURLPolicy` | `join_url_is_allowed` |

## Planned

| Swift name | Task |
|---|---|
| `TextGenerator` | Task 4 |
'''


def run(names, lib=LIB, docs=DOCS):
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / 'src').mkdir()
        (root / 'src' / 'lib.rs').write_text(lib, encoding='utf-8')
        (root / 'surface.txt').write_text(names, encoding='utf-8')
        (root / 'bindings.md').write_text(docs, encoding='utf-8')
        return subprocess.run(
            [sys.executable, str(SCRIPT), '--source', str(root / 'src'), '--surface', str(root / 'surface.txt'),
             '--docs', str(root / 'bindings.md')], capture_output=True, text=True)


class CheckBindingsSurface(unittest.TestCase):
    def test_every_kind_of_exposed_name_is_accepted(self):
        names = 'CalendarEvent\nEngineState\nEventStatus\nSeriesInfo\nAttendee\nFieldUpdateString\nCredentialStore\nZone\nOpaque\n'
        result = run(names)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_a_renamed_a_not_exposed_and_a_planned_name_are_accepted(self):
        result = run('UIDScope\nGoogleConnectorKind\nTextGenerator\n')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('TextGenerator', result.stdout)  # planned names are reported, not failed

    def test_an_unaccounted_name_fails_and_is_named(self):
        result = run('CalendarEvent\nMysteryType\n')
        self.assertEqual(result.returncode, 1)
        self.assertIn('MysteryType', result.stderr)
        self.assertNotIn('CalendarEvent', result.stderr)

    def test_a_name_inside_a_comment_does_not_count(self):
        result = run('InAComment\n')
        self.assertEqual(result.returncode, 1)
        self.assertIn('InAComment', result.stderr)

    def test_a_rename_to_a_type_that_does_not_exist_fails(self):
        docs = DOCS.replace('`UidScope`', '`NoSuchMirror`')
        result = run('UIDScope\n', docs=docs)
        self.assertEqual(result.returncode, 1)
        self.assertIn('NoSuchMirror', result.stderr)

    def test_a_not_exposed_entry_needs_a_reason(self):
        docs = DOCS.replace('replaced by `EngineConfig`', '')
        result = run('GoogleConnectorKind\n', docs=docs)
        self.assertEqual(result.returncode, 1)
        self.assertIn('reason', result.stderr)

    def test_a_name_exposed_as_a_function_is_accepted(self):
        result = run('JoinURLPolicy\n')
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_a_function_row_naming_a_missing_function_fails(self):
        docs = DOCS.replace('`join_url_is_allowed`', '`no_such_function`')
        result = run('JoinURLPolicy\n', docs=docs)
        self.assertEqual(result.returncode, 1)
        self.assertIn('no_such_function', result.stderr)

    def test_one_row_may_list_several_names_and_a_nested_name_matches_by_its_last_part(self):
        docs = DOCS.replace('| `GoogleConnectorKind` |', '| `GoogleConnectorKind`, `Outer.IcalKind` |')
        result = run('GoogleConnectorKind\nIcalKind\n', docs=docs)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_comments_and_blank_lines_in_the_list_are_ignored(self):
        result = run('# the names the Mac app uses\n\nCalendarEvent\n')
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == '__main__':
    unittest.main()
