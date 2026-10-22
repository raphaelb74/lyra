# Product and technical review

Review date: 22 October 2026. Baseline: working tree based on `4c874f5`, including the existing uncommitted and untracked work. This is a source and documentation review, not a new runtime, accessibility, benchmark, or hardware test report. No implementation changes or milestone completions are proposed by this document.

Related proposals:

- [Metadata specification and feasibility](METADATA_SPECIFICATION.md)
- [Metadata implementation plan](METADATA_IMPLEMENTATION_PLAN.md)
- [M6 Windows implementation](M6_WINDOWS_IMPLEMENTATION.md)

## Assessment

Lyra already has the main parts of a focused local music player: album/artist/track browsing, artwork, playlists, an editable queue, shuffle/repeat, seeking, PCM and DSD playback, explicit conversion policy, hardware volume, diagnostics, and a signal path. The current work also adds queue drag ordering and its keyboard alternative, signal-path presentation, and a licence screen. These should be reviewed as existing work, rather than scheduled as missing features.

The next priority is confidence in daily use: preserve library data when storage disappears, distinguish album editions, validate DAC transitions, make a large library fully reachable, and finish release evidence. Metadata editing is feasible and fits this product well. Windows is feasible, but the existing `AudioSink` abstraction does not yet isolate all Linux assumptions.

The suggested order is: finish Linux M5 and address library preservation; establish stable metadata identities; deliver local metadata edits; add reviewed online enrichment and safe file writes; deliver Windows PCM/DoP. ASIO native DSD is an independent, later gate. Metadata and Windows can be implemented in either order once the shared identity/path decisions are settled.

## What a listener would still miss

Priority: **P0** can lose user-managed library information or undermine safe playback; **P1** is important for a dependable release; **P2** improves collection management. These are recommendations, not revised milestone commitments.

| Priority | User need                                        | Current evidence                                                                                                                  | Recommended outcome                                                                                                                                |
| -------- | ------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| P0       | Unplug a music drive and keep playlists          | A scan silently skips unreadable directories, removes tracks it did not find, and playlist foreign keys cascade on track deletion | Mark folders/tracks unavailable; distinguish a complete scan from an incomplete traversal; remove library records only through a deliberate policy |
| P0       | Switch between PCM and DSD without hiss          | The SE hardware report records intermittent hiss and a warm-up workaround awaiting confirmation                                   | Repeat replug, first play, mode/rate changes and quit tests; publish the limits of each verified DAC/driver combination                            |
| P0       | Send DoP only to a DAC that can decode it        | Negotiation checks carrier format/rate and USB volume integrity, without a separate confirmed DoP-decoding capability             | Distinguish carrier acceptance from DAC DoP support; refuse unknown endpoints until compatibility is established                                   |
| P1       | Browse the entire collection                     | Tracks renders the first 500 matches, then asks the user to refine search                                                         | Pagination or accessible virtualization so every result can be reached without inventing a narrower query                                          |
| P1       | Keep different editions separate                 | Albums are unique by title plus album artist                                                                                      | Stable release identities; retain separate remasters, formats, editions and folder imports unless the user merges them                             |
| P1       | Correct album/artist names and artwork           | Scanner reads tags; no editor or write commands                                                                                   | Album/track editing, multi-selection, preview, undo, provenance, and a separate file-tag write action                                              |
| P1       | Reopen Lyra with the intended output and session | Device selection starts empty and selects the first listed device; engine queue/shuffle/repeat are not restored from settings     | Persist preferred endpoint and playback session; restore stopped/paused and require a decision when the saved device is missing                    |
| P1       | Reconnect a DAC without manual refresh           | Development docs explicitly describe Refresh and no hotplug detection                                                             | Refresh device availability automatically; retain selected identity; never migrate running playback silently                                       |
| P1       | Understand the controls and recover from errors  | Keyboard support and error UI exist; shortcut reference and hardware lamp checks remain in M5                                     | In-app shortcut help; keyboard/screen-reader walkthroughs; actionable empty, missing-file, device-busy and disconnected states                     |
| P1       | Install and upgrade reliably                     | `.deb` is the configured bundle; CI builds it; other package formats are planned                                                  | Validate installation, upgrades, data retention and uninstall on supported distributions before claiming release support                           |
| P2       | Find correct covers and release information      | Local embedded/folder artwork only                                                                                                | Explicit online lookup, edition selection and field-by-field review; offline operation remains complete                                            |
| P2       | Maintain a collection over time                  | No user-facing backup/restore, relink workflow or playlist exchange found                                                         | Versioned backup/restore of user data and edits; relink missing roots; later M3U8 import/export                                                    |
| P2       | Organize beyond album/artist                     | Genre filtering exists; richer credits and work/movement relationships are absent                                                 | Add composer, credits, dates and identifiers first; classical work structures and smart collections later                                          |

MP3, AAC/ALAC, AIFF, CUE support, ratings/favourites, listening history, smart playlists and advanced tag automation are possible later additions. They are not v1 release blockers under the present specification. Streaming, multi-room and remote access remain outside its scope; Roon parity would be a substantially different project.

## Technical findings

### T01 — Incomplete scans can delete library relationships (P0)

Evidence: [`scan.rs`](../crates/library/src/scan.rs), `audio_files`, `scan`, `write_batch`; [`db.rs`](../crates/library/src/db.rs), migrations 1 and 2.

`audio_files` returns just a list. `read_dir` failures and failed entries are skipped without telling the reconciliation phase that traversal was incomplete. `scan` treats all remaining known tracks as deleted. A previously indexed file that fails tag/format reading is also deleted by `write_batch`. Deleting tracks cascades to playlist entries. Removing one library folder while adding an encompassing root also deletes the old folder and cascades its tracks before rescanning.

These are source-level consequences, not reproduced failures in this review. A missing external drive or temporary permission problem can therefore discard playlist membership even though the music remains intact elsewhere. Introduce explicit scan completeness and availability; retain track identity across root changes; keep unreadable existing tracks with an error status. Add regression tests for missing roots, unreadable subtrees, transient parse failure and overlapping-folder consolidation before metadata work begins.

### T02 — Display strings double as entity identity (P1)

Evidence: [`db.rs`](../crates/library/src/db.rs), `UNIQUE (title, artist_id)` and unique artist names; [`scan.rs`](../crates/library/src/scan.rs), `write_track`.

Two releases with the same album name and album artist are combined. Artist identity is name-based. This is insufficient for remasters, ambiguous artist names and online provider IDs. The year is filled with `COALESCE(albums.year, ?3)`, so a changed year need not replace an existing album value. Plan migrations around stable local IDs, explicit release grouping, artist credits and provider identifiers. Do not rename shared SQL rows as the implementation of an album edit.

### T03 — Artwork refresh and accounting need an explicit lifecycle (P1)

Evidence: [`scan.rs`](../crates/library/src/scan.rs), `if !has_art`; [`art.rs`](../crates/library/src/art.rs), `MAX_CACHE_BYTES`, `store`; [`thumbs.rs`](../apps/desktop/src-tauri/src/thumbs.rs).

Existing album artwork is retained once present, so changed embedded covers are not reliably refreshed by a rescan. Folder-cover changes alone do not change audio-file size/mtime. The 512 MiB cap accounts for originals recorded in SQL, while generated thumbnails consume additional space. Names use album ID and second-resolution time, which also needs reconsideration for rapid successive edits and immutable HTTP caching. Introduce asset hashes/revisions, selected-art provenance, cover-file stamps, replacement invalidation and a measured total cache budget. User-selected artwork must be durable data rather than an evictable scan cache.

### T04 — Realtime guarantees need measurement (P1)

Evidence: [`player.rs`](../crates/unda/src/player.rs), `Player::new`; [`writer.rs`](../crates/unda/src/writer.rs), `run`, `send`; [`sink.rs`](../crates/audio-alsa/src/sink.rs), `write`, `drain`.

The reusable eight-buffer pool bounds audio payloads, but it uses `std::sync::mpsc::channel`; command/event channels are also unbounded. Bounded payload count alone does not establish the stated guarantee of bounded queues and no allocation or blocking synchronization in the steady-state writer. Audit queue internals and measure allocations on actual writer/sink paths under sustained playback, transitions and underruns. Document which device waits are necessary, their cancellation limits and the chosen underrun behavior for PCM versus DSD/DoP. Keep parser/network/database work outside this loop.

Existing mock-sink tests already verify PCM ordering, seek, same-format gapless playback, DoP phase, conversion and volume-change handling. Extend these tests and collect physical evidence; do not treat them as absent or as proof of the whole hardware path. Current position counts frames handed to the sink, so audible position and track-change timing may lead the device by queued frames; validate before exposing precise external timelines.

### T05 — Library scalability is not yet demonstrated (P1)

Evidence: [`db.rs`](../crates/library/src/db.rs), `tracks`, `albums`; [`TrackList.svelte`](../apps/desktop/src/lib/TrackList.svelte), `SHOWN = 500`; [`scan.rs`](../crates/library/src/scan.rs), whole-tree discovery.

Queries return complete lists and the frontend filters/sorts them. Scan discovery builds a whole collection before processing; cancellation is checked later between files. Define a 10k-track benchmark covering initial/incremental scan, cancellation, search, scrolling, artwork memory, UI latency and audio underruns. Move toward paged queries and incremental discovery based on measurements. The M4 checkboxes do not substitute for its still-required 10k-track acceptance evidence.

### T06 — Recovery and release evidence remain incomplete (P1)

Evidence: [implementation plan](IMPLEMENTATION_PLAN.md), [hardware reports](hardware/), [check script](../scripts/check.sh), [CI](../.github/workflows/checks.yml).

Complete A01–A12 and conversion acceptance evidence, especially MPRIS on the desktop, busy-device handover, hotplug, sustained gapless playback and format-change safety. The SE report explicitly leaves DoP and higher native rates unverified, and its warm-up result still needs user/hardware confirmation. Do not transfer ZEN ONE results to the SE or Windows.

Parser fuzzing, release privacy review and package installation remain planned. Dependency policy checks, lockfile inventories, fixtures and CI already exist. Refresh inventories/notices after dependency changes and reconcile stale docs: README still says M4 in progress; Development still contains a dropped-file-only sentence; the third-party overview still describes the M0 dependency set despite later features. These existing files were left untouched for this review.

### T07 — Windows needs shared boundaries beyond `AudioSink` (P1 for M6)

Evidence: unconditional ALSA/D-Bus/gdk-pixbuf desktop dependencies; Unix `OsStrExt` and `MetadataExt` in the library; ALSA-specific IPC reports and format labels; Linux-only bundle, CI and dependency-policy target.

Keep UNDA and format readers shared. Add platform adapters for device discovery, volume, diagnostics and desktop media controls; versioned native path encoding; portable image processing; WebView2-compatible artwork URLs; native Windows CI and installers. Details and acceptance gates are in [M6 Windows](M6_WINDOWS_IMPLEMENTATION.md).

### T08 — Metadata writing changes the trust and persistence model (P1 for metadata)

Evidence: [`tags.rs`](../crates/library/src/tags.rs) is a bounded reader; current IPC exposes no editor or network operations; [specification](SPECIFICATION.md) promises read-only local operation.

Adding a text field to the UI is easy; keeping edits through rescans and writing safe container-specific tags are the substantial work. Introduce local/file/provider metadata layers, stable IDs, conflict detection and a recoverable file-write protocol. Online access must be opt-in, with provider-specific terms and retention rules. Before implementation, explicitly revise the existing v1 read-only/no-remote-services scope; these proposal documents do not silently amend it.

### T09 — DoP carrier support does not establish DAC decoding (P0)

Evidence: [`sink.rs`](../crates/audio-alsa/src/sink.rs), `open_stream`; [`volume.rs`](../crates/audio-alsa/src/volume.rs), `inspect`; [`UNDA negotiation`](../crates/unda/src/sink.rs), `OutputFormat::negotiate` and `HardwareVolume::passes_dop`.

The reviewed path accepts a DoP candidate based on PCM carrier format/rate and volume integrity. USB Audio identification and 0 dB/no volume controls do not establish that the endpoint decodes DoP. A high-rate PCM-only USB output could satisfy those checks and render the carrier as noise. This is a source-level capability gap, not a reproduced hardware failure. Add a separate confirmed-DoP capability or explicit known-compatible device configuration before transmitting to unknown endpoints; test refusal with a PCM-only USB mock. Apply the same distinction to Windows rather than carrying this assumption into the port.

## Recommended release sequence

| Stage                          | Deliverable                                                                                                                                       | Gate                                                                                             |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| Finish M5                      | Safe missing-storage handling and DoP eligibility, measured library browsing, completed accessibility/media-key checks, packages and DAC evidence | Required project checks plus recorded A01–A12 outcomes; unresolved hardware cases clearly scoped |
| Metadata foundation            | Stable releases/tracks, edit revisions, separate source layers, backup/restore                                                                    | Migration preserves playlist IDs and all user data                                               |
| Metadata local editor          | Album/track edits, multi-edit, artwork, undo                                                                                                      | Offline edits survive restarts/rescans; no music files change                                    |
| Metadata enrichment and export | MusicBrainz/CAA proposals, reviewed field selection, verified format writers                                                                      | Terms reviewed; failed jobs recover; payload invariance fixtures pass                            |
| M6 Windows                     | Shared portability, WASAPI PCM/DoP, media controls, signed installer                                                                              | Native Windows CI and physical Windows DAC reports                                               |
| Optional expansion             | ASIO DSD, additional providers/formats, automation                                                                                                | Separate dependency, licensing and hardware gates                                                |

This review does not report a fresh test pass. Builds/tests were not run because the requested deliverable is documentation and the existing working tree is under active development. Only new planning documents are added.
