# Embedded metadata management: analysis and specification

Date: 22 October 2026. Status: proposed future scope; no code or music-file changes. This document complements [SPECIFICATION.md](SPECIFICATION.md); implementation requires an explicit scope update because v1 currently promises read-only music folders and no remote services. See the [implementation plan](METADATA_IMPLEMENTATION_PLAN.md) and [current-state review](CURRENT_STATE_REVIEW.md).

## 1. Feasibility and product direction

Yes: Lyra can embed album/track tag editing, batch corrections, artwork selection and online metadata lookup. Rust can perform parsing, provider access and file writing off the UI/audio threads, with Svelte presenting edits and comparisons through typed Tauri commands.

The existing library already reads a useful subset of tags from FLAC/WAV through Symphonia and DSF/DFF through `id3`. SQLite, a background scanner, artwork serving and change events are available. Missing pieces are durable edits, release identities, full tag preservation, provider adapters, file writers and recovery. A scan reader is not automatically a safe tag writer.

Use these references as workflow inspiration, not components to embed:

| Reference                                                                                            | Relevant behavior                                                       | Proposed Lyra scope                                                                             |
| ---------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| [Yate](https://2manyrobots.com/yate/)                                                                | Broad tag control, actions and external database integration            | Album/track editor and preview first; scripting and specialist automation later                 |
| [Mp3tag](https://www.mp3tag.de/en/index.html)                                                        | Batch tag changes, cover editing, online tag sources and filename tools | Multi-edit and artwork; renaming by template in the organize phase                              |
| [MusicBrainz Picard](https://picard.musicbrainz.org/)                                                | Match a release, then write tags and move files by a naming script      | Reviewed match (MT4), then organize by template (MT6)                                           |
| [Plex](https://support.plex.tv/articles/201018497-fix-match-for-albums/)                             | Fix match: re-identify an album; the library shows the matched data     | Fix match maps to reviewed online identification; files stay put unless the user organizes them |
| [Roon editing](https://help.roonlabs.com/portal/en/kb/articles/editing-and-grooming-your-collection) | Separates manual edits, identified metadata and file tags               | Keep source layers and provenance; saving library edits and writing files are distinct actions  |

The initial product should solve everyday album cleanup without requiring a dedicated tagging application. Full Yate/Mp3tag automation, Roon's licensed editorial catalogue, artist biographies, streaming integration and its relationship graph are separate projects. Open release databases do not automatically provide rights to reuse reviews, biographies or every image.

## 2. Scope and user stories

### Initial supported workflows

1. Correct an album title or album artist for one selected release, without changing another edition with the same name.
2. Correct a track title, track artist, disc/track position, date, genre or composer.
3. Edit several tracks together; show mixed values and change only deliberately edited fields.
4. Add or replace cover art from a local file; preview it; choose library storage first and embedding separately.
5. Search online for the correct release, compare editions and track alignment, choose fields, then save the accepted result.
6. Choose **Write tags to files…**, review destinations, supported fields and backups, then apply a recoverable batch.
7. Revert local edits, restore file tags from a verified backup, or remove an online match without losing unrelated edits.
8. Keep music playing during edits/lookups; defer file replacement when playback has the file open or prefetched.
9. Tidy up a poorly tagged library: reviewed cleanup proposals for spacing, broken encodings, track and disc numbers, dates, album artists, featured artists, capitals, duplicate artists and genres ([decision record](METADATA_DECISIONS.md) 10). Special characters follow decisions 5–9 there.
10. Organize folders: after cleanup or a match, move and rename files into a chosen layout, such as `Album Artist/Year - Album/01 Title.flac`, with a full preview and undo ([§8](#8-organizing-files)).

### Release boundaries

| Capability                               | First delivery                                                             | Later extension                                                           |
| ---------------------------------------- | -------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| Local album/track edits                  | Required                                                                   | Artist-wide identity merge tools                                          |
| Multi-edit                               | Required for an album/track selection                                      | Arbitrary collection-wide rules and regular expressions                   |
| Local artwork                            | Required                                                                   | Artist photos, multiple booklet/back images                               |
| MusicBrainz matching + Cover Art Archive | First online delivery, opt-in                                              | Explicit automatic enrichment of newly imported albums                    |
| File writing                             | FLAC, then DSF after independent validation                                | WAV after chunk-preservation tests; DFF only after interoperability proof |
| File renaming/moving                     | MT6: template-based organize with preview and undo, inside library folders | Custom naming scripts; watching folders and organizing new imports        |
| Audio fingerprinting                     | Excluded initially                                                         | Separate CPU, privacy, conversion and licensing work                      |
| Playback formats                         | Existing FLAC/WAV/DSF/DFF                                                  | MP3/AIFF/ALAC require separate playback/import acceptance                 |

Being able to edit a format does not imply Lyra can play it. Do not add unsupported playback formats indirectly through a tagging dependency.

## 3. User interface and behavior

Add **Edit metadata…** to album and track context menus and the album action menu. A selection-aware editor has General, Tracks, Artwork and Sources tabs. Later, an Advanced tab may expose supported custom text fields. Multi-selection and keyboard access are prerequisites for batch editing.

The editor shows the number and identity of selected tracks/releases, unsaved changes, and the current save destination. Use explicit actions:

- **Save in Lyra**: persist library edits; music files are untouched.
- **Find metadata online…**: display the provider and disclose what is sent, then search.
- **Write tags to files…**: preview the file changes, unsupported fields and backup location before committing.
- **Reset to file tags**: remove selected manual/provider choices with a preview.

For an album-artist edit, explain that album artist controls release grouping while track artist controls individual credits. A global artist rename must be a separate, scoped operation with affected-release counts. The ordinary album editor must not update every row referencing a shared artist.

Multi-edit fields distinguish **unchanged**, **set value**, **clear**, and **inherit/reset**. Mixed values are a display state, never a literal string to save. Lists such as genres and artists require explicit replace/add/remove semantics. The initial implementation may support replacement only if it says so. Clearing a value suppresses lower layers; resetting removes the override and reveals the next source.

The Tracks tab preserves disc and track order, displays total counts, and flags duplicate/missing positions. Warn about unsaved changes on dismissal. After saving, album lists, search, queue labels, Now Playing and desktop metadata update without restarting the audio stream. Keyboard focus returns to the originating item. Errors remain in the editor with retry and per-file details.

Online results show release title, credited artist, country, date, label, catalogue/barcode, media format, disc/track counts and durations when available. The user can choose an edition, repair track mapping and accept fields individually. Missing covers, incomplete tags, ambiguous matches and provider failures are normal states. Nothing is overwritten merely because a search succeeded.

## 4. Metadata fields and identity

### Domain fields

| Group            | Fields                                                     | Rules                                                                           |
| ---------------- | ---------------------------------------------------------- | ------------------------------------------------------------------------------- |
| Track            | Title, artist credit, artist list, sort title/artist       | Store credited text separately from identity/list order                         |
| Release          | Album title, album artist credit/list, sort album/artist   | Selected release scope; no automatic cross-edition merge                        |
| Position         | Disc number/total, track number/total                      | Optional positive integers; validate duplicate positions and totals             |
| Dates            | Release date, original release date                        | Preserve precision: year, year-month, or full date; do not invent missing parts |
| Classification   | Genre list, compilation flag                               | Preserve multi-values; avoid implicit delimiter splitting                       |
| Credits          | Composer; later performer, conductor, orchestra, producer  | Roles and ordered values; later artist entities can be linked separately        |
| Edition          | Label, catalogue number, barcode, country, edition note    | Optional strings; barcode remains text to retain leading zeroes                 |
| IDs              | Provider + entity type + ID; ISRC                          | Distinguish MusicBrainz release, release-group, recording and release-track IDs |
| User information | Comment, local notes                                       | Local notes remain local unless explicitly mapped to a supported tag            |
| Artwork          | Selected front cover, MIME, dimensions, digest, provenance | Multiple source candidates, one effective front cover initially                 |

Duration, codec, DSD rate, sample depth and channels are read-only properties of the actual file. An online match cannot change them. Tagging must not apply ReplayGain, normalization, DSP, sample conversion or transport changes. Unknown existing ReplayGain/custom tags are preserved without activating processing.

### Stable identities

Current albums use `(title, album artist)` identity and tracks are indexed by path. Replace the album uniqueness policy before offering edits. Introduce a stable local release ID independent of display tags; retain track IDs through updates, planned moves and root changes. Different folders/editions start separate unless grouping evidence or a reviewed merge connects them. Multi-disc grouping needs an explicit grouping rule and a correction UI, not just matching titles.

Keep artist display credits distinct from stable artist entities. Provider IDs are optional links, not mandatory primary keys: the library must support unknown, ambiguous and fully offline music. A recording match alone does not identify an edition/mastering. Attaching a provider release must not merge two local copies automatically.

Migration must back up the existing database and keep playlist/track references. Previously merged editions cannot always be split automatically from existing album rows: re-read each track's file tags/path, propose conservative groups, and preserve the old mapping until reviewed. No new IDs should be created just because an album title changes.

## 5. Source layers and persistence

Keep file observations, accepted provider data and manual edits separately. Resolve each field using:

1. A manual set/clear override, when present.
2. Explicit per-field source preference, when configured.
3. A nonempty observed file value.
4. An accepted provider value for a missing field.
5. A display fallback such as filename or Unknown Artist.

This conservative default preserves existing tags. Choosing a provider field in the comparison stores an explicit source choice; subsequently supplied file values must not silently overturn that choice. Choosing a custom value stores a manual override. Provider candidates remain proposals until accepted. A provider refresh cannot overwrite manual edits.

Each value/choice records scope, typed field, origin, source revision, timestamp, and provider/entity ID where relevant. Store explicit clearing separately from absent values. Unselecting a provider removes its data/choices and falls back predictably. A rescan replaces the file layer only; it does not wipe edits. External tag changes appear as differences and may invalidate a pending file-write preview.

Conceptual schema additions, finalized during implementation rather than assigned a migration number now:

| Data                                            | Purpose                                                              |
| ----------------------------------------------- | -------------------------------------------------------------------- |
| Stable release IDs and track/release membership | Edition identity and controlled split/merge                          |
| File metadata observations + file revision      | Raw normalized fields from the last complete read                    |
| Manual overrides + field preferences            | Typed set/clear/reset choices with entity revision                   |
| Provider links and accepted field values        | Provenance, provider IDs, fetch time and applicable retention policy |
| Artwork assets/references                       | Durable selected artwork separated from disposable thumbnails        |
| Edit transactions                               | Before/after values, affected IDs and undo revision checks           |
| File-write jobs/items                           | Prepared, verified, replaced, reconciled, failed and recovery states |

Use normal relational tables for identity and relationships, with a versioned typed field representation for extensible metadata. Avoid untyped arbitrary JSON as the only data contract. Add migrations after current schema version 3; do not rewrite shipped migrations. Back up through SQLite's supported snapshot/backup mechanism or a closed database, including WAL handling, rather than copying an active main DB file alone.

## 6. File formats and write support

| Format | Current read path                 | Proposed tag representation                                           | Initial write position                                               |
| ------ | --------------------------------- | --------------------------------------------------------------------- | -------------------------------------------------------------------- |
| FLAC   | Symphonia metadata/visuals        | Vorbis comments and FLAC PICTURE blocks                               | First writer after preservation fixtures                             |
| DSF    | Container metadata offset + `id3` | ID3v2 at the metadata pointer                                         | Second writer; update DSF file length/pointer correctly              |
| WAV    | Symphonia                         | RIFF INFO and/or ID3 chunk under an explicit compatibility policy     | Local edits initially; writing follows RIFF tests                    |
| DFF    | Container ID3 location + `id3`    | Container-specific ID3 chunk conventions and existing native metadata | Local edits initially; writing deferred until independently verified |

Example mappings, subject to adapter compatibility tests:

| Domain                      | FLAC                                                   | ID3v2                                                  |
| --------------------------- | ------------------------------------------------------ | ------------------------------------------------------ |
| Title / album               | `TITLE` / `ALBUM`                                      | `TIT2` / `TALB`                                        |
| Track artist / album artist | `ARTIST` / `ALBUMARTIST`                               | `TPE1` / `TPE2`                                        |
| Track/disc positions        | `TRACKNUMBER`, `TRACKTOTAL`, `DISCNUMBER`, `DISCTOTAL` | `TRCK`, `TPOS` with optional totals                    |
| Release/original date       | `DATE`, `ORIGINALDATE`                                 | Version-aware date frames, e.g. `TDRC`, `TDOR` in v2.4 |
| Genre / composer            | `GENRE` / `COMPOSER`                                   | `TCON` / `TCOM`                                        |
| Front cover                 | FLAC PICTURE type 3                                    | `APIC` front cover                                     |
| Provider IDs                | Documented MusicBrainz tag conventions                 | ID3 `UFID`/`TXXX` conventions verified per ID type     |

Preserve the existing ID3 version by default; show compatibility limits instead of silently dropping fields in a v2.3/v2.4 conversion. Preserve unknown frames/comments, multiple pictures, private tags and container chunks. Do not translate conflicting WAV INFO/ID3 values without showing the selected policy. RF64/BW64 and files beyond supported RIFF limits must be explicitly unsupported until tested. DSF/DFF writing is container work, not just calling a generic ID3 path writer.

Candidate libraries need a source/licence/preservation spike before selection:

- [`id3`](https://docs.rs/id3/latest/id3/struct.Tag.html), already used, offers tag serialization and file-writing APIs; DSF/DFF integration still needs container-specific validation.
- [`metaflac`](https://docs.rs/metaflac/latest/metaflac/) is a focused FLAC read/write candidate.
- [`lofty`](https://docs.rs/lofty/latest/lofty/) covers FLAC and WAV, among other formats; its published table does not list DSF/DFF. Do not assume DSD support.

Symphonia remains the decoder. Avoid a broad metadata conversion that normalizes or discards unrecognized fields. Record exact chosen versions and licences in the existing inventory. No dependency is selected or added by this proposal.

## 7. Safe file writing and audio preservation

File writing is an explicit, recoverable operation, separate from saving the library. Initial supported storage is local, writable regular files in selected library roots. Network shares, symlinks/hardlinks, unusual ownership/ACLs and unsupported containers may be refused with a reason until their semantics are tested.

1. Prepare an immutable preview from library IDs, entity revisions and fresh file identity/revision. Include fields, artwork, size/space estimate, backups and unsupported operations.
2. Acquire a per-file worker lease coordinated with scan and playback source opening. Local edit saves need no audio interruption. File replacement waits until neither the current nor prefetched source holds it; a user may explicitly stop playback to proceed. Lock ordering must never make the realtime writer wait for metadata.
3. Revalidate target identity, root containment and revisions. Detect external changes, links and path replacement; do not rely on path strings or mtime/size alone. Refuse stale previews instead of overwriting an external tagger's work.
4. Create a unique temporary sibling and durable recovery backup without modifying the original. Copy audio payload and all untouched container content; change selected metadata only. Check disk space and retain required ownership/permissions/ACLs; refuse if preservation cannot be met.
5. Reopen the temporary file; verify requested tags and container bounds. For this first implementation, compare streaming cryptographic digests of original/copied audio payload and immutable audio properties before replacement. This is worker disk I/O, never audio-thread work.
6. Flush data and perform a platform-appropriate atomic replacement on the same filesystem, with durable journal states and directory flush where supported. Linux rename and Windows replacement/open-handle semantics require distinct implementations. Never truncate the original in place.
7. Re-read tags, update the file layer/stamp and materialized display values without replacing track IDs, invalidate artwork as needed, commit the item journal, and notify the UI once reconciliation is complete.

SQLite and multiple file replacements cannot form one atomic transaction. Each file is atomic; a batch can partially succeed. Report saved, skipped, failed, deferred and cancelled items separately. Cancellation finishes the current atomic item and leaves unstarted files intact. After a crash, reconcile journal/backup/temp/original state before retrying; never replay a replacement blindly.

Local undo restores earlier override/source choices with revision checks. File undo uses a verified retained backup and refuses to overwrite later external changes. Show retention/disk usage and require a deliberate cleanup action; do not promise undo after backup deletion. Initial policy: backups persist until explicitly removed; before a batch, require free space for backups plus the largest in-flight temporary copy and a safety margin. Export/import of local edit history and durable artwork is part of recoverability.

Full-file hashes are expected to change after tagging. Audio invariance means unchanged FLAC compressed audio frames (plus decoded fixture samples), unchanged WAV `data`, unchanged DSF/DFF DSD payload/order/sample count, and unchanged immutable stream properties. Verify DoP/native test outputs before and after edits using existing synthetic fixtures. This does not establish a new DAC bit-perfect claim. FLAC block structure is documented by [Xiph and RFC 9639](https://xiph.org/flac/format.html).

## 8. Organizing files

After cleanup or a match, the user may move and rename files into one consistent layout. It is an explicit action with a preview, never a side effect of an edit, a match or a scan.

### Layout

- **Templates** use effective values, after edits and accepted matches. Presets: `Album Artist/Album`, `Album Artist/Year - Album` (default) and `Genre/Album Artist/Album`; each file is named `01 Title.ext`. A custom template takes the same fields.
- **Multi-disc** releases get a `1-01` prefix, or `Disc 1/` subfolders when the user prefers. Single-disc releases get neither.
- **Compilations** go under `Various Artists`.
- **Missing fields** become `Unknown Artist`, `Unknown Album` or the existing file name, never an empty component.
- **Scope:** files stay inside their library folder, or move to another library folder the user picks. Nothing is written outside the library folders.

### Names safe on every filesystem

Music often lives on exFAT or FAT USB drives and SMB shares, not only ext4. Generated names therefore:

- replace `/ \ : * ? " < > |` and control characters;
- drop trailing dots and spaces;
- avoid reserved Windows names (`CON`, `NUL`, `COM1` …);
- never start with `.`, which the scanner treats as hidden;
- are written in NFC;
- cut each component at 255 bytes on a character boundary, keeping the extension.

A rename that only changes letter case goes through a temporary name, so it works on case-insensitive filesystems.

### Collisions and other files

- **Collisions:** two tracks mapping to the same name, or an unrelated file already at the target, stop that album and show it. Nothing is overwritten. A ` (2)` suffix is offered as a proposal, not applied on its own.
- **Other files:**
  - Covers, booklets, `.cue` and `.log` files move with their album when the source folder holds exactly that album. Otherwise they stay where they are, and the preview says so.
  - Non-audio files are never deleted.
  - Emptied folders are removed only when they are empty, and only when the user opts in.
- **Albums stored as one file plus a `.cue` sheet** are left alone: Lyra does not split them.

### Execution

1. Preview: every move, grouped by album, with counts, collisions and skipped files.
2. A journal records each move as planned, moved and indexed.
3. A move within one filesystem is a single atomic rename. A move across filesystems copies, verifies the copy's hash, and only then deletes the source.
4. The library updates the track's path in place: track IDs, playlists, edits and artwork stay attached.
5. A file that is playing or prefetched is deferred, as for tag writes (§7). No scan runs during a batch.
6. Undo replays the journal backwards. It refuses for any file that changed since the move.
7. After a crash, the journal is reconciled before anything else runs.

## 9. Online enrichment

### Providers and constraints

| Provider                | Role                                                                | Proposed policy                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| ----------------------- | ------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| MusicBrainz             | Release/artist/recording IDs, titles, dates, tracklists and credits | First metadata adapter; read-only public API. Meaningful user-agent; at most one request/second across Lyra jobs; cache/deduplicate and handle throttling. API access and commercial use need review independently of data licensing. [API](https://musicbrainz.org/doc/MusicBrainz_API), [rate policy](https://musicbrainz.org/doc/MusicBrainz_API/Rate_Limiting)                                                                                 |
| Cover Art Archive       | Release-associated front-cover candidates                           | Fetch by selected release ID, support missing art and controlled redirects; show source. Artwork rights require separate treatment. [API](https://musicbrainz.org/doc/Cover_Art_Archive/API)                                                                                                                                                                                                                                                       |
| Discogs                 | Alternative edition/label/catalogue lookup                          | Deferred adapter. Developer portal returned 403 during research; verify current authentication and quotas during the spike. Its terms distinguish CC0 data from restricted images and impose cache/display freshness conditions, including a six-hour condition. Do not promise permanent tag/art persistence until a documented compliant policy exists. [API terms](https://support.discogs.com/hc/en-us/articles/360009334593-API-Terms-of-Use) |
| AcoustID                | Later fingerprint identification                                    | Deferred: API key, published three-request/second ceiling, and commercial-use arrangement when applicable; fingerprinting needs a separate analysis pipeline. [Web service](https://acoustid.org/webservice)                                                                                                                                                                                                                                       |
| Editorial/bio providers | Reviews, artist biographies, artist photos                          | Excluded until a suitable service and content rights are selected                                                                                                                                                                                                                                                                                                                                                                                  |

MusicBrainz distinguishes CC0 core data from supplementary data with different reuse terms, and cover art is separate. Maintain a field/content licence map and attribution rather than labelling every fetched value or image CC0. Limit the first provider implementation to reviewed fields. Recheck service terms at implementation/release time. [MusicBrainz data licensing](https://musicbrainz.org/doc/About/Data_License). Cover Art Archive explicitly says images remain copyrighted by their respective owners; archive availability is not a blanket redistribution licence. [Cover Art Archive](https://coverartarchive.org/).

### Matching and automation

Use embedded provider IDs first, then barcode/catalogue clues, then album/artist plus disc/track counts and durations. Rank candidates using explainable evidence. Do not claim a statistically calibrated confidence percentage without a labelled evaluation set. Distinguish release from release-group and recording from release-track IDs; bonus tracks, hidden tracks, multidisc sets and remasters require explicit alignment.

The initial online action is automatic retrieval and ranking followed by user review. A later **Enrich new albums automatically** setting may schedule proposals only after the user enables online access. A separate **Apply high-confidence missing fields** policy may fill reviewed empty fields; it cannot replace existing tags, write files, merge editions or replace chosen artwork without an explicit policy. Automatic file writing is excluded from the initial release.

### Network and security boundary

Online access is off by default. Enabling it discloses that providers see the public IP and submitted titles/artists, IDs, track counts/durations and other selected lookup clues. Never send local paths, usernames, whole audio files or logs. Fingerprints are a separate future opt-in. Playback and offline editing work without accounts or connectivity.

HTTP runs in bounded, cancellable Rust workers with timeouts, response/decompressed-size limits and provider-specific backoff (`Retry-After`, 429/503). No network requests at ordinary startup/scan while disabled. Provider tokens, if introduced, use OS credential storage and are redacted from logs. Requests cannot be arbitrary URLs supplied by the webview or tags. Permit only documented HTTPS providers/redirect hosts, rejecting local/private destinations and credentials; Cover Art Archive redirects require an explicit, tested host policy.

Artwork is downloaded to Lyra storage and served through the existing restricted artwork mechanism. Do not expose remote images in the webview or broadly relax CSP. Decode supported raster types with byte, dimension and pixel limits; reject executable/vector/HTML content. Proposed initial limits: existing 16 MiB picture cap, 32 MiB decoded tag/response ceiling where applicable, 1024-byte text-field cap unless a field explicitly allows more, and 40 megapixels for image decoding. Verify these budgets in the spike; a larger compressed tag must not trigger unbounded allocation. Current tag limits are a baseline, not proof that every image-decode allocation is already bounded.

Disabling online access cancels outstanding requests and prevents new work. Accepted metadata and user art remain available offline where provider terms permit; disposable response caches have separate deletion and retention controls.

## 10. Architecture and IPC contracts

Prefer small cohesive modules/crates: a pure typed metadata model and resolver; container read/write adapters; a provider layer; and a library-owned job coordinator for persistence/revisions. Tentative names are `metadata-core`, `metadata-files` and `metadata-online`; only split into crates when boundaries justify them. Neither UNDA nor `AudioSink` depends on network or tag writing.

Flow: Svelte editor → scoped Tauri command → metadata worker → revisioned SQLite update or prepared file operation → targeted library change event. Provider candidates use the same validated edit-preview contract as manual edits.

Proposed command groups, not final names:

| Command                                      | Input / output intent                                                                              |
| -------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `get_metadata`                               | Entity IDs → layers, effective fields, capabilities, revision                                      |
| `preview_metadata_edit`                      | IDs + expected revisions + set/clear/reset operations → immutable preview ID and affected entities |
| `commit_metadata_edit`                       | Preview ID → local edit transaction ID/new revisions                                               |
| `search_metadata` / `get_metadata_candidate` | Selected IDs/provider/query constraints → cancellable job/candidate IDs                            |
| `prepare_tag_write` / `commit_tag_write`     | IDs/fields/revisions → preview, then explicit recoverable file job                                 |
| `get_metadata_job` / `cancel_metadata_job`   | Job ID → per-item progress/status                                                                  |
| `undo_metadata_edit` / `restore_tag_backup`  | Transaction/backup ID + revisions → preview/recovery result                                        |

Revalidate preview revisions at commit; previews are bounded, expiring and bound to selected entities/settings. Never accept arbitrary write paths. Rust resolves library IDs to allowed files and exposes per-format capability/reason codes. New Tauri commands receive narrowly scoped permissions. Error codes include `metadata_conflict`, `unsupported_tag_write`, `file_in_use`, `read_only`, `insufficient_space`, `provider_unavailable`, `ambiguous_match`, `verification_failed` and `recovery_required`. UI must distinguish a failed file write from an already-saved library edit.

Scan, edit and write jobs serialize mutations per file/entity, not with a global library lock held across network or filesystem work. Different workers use separate SQLite connections with short transactions. Playback source leases cover current/prefetched files; generation/revision checks prevent stale rescan results overwriting a just-committed write. Library-to-UI notifications include affected IDs/revisions so a large collection need not reload in full for each edited track.

## 11. Acceptance criteria

| ID   | Scenario                                             | Required result                                                                                                      |
| ---- | ---------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| MT01 | Edit album title/artist, restart and rescan          | Correct selected release; edits persist; other editions and playlist IDs unchanged                                   |
| MT02 | Multi-edit mixed fields                              | Only requested fields change; set/clear/reset/list rules are deterministic                                           |
| MT03 | External tagger changes a pending target             | Stale preview/write refused; external changes preserved                                                              |
| MT04 | Select local/downloaded cover                        | Correct source/provenance; thumbnails invalidate; durable selection survives cache eviction                          |
| MT05 | FLAC/DSF tag round-trip                              | Requested fields correct; unknown content retained; audio payload and fixture sample/transport comparisons identical |
| MT06 | Unsupported WAV/DFF variant                          | Library edit works; file-write action gives explicit capability reason                                               |
| MT07 | Crash/disk full/permission error in each write phase | Original or verified replacement plus recoverable backup; no unreconciled success claim                              |
| MT08 | Batch partial failure/cancellation                   | Accurate per-file outcome; unaffected files and identities intact                                                    |
| MT09 | Undo and restore after external changes              | Local undo works with revisions; stale full-file restore is refused                                                  |
| MT10 | Lookup ambiguous multidisc/reissue                   | Candidate/track review required; no silent edition merge or overwrite                                                |
| MT11 | Network off, throttled, timeout, missing art         | Offline playback/editor work; bounded retry/cancellation; useful UI status                                           |
| MT12 | Metadata job + scan during PCM/native/DoP playback   | No metadata-caused writer blocking; benchmark records UI timing, disk load and underruns                             |
| MT13 | Oversized tags/images, malicious redirects/paths     | Bounded rejection; no out-of-root writes, remote webview access or private-network fetches                           |
| MT14 | Missing drive and folder/root consolidation          | User edits, playlist membership and stable IDs retained                                                              |
| MT15 | Database migration/backup/restore                    | Existing library preserved; WAL-safe backup restores edits, links, jobs and selected artwork                         |
| MT16 | Default offline/automatic policies                   | No startup/scan network while disabled; automation changes only fields explicitly covered by policy                  |
| MT17 | Organize preview, then apply                         | The result matches the preview exactly; covers and cue/log files follow their album; nothing outside library folders |
| MT18 | Organize with hostile names on ext4 and exFAT        | Illegal characters, reserved names, leading dots, 255-byte limits and case-only renames all produce valid names      |
| MT19 | Organize collision, crash, undo, cross-device move   | No overwrite; journal recovers mid-batch; undo restores and refuses changed files; cross-device copies are verified  |
| MT20 | Organize while playing; playlists afterwards         | Playing and prefetched files deferred; track IDs, playlists, edits and artwork unchanged                             |

Use synthetic fixtures, recorded synthetic provider responses and mock HTTP in CI. Live service checks and compatibility reads with independent taggers are separate manual reports. Follow all project Rust/frontend quality gates during implementation; no milestone is complete on mocks alone when physical playback is part of acceptance.
