# Metadata: MT0 decision record

Date: 22 October 2026. Phase MT0 of the [metadata plan](METADATA_IMPLEMENTATION_PLAN.md). Nothing here changes the app yet: no writer is exposed, and no dependency is added until the phase that needs it.

## Questions

1. Which code writes tags into FLAC and DSF files without changing anything else, and which formats stay library-edit-only for now?
2. How does Lyra handle unusual text (accents in two Unicode forms, emoji, right-to-left scripts, invisible characters, old encodings) without losing or garbling it?
3. How does poorly tagged music become a well-structured library?

## Method

A throwaway Cargo project outside the repository (not committed) tested the candidates on Lyra's fixtures:

- **FLAC:** `s16_44100.flac` with a seek table, ten Vorbis comments (a two-value `ARTIST`, `REPLAYGAIN_TRACK_GAIN`, `MUSICBRAINZ_ALBUMID`, a custom key, and a lowercase duplicate `title`), two pictures (front and back), an APPLICATION block Lyra does not know, and 4 KiB of padding. Each writer changed `TITLE` and `ALBUM` only.
- **Text:** the same file with values in decomposed and composed Unicode (`Beyonce` + combining accent, `Beyoncé`), an emoji sequence joined with zero-width joiners, a zero-width space, a byte-order mark, Hebrew, Japanese kanji, a newline, `=` inside a value, a NUL inside a value, and mojibake (`BeyoncÃ©`). Then three malformed entries a careless tagger leaves behind: a Latin-1 value (`Björk` as one byte `0xF6`), a key with a control character, and an entry without `=`.
- **DSF:** `dsd64_stereo.dsf` was tagged, retagged with a 300 KB cover, then retagged small again. Each result was read back with Lyra's own `format-dsd` reader.
- **ID3:** a v2.3 tag with TXXX, PRIV and TCOM frames was read, edited and written again; every text sample was round-tripped through ID3 v2.3 and v2.4.

Checks: SHA-256 of the FLAC audio frames and of the DSD payload as the DAC would receive it; `flac -t`, which decodes the audio and compares it with the MD5 in STREAMINFO; the raw block, comment and frame lists before and after.

## Results

| Candidate                  | Licence           | New crates                                      | Audio     | What else changed                                                                                                                                  |
| -------------------------- | ----------------- | ----------------------------------------------- | --------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `lofty` 0.25.4 (FLAC)      | MIT OR Apache-2.0 | about 12 (`flate2`, `ogg_pager`, proc macros …) | identical | Moves VORBIS_COMMENT and PICTURE blocks after the padding. Edited keys go to the end of the comment list; other comments and their order are kept. |
| `metaflac` 0.2.8 (FLAC)    | MIT               | 2 (`byteorder`, `hex`)                          | identical | Block order kept, but comments are held in a hash map: their order is lost, keys are upper-cased, and `title` merges into `TITLE`.                 |
| `id3` 1.17.2 (in the tree) | MIT               | none                                            | n/a       | Unknown frames and the v2.3 version are kept. The edited frame moves to the end.                                                                   |
| `id3` + own DSF patch      | MIT               | none                                            | identical | Only the ID3 tag at the end, the total size and the metadata pointer change. Growing and shrinking the tag both worked.                            |

Both FLAC writers pass `flac -t`. Neither uses the existing padding: each rewrites the whole file.

Text, `lofty`:

- Every valid UTF-8 value came back byte for byte. Decomposed text stays decomposed: nothing is normalized behind Lyra's back.
- In the default mode, the three malformed entries are **dropped without a word** on the next save. Strict mode refuses the file for the Latin-1 value and the bad key, but still drops the entry without `=`.
- A vendor string that is not UTF-8 is replaced by an empty one.

Text, `id3`: v2.4 round-trips all eleven samples. v2.3 turns a NUL inside a value into `/`, because v2.3 has no proper multi-value separator.

## Decisions

### Writers

1. **`lofty` writes FLAC**, and later the formats Lyra may add (MP3, AIFF, ALAC/M4A, WavPack, Opus). It is actively maintained, and one API covers them all. Moving blocks behind the padding is allowed: [RFC 9639](https://www.rfc-editor.org/rfc/rfc9639) only requires STREAMINFO first. The dependency is added in MT5a, with its licence and lockfile inventory recorded then.
2. **`lofty` never writes unchecked.** Its silent drops make two guards mandatory:
   - **Before:** Lyra reads the raw comments itself. If any entry cannot be represented (bad encoding, bad key, no `=`), the file is not written. The user is told why, and offered the encoding repair below as an explicit proposal.
   - **After**, on the temporary copy, before it replaces the file:
     - every raw comment the edit did not target is still present, byte for byte;
     - every non-tag block is still present, byte for byte (its position may change);
     - the audio frames hash the same;
     - the file reads back with the requested values.

   Any difference discards the copy.

3. **DSF and DFF:** `lofty` does not read DSD containers. The `id3` crate already in the tree serializes the tag, and Lyra patches the container itself. Everything before the metadata pointer (or the end of the data chunk) stays, the tag is appended, and the size and pointer are updated. The payload is verified through `format-dsd`. The existing ID3 version is kept.
4. **WAV and DFF stay library-edit-only** until each has its own preservation tests:
   - WAV: the RIFF INFO and ID3 policy, unknown chunks such as `bext` and `iXML`, and RF64.
   - DFF: the `ID3 ` chunk convention.

### Special characters

5. **The file layer keeps what the file says.**
   - Valid text is stored exactly: no normalization, no trimming beyond the ends, no length cap.
   - Today's 1024-byte cap applies only to what the library displays. A capped value is marked, so a write can never truncate it.
   - Bytes that are not valid text are kept as bytes, with an "unreadable text" flag.
6. **Matching uses a separate key**, so the same artist is one entry, but the name is shown as tagged. The key is built as:
   - Unicode NFC, so `Beyonce` + accent equals `Beyoncé`;
   - full Unicode lower-casing, so `BEYONCÉ` matches too (today's SQLite `NOCASE` only folds ASCII);
   - collapsed whitespace;
   - invisible format characters removed (zero-width space, byte-order mark, direction marks).

   This needs `unicode-normalization` (MIT/Apache, no dependencies), added in MT1.

7. **Values are never split on `/`, `&`, `,` or `;` automatically.** "AC/DC" and "Simon & Garfunkel" are single names. A NUL between values is a separator, as in ID3 v2.4 and Vorbis multi-values. A multi-value written to an ID3 v2.3 tag is flagged, because v2.3 joins it with `/`.
8. **Writing is stricter than reading.**
   - Vorbis keys must be ASCII from `0x20` to `0x7D`, without `=`.
   - Values must be valid UTF-8 without control characters (newline and tab are allowed in comments only).
   - Lyra writes NFC unless the user kept the original form.
9. **Display:** names in rows and headers are isolated for direction (`dir="auto"`), so a Hebrew or Arabic title does not reorder the row around it.

### From messy tags to a structured library

10. **Cleanup is a set of rules that propose; they never apply silently.** Each rule is a pure, tested function from raw file values to a proposed change, stored in the edits layer (MT1), never in the file. Results are grouped by rule with counts and examples, and accepted per rule or per item, with undo:

| Rule             | Example                                                                                                                                                        |
| ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Text hygiene     | Leading or trailing spaces, double spaces, invisible characters, NFC                                                                                           |
| Encoding repair  | Latin-1 or Windows-1252 bytes → UTF-8; mojibake `BeyoncÃ©` → `Beyoncé` (proposed only when re-encoding the text as Latin-1 gives valid UTF-8)                  |
| Numbers          | `3/12` → track 3 of 12; `1/2` → disc 1 of 2; `03` → 3; `Album (Disc 2)` → album `Album`, disc 2                                                                |
| Dates            | `13.04.1973`, `1973/04/13`, `April 1973` → ISO date, keeping year-only or year-month precision; an ambiguous `04/05/1973` is left alone                        |
| Album artist     | Missing: the track artist when every track agrees; `Various Artists` when the album is a compilation                                                           |
| Featured artists | `Song (feat. X)`, `A ft. B` → title `Song`, artist A, featuring X. Only `feat.`, `ft.` and `featuring`; never `&` or `,`                                       |
| Capitals         | ALL CAPS or all lowercase → title case, only for text entirely in one case, with exceptions for short words; never touches mixed-case names such as `deadmau5` |
| Duplicates       | Artists that match by key (accents, case, spaces, a leading `The`) → merge into one, with the spelling chosen by the user                                      |
| Genres           | `(17)` → `Rock`, case variants, `Rock; Pop` → two genres                                                                                                       |
| Sort names       | `The Beatles` → sort as `Beatles, The` (sort field only)                                                                                                       |

A test corpus of badly tagged fixtures covers every rule, including names a rule must leave alone. Online lookups (MT4) are a separate, later source of proposals.

**As built (MT2 block 1).**

- **Chaining.** Rules run in the table's order, each on what the ones before left. A field gets one proposal, which names every rule that changed it. Accepting all proposals and running the rules again proposes nothing; a test checks this.
- **Encoding repair.** Only mojibake for now. Raw Latin-1 bytes need the raw comment reader that comes with MT5a.
- **Featured artists.** There is no separate featuring field, so the guest joins the artist credit, as MusicBrainz writes it: `Song (feat. X)` by A becomes title `Song`, artist `A feat. X`.
- **Album artist.** Guests do not make an album a compilation: tracks agree on their main artist, the part before `feat.`.
- **Capitals.** Only text of two words or more. A single word in one case is usually meant (`ABBA`, `MGMT`, `deadmau5`). Artist names are changed only from ALL CAPS, because lower case is a style too (`k.d. lang`).
- **Duplicates.** The most used spelling is proposed; the editor (MT3) lets the user pick another. An edited spelling names the artist, since spellings that share a matching key are one artist.

**As built (MT2 block 2): edit service.**

- **Revisions.** One counter rises on every change. A track takes its value when its edits or its file change, a release when its edits or its tracks change. Database triggers set them, so no writer can forget (migration 9).
- **Preview, then commit.** `preview_edits` lists each field's shown values before and after, the tracks and releases it rewrites, and the revisions it saw. `commit_edits` checks the changes again and plans them in its write transaction. If one of those revisions has moved, it saves nothing (`Stale`). `save_edits` still saves without a preview.
- **Setting a field to its file value is an edit.** It pins the value, so a later change to the file does not change what is shown. A preview lists it.
- **Undo** refuses a step whose edits are no longer what it saved (`UndoConflict`) and changes nothing. Every save is a step, so this guards against other writers and against MT5 file writes. `reset_edits` drops all edits of some tracks or releases, as one step.
- **Validation** has typed problems: wrong scope, unknown target, empty, control characters, over 1024 bytes, several values where one is allowed, duplicates, numbers written as a scan shows them (`3`, not `03`), dates that exist, and MusicBrainz IDs as lowercase UUIDs. Artist, album artist, composer, genre and the two artist IDs may have several values.
- **Source preference.** A fixed order, not a setting: manual over provider over rule. A weaker origin's change to a stronger one's field is held, and the preview names the stronger origin. Cleanup does not propose over manual or provider edits. A setting can come later if real use asks for one.
- **Release edits follow their tracks.** A release a scan empties hands its edits to the releases created for its tracks (two discs merged into one album). Older releases go first; a field already edited stays as it is. The edits' history moves too, so their steps still undo. A release that already existed keeps its own edits. When a release is split, the part that keeps the old ID keeps the edits, as before.

## Capability matrix

| Format | Library edits and cleanup (MT3) | File writes                       | Gate                                                                                                  |
| ------ | ------------------------------- | --------------------------------- | ----------------------------------------------------------------------------------------------------- |
| FLAC   | yes                             | MT5a, `lofty` with Lyra's guards  | Raw comment and block preservation, audio-frame hash, `flac -t` in a manual report                    |
| DSF    | yes                             | MT5b, `id3` + own container patch | Payload hash through `format-dsd`; grow, shrink, untagged, malformed pointer and truncated-file tests |
| WAV    | yes                             | later, `lofty`                    | INFO/ID3 policy, unknown-chunk preservation, RF64 refusal                                             |
| DFF    | yes                             | later, `id3` + container patch    | `ID3 ` chunk convention and third-party reads                                                         |

## Still open

- **Read by other taggers.** No independent tagger is installed here (exiftool, kid3, Mp3tag). A dated manual report should read Lyra-written FLAC and DSF files before MT5 ships.
- **Online sources.** The MusicBrainz and Cover Art Archive licensing map is deferred to MT4, which is when network access is decided.
- **10k-track baseline.** Deferred along with A07.
- **Scope change.** [SPECIFICATION.md](SPECIFICATION.md) still describes music folders as read-only. MT1–MT3 (edits, cleanup and the offline editor) do not change files, so they need no scope change. MT5 (file writes) does.
