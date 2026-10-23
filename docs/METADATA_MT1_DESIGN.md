# MT1 design: stable identities and layered values

Date: 22 October 2026. Phase MT1 of the [metadata plan](METADATA_IMPLEMENTATION_PLAN.md), following the [MT0 decisions](METADATA_DECISIONS.md). For review before any migration is written.

MT1 changes the library database so that later phases can edit, clean up, match and move music without losing anything. Nothing in it changes music files. It ships in four blocks, each one a working app with its own tests.

## Today

| Thing  | Identity                                          | Problem                                                                                                                 |
| ------ | ------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Track  | `id`, found again by `path`                       | A file moved or renamed outside Lyra is a new track: playlists lose it                                                  |
| Album  | unique `(title, artist_id)`, title `NOCASE`       | Two editions with the same name merge. A compilation without an album-artist tag splits into one album per track artist |
| Artist | unique `name COLLATE NOCASE`                      | `NOCASE` folds ASCII only: `BEYONCÉ` and `Beyoncé`, or the two Unicode forms of `é`, are separate artists               |
| Values | the cleaned, capped text in `tracks` and `albums` | No record of what the file said, no place for edits, and the 1024-byte display cap is the only copy                     |

## Block 1: matching keys and backup (migration 4) — done

- **Backup.** Before any migration runs, `VACUUM INTO 'library-v{N}-{date}.db'` writes a consistent copy next to the database. It is safe with WAL and needs no extra crate. The last three backups are kept.
- **Matching keys.** `key(text)` is NFC, then full Unicode lower-casing, then collapsed whitespace, with invisible format characters removed. Examples: `Beyonce` + combining accent, `BEYONCÉ` and `beyoncé` share one key, while `AC/DC` and `Simon & Garfunkel` keep theirs as they are. Adds `unicode-normalization` (MIT/Apache-2.0, no dependencies).
- **Artists** get a `key` column, unique, replacing `UNIQUE (name COLLATE NOCASE)`. Rows that now share a key merge: tracks and albums point at the row with the most tracks, and its spelling is shown.
- **Rebuilding a table.** SQLite cannot drop a constraint in place, so `artists` and `albums` are rebuilt. Foreign-key checks pause for the migration (`PRAGMA foreign_keys` cannot change inside a transaction), then `PRAGMA foreign_key_check` must come back empty before the commit. That step lives in the migration runner and is tested on a copy of a version 3 database.

## Block 2: release identity (migration 5) — done

An album row becomes a **release**: a stable `id` that edits, artwork and later matches attach to, plus a `grouping` string recording why its tracks belong together. The table keeps the name `albums`, so queries and the UI change little.

How a scanned track finds its release, first rule that applies:

1. **MusicBrainz album ID** in the tags: `mb:<id>`. Different IDs mean different editions, even with the same title.
2. **Same folder**: `dir:<release folder>|<album key>|<album-artist key>`. The release folder is the track's folder, or its parent when the folder name is a disc folder (`CD1`, `CD 2`, `Disc 1`, `Disk 2 - Live`, matched without case). So two copies of an album in different folders (a remaster, a hi-res download) stay separate, and `Album/CD1` with `Album/CD2` stay one.
3. **No album-artist tag**: the album-artist part is left out, so a compilation in one folder stays one release. After the scan, its shown album artist is the track artist when every track agrees, otherwise `Various Artists`. The compilation flag forces `Various Artists`. This changes only how the library groups tracks; the files and their tags are untouched.

The `grouping` string is evidence, not identity. When files move but stay together, the release follows them: if every track of a release now computes the same new grouping, the release's `grouping` is updated and its `id` stays. Only a real split or merge creates or removes a release.

**Disc numbers.** A track without a disc tag in a disc folder takes the folder's number, so `CD1/01` plays before `CD2/01`.

**Migration 5.** Existing albums may merge editions, and the database does not record each track's album-artist tag, so they cannot be split reliably in SQL. Migration 5 gives every album a placeholder grouping and marks every track for a re-read, as migration 3 did for genres; the next scan regroups them. Track IDs, and so playlists, are kept because tracks are matched by path. The same rule that lets a release follow its tracks keeps album IDs and artwork: a new release whose tracks all came from one old album, now empty, takes that album's place. Only a merged edition gets a new ID, for its second part. Tracks also gain their album-artist tag (`album_artist_id`, empty when untagged) and compilation flag, so the album artist is decided from the tracks after each scan.

## Block 3: path-independent tracks (migration 6) — done

A track keeps its `id` when its file moves or is renamed, whether by Lyra (MT6) or by another program.

- **Content identity**, stored per track as `audio_id`:
  - FLAC: the MD5 of the decoded audio from STREAMINFO, which Symphonia already reads;
  - WAV, DSF, DFF: size, sample rate, channels and frame count together.

  Reading a hash of the audio is avoided: it would read every file in full on the first scan.

- **During a scan**, once every folder is walked, tracks no longer found are compared with files new to the library. A new file whose `audio_id` matches exactly one missing track, and only one, is that track moved: its path and folder are updated, and its ID, playlists and edits stay. Several candidates, or none, fall back to today's behaviour, so a wrong match is never made.
- Tracks in unavailable folders (T01) are not candidates. Their files are not gone, only out of reach.
- A scan that is cancelled moves and removes nothing: it may not have reached where a missing track went.
- A moved track takes the new file's values, and the release follows it as in block 2, so a renamed album folder keeps its album ID and artwork too.
- Migration 6 adds `audio_id` and marks every track for a re-read to fill it in.

## Block 4: layered values (migration 7) — done

| Table          | Holds                                                                                                                         |
| -------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `file_values`  | What the file says, per track: field, position, exact text (or the raw bytes and an unreadable flag), replaced by each read   |
| `edits`        | The user's choices, per track or release: field, `set` with a value or `clear`, origin (manual, cleanup rule, provider), time |
| `edit_history` | Each saved change with its before and after, for undo                                                                         |

- **Shown value** = the edit, if there is one, otherwise the file value (§5 of the [specification](METADATA_SPECIFICATION.md)). The existing columns in `tracks` and `albums` hold the shown value, so browsing queries stay as they are, and they are rewritten whenever an edit or a read changes it.
- **A rescan** replaces `file_values` only. Edits survive, and an edit whose file value changed underneath it is flagged for review.
- **Fields** are a fixed list in Rust, not free text: title, artist, album, album artist, track, track total, disc, disc total, date, original date, genre, composer, compilation, sort fields and MusicBrainz IDs. New fields need a code change, which keeps the contract typed.

**As built.**

- Each field has a scope: track (title, artist, numbers, genre, composer, sort names, recording IDs) or release (album, album artist, dates, disc total, compilation, album sort names, release IDs). A release field's file value is that of its first track, in disc and track order, that has one.
- One module (`shown`) computes every shown column from the file values, the path and the edits, for scans, edits and undo alike, so clearing an edit shows exactly what a scan would.
- Edits never regroup tracks: releases are found by what the files say.
- File values over 64 KiB keep their start and are marked `cut`, so a later writer can refuse to write them back.
- Undo reverses the last saved step. The rest is left to MT2: checking that nothing changed since, field validation beyond the basic forms, and previews.
- Invalid bytes in tags are not yet kept as bytes, because Symphonia hands over decoded text. The raw comment reader that MT5a needs anyway will add that.
- A release's edits go when the release is removed. That happens when all its tracks leave and no other release takes its place.

## Out of MT1

- The edit service, cleanup rules and preview (MT2).
- The editor UI (MT3).
- Online matching (MT4).
- Backup and restore as a user action: the automatic pre-migration copy comes in block 1; export and import come with the editor, once there are edits worth exporting.

## Tests per block

| Block | Must show                                                                                                                                                             |
| ----- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1     | A version 3 database migrates with a backup written; NFD, NFC and upper-case spellings become one artist; `AC/DC` stays whole; foreign keys check clean               |
| 2     | Same-name editions in two folders stay apart; `CD1`/`CD2` folders join; an untagged compilation is one release under `Various Artists`; playlists survive the regroup |
| 3     | A file moved or renamed outside Lyra keeps its track ID and playlist entries; two identical candidates move nothing; unavailable folders are never matched            |
| 4     | An edit survives rescans and restarts; clearing it shows the file value again; a file change under an edit is flagged; undo restores the previous state               |
