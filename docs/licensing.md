# Licensing of what the libraries ship

This page records which licences apply to the code in the artifacts of this repository, what the Mozilla Public License 2.0 (MPL-2.0) asks of us because the Swift and Kotlin bindings link UniFFI, and where each obligation is met. It is an engineering record, not legal advice.

## The licences in play

| What | Licence | Where it ends up |
|---|---|---|
| This repository's code (the crates `timetug-*`, the Swift and C# wrappers) | MIT with the Commons Clause, `LICENSE` | every artifact |
| `calendar-connectors` crates | MIT (its own `LICENSE`) | every artifact |
| The six UniFFI runtime crates: `uniffi`, `uniffi_core`, `uniffi_internal_macros`, `uniffi_macros`, `uniffi_meta`, `uniffi_pipeline` (all 0.32.2) | **MPL-2.0** | only the bindings library `timetug-engine-ffi`: the XCFramework (macOS, iOS) and the Android `libtimetug_engine_ffi.so` |
| The UniFFI generator crates `uniffi_bindgen`, `uniffi_udl` (0.32.2) | MPL-2.0 | build time only, linked into no artifact; their templates are the source of the generated Swift and Kotlin |
| Every other crate | MIT, Apache-2.0, Unicode-3.0, Unlicense or a choice between them | listed in `THIRD-PARTY-NOTICES.md` |

`cargo deny list` and `cargo metadata` (checked 2026-11-15, Cargo.lock at UniFFI 0.32.2) show no other licence than these. The one crate that offers a copyleft option, `r-efi` (`MIT OR Apache-2.0 OR LGPL-2.1-or-later`), is used under MIT or Apache-2.0, and links on no platform the engine ships on. Nothing in the tree is GPL, LGPL or AGPL only. The C interface library `timetug-engine-c`, and so the .NET package and the Windows app, link no UniFFI crate (`cargo tree -p timetug-engine-c` has none); the Windows package still carries the full notices file, which lists them, because it is one file for both libraries.

## The owner's decision

On 2026-11-15 the owner accepted UniFFI's MPL-2.0 licence for the shipped apps ("Yes, I accept [uniffi's MPL-2.0] license. Make sure that if the license needs to be referenced in my source somewhere, it's properly referenced"). `deny.toml` records it next to the per-crate exceptions. Any other crate under MPL-2.0 or another copyleft licence is a new decision.

## What MPL-2.0 requires of us

MPL-2.0 is a file-level (weak) copyleft licence. Its "Covered Software" is the source files that carry the Exhibit A notice ("This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0 ...", section 1.4), the executable form made from them, and modifications of them. The Rust files of the UniFFI crates carry that notice. We use them unmodified, from crates.io, pinned by version and checksum.

| Clause | What it asks | How it is met |
|---|---|---|
| 2.1, 2.3 | Grant of rights (incl. patents), as is | nothing to do |
| 3.1 Source form | When we distribute the covered source files, they stay under MPL-2.0, and recipients are told so and told how to get the licence | we do not alter or copy them; `THIRD-PARTY-NOTICES.md` names the licence and carries its full text in each UniFFI section |
| 3.2 Executable form | The covered code compiled into our binary: say how to obtain its source, at no more than distribution cost; the binary may be under other terms if those do not limit the recipients' rights in the covered source | the notice in `THIRD-PARTY-NOTICES.md` (top section) gives the source: https://github.com/mozilla/uniffi-rs tag `v0.32.2` and crates.io `uniffi` 0.32.2, with the exact files fixed by `Cargo.lock`; we host no modified copy |
| 3.3 Larger work | The rest of our library may be under our own terms (MIT with the Commons Clause) if we also comply for the covered files | `LICENSE` stays as is; the notice says the MPL-2.0 covers only the UniFFI crates' files |
| 3.4 Notices | Do not remove or change licence notices in the covered files | we never edit them |
| 3.5, 6 | Warranty, support: only on our own behalf; no warranty from the contributors | our `LICENSE` disclaims; nothing else offered |
| 5 | Terminates if the licence is violated (30 days to cure) | keep the items above true |

What it does **not** ask: that the Mac or Windows app, the rest of this repository, or the connector crates be released under MPL-2.0; that the app source be published; that every app screen mention it. What the owner's app has to do is carry the notice (below).

## The generated Swift and Kotlin

`TimeTugKitGenerated.swift` and `kotlin/generated/` are written by `uniffi_bindgen` from its templates plus our interface. Neither carries a licence header, and UniFFI's repository documents no licence for the generated output. The templates in `uniffi_bindgen` 0.32.2 carry no Exhibit A notice (checked in the published crate), so on the definition in section 1.4 they are arguably not Covered Software; that is a legal reading we do not rely on. We treat the template-derived parts of the generated files conservatively: they are distributed with the notices, they are not relicensed, and the pointer to the upstream source and licence in `THIRD-PARTY-NOTICES.md` covers them. The generated files are not edited by hand; a header comment can be added through the generator script if an owner or lawyer wants one.

## Where the notice travels

| Artifact | Carries |
|---|---|
| Source repository and release tag | `LICENSE`, `THIRD-PARTY-NOTICES.md`, this page, the generated sources |
| `TimeTugEngineFFI-X.Y.Z.xcframework.zip` (Swift package binary) | `THIRD-PARTY-NOTICES.md` and `LICENSE` inside `TimeTugEngineFFI.xcframework/` (`scripts/dev/build-xcframework.sh`; `release.yml` stops when the zip lacks them). SwiftPM consumers also get the notices through the package's git checkout |
| `TimeTug.Engine` NuGet package | `LICENSE` at the package root and `contentFiles/any/any/include/THIRD-PARTY-NOTICES.md` (`dotnet/pack/TimeTug.Engine.nuspec.props`; `scripts/ci/tests/test_pack_inventory.py` requires both) |
| Android library (`libtimetug_engine_ffi.so`) | Not released yet. When it is, ship `THIRD-PARTY-NOTICES.md` and `LICENSE` in the same archive or AAR, as the other artifacts do |
| An app that embeds one of these (Mac, iOS, Android, Windows) | Its own acknowledgements: it must reproduce the notices of the libraries it links, and for UniFFI the licence name, the source location and the licence text (the notices file has them). The app repositories track this |

## Keeping it true

- `THIRD-PARTY-NOTICES.md` is generated (`scripts/dev/generate-third-party-notices.py`) and checked by `scripts/ci/check-third-party-notices.py`; run the generator after any change to `Cargo.lock`.
- When `uniffi` is upgraded: update the version in the notice header (the generator), `LICENSE_TEXT_OVERRIDES` in the generator, and re-read the licence of the new crates (`cargo deny list`).
- Do not modify a UniFFI file in this repository or in a fork. If a patch is ever needed, the patched files must be published under MPL-2.0 at a location named in the notices (section 3.2) before a binary ships.
