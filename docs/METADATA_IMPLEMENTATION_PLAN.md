# Metadata management implementation plan

Date: 22 October 2026. Status: future work, all phases unstarted. Implements [METADATA_SPECIFICATION.md](METADATA_SPECIFICATION.md). No existing milestone checkbox or implementation file is changed by this plan.

## Sequencing and prerequisites

Finish Linux M5 release work first, particularly DAC format-change validation and library preservation. The metadata phases below use the temporary prefix **MT**, avoiding a collision with the existing M6 Windows/macOS milestone. Adopt their numbering in the main implementation plan only when this work is scheduled.

Recommended delivery order: **MT0 → MT1 → MT2 → MT3**, then **MT4 → MT5 → MT6**, with **MT7** enabled only after evidence supports it. MT3 gives an immediately useful offline editor. MT4 and MT5 are independently useful: online proposals do not require file writers, and file writers do not require online services. MT6 (organizing files) reuses MT5's journal and recovery. MT8 is optional expansion. Implement one phase at a time and keep the workspace buildable.

The [Windows plan](M6_WINDOWS_IMPLEMENTATION.md) shares decisions about path identity, replacement semantics, image handling and durable backups. Design those boundaries before choosing Linux-specific shortcuts. Neither proposal authorizes Windows playback or network changes to the current v1 scope by itself.

## MT0 — Preservation fixes and dependency/format spike

- [x] Address review finding T01: distinguish missing/incomplete scans from deletions; preserve tracks/playlist relationships during failed reads and folder consolidation (done in M5). T02, edition identity, moves to MT1.
- [ ] Reconfirm required fields, library-first default, and explicit write/online actions; update the main specification and read-only promise when implementation is authorized.
- [x] Audit current ID3/FLAC readers and candidate writers for unknown-frame/block preservation, duplicate/multi-value fields, ID3 versions, malformed inputs and supported container variants ([decision record](METADATA_DECISIONS.md)).
- [x] Compare focused `metaflac` + existing `id3` with `lofty` where relevant. Read exact-version licences and sources; test capabilities rather than adopting a generic supported-format claim. Outcome: `lofty` for FLAC (and later formats) behind Lyra's own before/after checks, since it silently drops malformed comments; `id3` + own container patch for DSF/DFF.
- [x] Document FLAC, DSF, WAV and DFF writer capabilities separately (capability matrix in the decision record; WAV and DFF stay library-edit-only). Refusal behaviour for links and filesystems is defined with the MT5a write protocol.
- [ ] Confirm MusicBrainz API use and a field/content licensing map; confirm Cover Art Archive image handling. Keep Discogs, AcoustID and editorial integrations outside the initial dependency set.
- [ ] Record an initial 10k-track scan/playback baseline and choose concurrency/I/O budgets from measurement.

**Done when:** missing/unreadable storage and root consolidation preserve user references in tests; a short decision record names exact candidate dependencies, supported write formats and unresolved cases; no music-file writer is exposed yet. MT14 is covered.

## MT1 — Stable identities, layered data and backup

- [x] Add forward-only migrations after the current schema. Back up the database safely before migration (`VACUUM INTO`, newest three kept; migrations may run Rust code, with foreign keys checked before each commits).
- [x] Separate stable local release identity from album title/artist; retain existing track/playlist IDs. Design conservative reconstruction for already-merged editions. _Migration 5: grouping by MusicBrainz release ID, else release folder (disc folders joined), album and album artist; the next scan regroups, and a release whose tracks all move keeps its ID and artwork._
- [x] Track identity independent of the path: a move or rename (MT6, or done outside Lyra) updates the path, not the ID. _Migration 6: `audio_id` (FLAC audio MD5, else codec, size, rate, channels and length); only a unique match on both sides counts as a move._
- [ ] Add file observations, manual overrides, source preferences and accepted provider IDs/values with typed fields and revisions. _File values, edits with origin (manual, rule, provider) and a fixed field list done (migration 7); source preferences and revisions remain._
- [x] Represent absent/set/clear/reset independently. Define release-wide versus track-specific field resolution. _Each field has a track or release scope; a release field's file value is its first track's._
- [ ] Distinguish artist credits from artist identity; allow offline entities with no provider ID.
- [ ] Matching keys (NFC, full Unicode lower-casing, collapsed whitespace, invisible characters removed) for artists and releases, replacing SQLite's ASCII-only `NOCASE`; file values stored exactly, with an unreadable-text flag for invalid bytes ([decisions](METADATA_DECISIONS.md) 5–7). Adds `unicode-normalization`. _Artists (migration 4) and releases (album and album-artist keys in the grouping, migration 5) done; exact file values come with block 4._
- [ ] Add durable artwork references, immutable asset digests and disposable thumbnail/cache accounting.
- [ ] Add edit transaction history and a versioned backup/restore workflow covering the DB and selected artwork. Backup excludes credentials and evictable response caches. _History and undo of saved steps done; backup/restore as a user action comes with the editor._
- [ ] Make rescans update only observations; preserve edits and surface unavailable tracks. Use revisions to reject stale scanner results. _Rescans replace file values only; edits stay and are marked for review when the file changes under them. Revisions remain (MT2)._

**Done when:** migration tests preserve IDs and playlists; two editions with identical display names remain separate; local overrides survive full/incremental scans and restart; backup restores a complete consistent library. Cover MT01, MT02, MT14 and MT15 model cases.

**Risk:** historic album rows may already combine editions. Do not automatically infer a perfect split. Retain reviewable mappings and offer correction where files do not supply enough evidence.

## MT2 — Edit service, preview and artwork lifecycle

- [ ] Implement pure field validation and effective-value resolution with typed errors and operation semantics.
- [ ] Cleanup rules as pure functions proposing edits: text hygiene, encoding repair, numbers, dates, album artist, featured artists, capitals, duplicate artists, genres, sort names ([decision](METADATA_DECISIONS.md) 10). A corpus of badly tagged fixtures, including names each rule must leave alone.
- [ ] Add revision-bound edit previews and atomic local edit commits; include affected IDs and release grouping consequences.
- [ ] Implement local edit undo with revision conflict checks and explicit reset-to-file operations.
- [ ] Add restricted local artwork selection through Rust, bounded decode, durable storage, thumbnail invalidation and total cache accounting.
- [ ] Introduce worker jobs, short database transactions and targeted change events; no I/O/network while a UI/library global mutex is held.
- [ ] Extend typed Rust/TypeScript IPC, validators and Tauri permissions. Resolve selected IDs server-side; do not accept arbitrary write paths.
- [ ] Propagate changed labels to queue, Now Playing, search and desktop metadata without touching the stream format or playback buffers.

**Done when:** service-level tests exercise preview/commit/conflict/undo, replacement artwork and unavailable files; invalid or stale operations make no partial local edit. MT01–MT04 and MT13 pass at the service boundary. Playback engine dependencies remain unchanged.

## MT3 — Embedded offline editor

- [ ] Add album/track entry points and selection-aware General, Tracks, Artwork and Sources tabs.
- [ ] **Tidy up library**: run the cleanup rules over the library or a selection; review proposals grouped by rule with counts and examples; accept per rule or per item; undo.
- [ ] Add accessible multi-selection, mixed values, explicit clear/reset, field validation and unsaved-change handling.
- [ ] Separate album artist from track artist and show selection scope. A shared artist rename is not implied.
- [ ] Implement **Save in Lyra**, undo/reset and read-only file capability messaging. File writing/online controls remain unavailable until their phases ship.
- [ ] Add keyboard-only flow, screen-reader status, focus return and useful empty/error states.
- [ ] Add editor E2E coverage using synthetic library data and verify live label refresh.

**Done when:** a listener can correct an album and its track metadata/artwork offline, undo changes and restart/rescan without losing them; a filesystem comparison confirms source music files are untouched. Deliver this as the first user-facing release. Complete MT01–MT04, MT06 (read-only mode), MT09 (local undo), MT14 and MT15 user workflows.

## MT4 — Reviewed online identification and artwork

- [ ] Add the default-off online setting and first-use disclosure; no background startup/scan requests when disabled.
- [ ] Implement a provider interface for search, release lookup, track alignment and cover candidates, separate from local editing/writing.
- [ ] Implement MusicBrainz identification using reviewed fields and IDs; one shared limiter across jobs, a meaningful user-agent, request deduplication and cached responses under the applicable policy.
- [ ] Add Cover Art Archive lookup by selected release; test all permitted redirect destinations and image limits.
- [ ] Build results/edition comparison, manual track remapping, per-field acceptance and provenance display.
- [ ] Persist accepted field choices separately; provider refresh respects manual overrides and source preferences.
- [ ] Test cancellation, bounded response/decompression, 429/503/backoff, timeout, no result, no art and expired cache; secrets/path information must not appear in requests/logs.
- [ ] Document provider service/content policies, attribution, cache retention and how to disable/clear fetched data.

**Done when:** a selected synthetic/test album can be matched and reviewed; accepted fields/art remain available offline where permitted; ambiguous editions require a decision; all CI provider interactions are mocked. MT10, MT11, MT13 and MT16 pass. A manual live smoke report is dated and separate from CI.

**Risk:** release-level databases can identify a recording while still choosing the wrong mastering. The UI must preserve edition choice and never equate a fingerprint/recording ID with an exact release.

## MT5 — Safe write-back, one container at a time

### MT5a — Job/recovery infrastructure and FLAC

- [ ] Implement revision-bound file-write previews, per-file leases and coordination with scanner/current/prefetched playback sources.
- [ ] Add journal, unique sibling temporary files, backups, disk-space checks, filesystem capability checks, verification and platform-specific atomic replacement.
- [ ] Add recovery for every interruption point: prepared, temp created, verified, replaced but not indexed, indexed but not marked complete, cleanup interrupted.
- [ ] Implement FLAC selected-field/picture updates preserving audio frames and unknown metadata. Read original/temp payload digests before commit; compare decoded synthetic fixtures too.
- [ ] Add **Write tags to files…**, deferred/busy states, per-file batch outcomes, cancellation and backup browsing/restoration with external-change checks.
- [ ] Record a compatibility read with an independent tagger, without bundling it or using its output as the sole integrity test.

**Done when:** FLAC writes satisfy MT03, MT05 and MT07–MT09; crash/disk-full simulation never leaves an unverified original replacement; local edits and playlist IDs survive both success and failure.

### MT5b — DSF

- [ ] Serialize ID3 to the correct metadata location and update DSF length/pointer fields. Preserve data blocks, sample counts, padding semantics, unknown frames and existing tag version.
- [ ] Cover untagged DSF, growing/shrinking art, repeated edits, ID3 v2.3/v2.4, odd tag lengths, malformed pointers and large-file bounds.
- [ ] Verify DSD payload hashes and existing native/DoP fixture outputs before/after writing; independently read resulting tags.

**Done when:** DSF passes the same write/recovery suite as FLAC, including synthetic block-boundary and truncated-input tests. Native/DoP transport bytes are unchanged by tagging. No new hardware claim is inferred from container tests.

### MT5c — WAV, then DFF if proven

- [ ] Validate the WAV INFO/ID3 coexistence policy; preserve arbitrary RIFF chunks/order/padding and refuse RF64/BW64 or oversized RIFF until separately implemented.
- [ ] Validate DFF's specific chunk lengths, alignment, metadata placement and third-party compatibility. Preserve native metadata/unknown chunks; refuse unsupported compressed/structural variants.
- [ ] Apply the same audio invariance, backup and failure-injection gates to each format independently.

**Done when:** each enabled format has a dated capability matrix and round-trip/interoperability evidence. A failing/unproven format stays library-edit-only with an explanation. Do not block FLAC/DSF delivery on DFF support, and do not describe the editor as writing every supported playback format.

## MT6 — Organize files

- [ ] Template engine on effective values: presets and custom templates, multi-disc and compilation rules, fallbacks for missing fields. Pure and unit-tested.
- [ ] Filesystem-safe names: illegal characters, reserved names, leading dots, trailing dots and spaces, NFC, 255-byte components on a character boundary, case-only renames through a temporary name.
- [ ] Preview grouped by album: moves, collisions, other files (covers, booklets, cue/log) moving or staying, skipped files.
- [ ] Execution on the MT5a journal: atomic rename on one filesystem; copy, verify and delete across filesystems; path updated in place in the library; playing and prefetched files deferred; no scan during a batch.
- [ ] Undo by journal, refusing changed files; crash reconciliation; optional removal of emptied folders.
- [ ] **Organize folders** in the UI, from the library, an album or a selection, after cleanup or a match.

**Done when:** MT17–MT20 pass on ext4 and on an exFAT image; a filesystem comparison after undo matches the original tree.

## MT7 — Optional automatic enrichment and release hardening

- [ ] Evaluate matching against synthetic/manual-labelled cases: remasters, compilations, split discs, duplicate names, hidden/bonus tracks and incomplete tags.
- [ ] Add optional scan-triggered proposal jobs only after online consent; low-priority bounded concurrency and cancellable scheduling.
- [ ] Keep **Apply missing fields** separate from lookup consent; restrict it to validated high-confidence cases and documented fields. Never enable automatic file writes by default.
- [ ] Measure scan/edit/artwork/file-write workloads during PCM, native DSD and DoP playback; record CPU/RAM/disk/UI timing and underruns on named hardware. Reduce concurrency or defer writes if the budget fails.
- [ ] Fuzz new parsers/adapters; audit filesystem permissions, redirect policy, credentials, privacy, licences and backup cleanup UX.
- [ ] Complete acceptance matrix MT01–MT16 with links to results and update user documentation/limitations.

**Done when:** all promised format/workflow acceptance rows pass, unresolved cases are disabled or explicitly scoped, and the standard project checks pass. Automation remains optional and playback policy is unchanged.

## MT8 — Separately scoped extensions

Candidates: Discogs after an approved persistence/attribution policy; AcoustID after separate licensing/privacy/CPU design; richer role credits and classical works/movements; custom text fields; M3U8 exchange; custom naming scripts and organizing new imports automatically; rule-based actions. Each requires its own specification delta and acceptance criteria. No scripting interpreter or third-party tagger code is bundled by default.

## Validation and implementation boundaries

For every implementation phase, run the AGENTS.md definition of done: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, frontend formatting and type checking, and `npm run build` in `apps/desktop`. Retain existing lint, frontend unit, dependency-policy and native smoke checks from `scripts/check.sh`/CI. Add meaningful tests for data loss, concurrency, parser invariants and crash recovery rather than tests that merely mirror UI markup.

Use synthetic music/artwork, deterministic malformed fixtures and recorded synthetic HTTP responses. CI does not require real provider credentials or a DAC. Live providers, independent taggers, filesystem semantics and physical DAC acceptance receive separate dated reports. Windows file writing must pass on NTFS with real replacement/open-handle behavior before being enabled there.

## Effort and decisions still required

These are engineering estimates for one developer familiar with the codebase, including implementation/tests/docs and assuming M5 is stable; they are not dates or commitments:

| Work                                           | Indicative effort                                              |
| ---------------------------------------------- | -------------------------------------------------------------- |
| MT0–MT2, preservation, migrations and services | 2–4 developer weeks                                            |
| MT3, polished offline editor                   | 1–2 weeks                                                      |
| MT4, reviewed MusicBrainz/CAA                  | 1–3 weeks                                                      |
| MT5a–b, safe FLAC/DSF writes and recovery      | 2–4 weeks                                                      |
| MT5c, WAV/DFF                                  | 1–3+ weeks depending on preservation/interoperability findings |
| MT6, organize files                            | 1–2 weeks after MT5a                                           |
| MT7, automation/measurement/hardening          | 1–2 weeks                                                      |

The first useful offline editor is therefore roughly 3–6 developer weeks after prerequisites; the broader initial system, organizing files included, is roughly 8–17 weeks including release hardening, excluding optional WAV/DFF/automation expansion. Re-estimate after MT0 using actual writer and migration results. Existing merged albums, unfamiliar tag variants, disk recovery and Windows semantics can expand these ranges.

Before coding, settle exact field/date/list mappings, the writer dependencies and unsupported variants, edition migration rules, backup storage UX and provider field licences. The specification already proposes conservative defaults; these are design decisions for implementation review, not reasons to change current files during this documentation task.
