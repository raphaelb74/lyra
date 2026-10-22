# Lyra design system — plan spec

Status: accepted 2026-10-21, delivered in M5 as six phases ("Design overhaul" in [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md)). Implements [DESIGN_Brief.md](DESIGN_Brief.md). Each phase's outcome may adjust the later ones.

## 1. Where we start

Baseline audit of `apps/desktop/src` (2026-10-21):

- **Tokens:** 21 colour tokens in `app.css`, with a green-tinted palette (`--accent #b4d9cb`). No tokens for type, spacing, radius, motion or elevation.
- **Hard-coded values:** about 62 font-size declarations using 12 distinct sizes (11 to 72 px), about 113 raw spacing values, and 4 radii (0, 6, 8, 10 px). One stray `rgb()` shadow in `AddToPlaylist.svelte`.
- **Duplicated styles:**
  - The track-row grid exists 4 times (TrackList, Playlists, AlbumView, QueueView).
  - The `h3` reset exists 6 times, and muted text is re-declared 9 times.
  - Ellipsis truncation appears 7 times.
  - The `♪` cover placeholder appears 4 times.
  - The verdict colours appear twice, and so does the dashed drop box.
- **Icons:** Unicode glyphs only (⏮ ▶ ❚❚ ■ ⏭ ♪ ← ↑ ↓ ✕ ♮). They render in whatever font the system falls back to, so their weight and alignment vary between distros.
- **Fonts:** `system-ui`. The look changes with the distro. Mint 22.3 here has none of the candidate typefaces installed.
- **Shell:**
  - The sidebar mixes navigation, device choice, volume and a "Rust connected / Check" box that only developers need.
  - "Now playing", "Queue" and "Files" are top-level views.
  - A white vertical strip renders between the sidebar and the main area (visible in the browser preview at 1280×800).
- **Accessibility gaps:**
  - Playback state is not announced: there is no live region in the player bar.
  - The seek slider has no `aria-valuetext`.
  - The Library tabs have no arrow keys and no `tabpanel`.
  - The playlist menu neither moves focus into itself nor returns it on Escape.
  - Inputs, selects and radios get no focus style.
  - The play button changes its `aria-label` and also sets `aria-pressed`.
  - Heading levels skip.
  - `aria-current` renders `"false"`.
- **Performance:**
  - The position is polled every 250 ms while playing.
  - The track list is capped at 500 rows, and there is no virtualization.
  - The album grid renders every album.
- **Runtime:** WebKitGTK 2.52.6. It supports `color-mix()`, `:has()`, container queries, `@layer`, `:focus-visible` and `-webkit-slider-*`, all present before 2.52. `backdrop-filter` is not needed by this spec and is avoided, because its performance on Linux is uncertain.

## 2. What the references teach

These are principles, not looks. Sources were gathered by research agents, and several vendor pages could not be fetched, so treat the details as indicative.

| Reference | Observation | What Lyra takes |
|---|---|---|
| Roon signal path | Colour-coded quality: purple "Lossless", blue "Enhanced" (processing the user asked for), green "High quality", yellow "Low quality" ([blog](https://blog.roonlabs.com/signal-path/)) | One quality indicator, explained on demand. Lyra stays stricter: it says only what the engine verified, and uses one lit state, not four colours. |
| Roon users | The most common complaints: shrinking artwork in Now Playing, "wasted space", cluttered credits | Artwork is the largest element in Now Playing. Credits stay minimal. |
| Audirvāna 3 | A redesign moved the sidebar to a top bar; users asked for the sidebar back because of the wasted space ([forum](https://community.audirvana.com/t/audirvana-studio-ui-issue-sidebar/46299)). Displayed bit depth confused people. | Keep a compact sidebar. Show the source bit depth in the short label and both source and output in the signal path, each labelled. |
| OxiTide | Rust engine with native toolkits. Sidebar with Albums, Tracks, Artists, Playlists. Play bar "names the format reaching the DAC". | The same honest output label, placed in the player bar. |
| Sonos | The 2024 removal of tabs was received as a regression. | Navigation stays predictable and always visible. |
| Linn Selekt, dCS Vivaldi | Monochrome or greyscale displays, sparse text, and one illuminated element (Linn's 100-line volume ring) | A monochrome UI with one point of light. |
| Naim | An LED ring shows volume only. The app offers a slider or buttons. | Each light means one thing. Volume can be set precisely, with the keyboard, by step and by dB. |
| Devialet | Carved from one block. Icons on the remote vanish in dim light. | Few materials, and enough contrast for low light: the secondary text floor is 4.5:1. |

## 3. Design concept: "one point of light"

Lyra reads like a black anodized component. It is graphite surfaces with fine, evenly weighted type and no decoration.

**The one memorable element** is the **signal lamp**: a 6 px dot beside the format label in the player bar. It is the only lit element in the interface.

| Lamp | Meaning | Source of truth |
|---|---|---|
| Lit (accent, soft 4 px halo) | Bit-perfect, verified by the engine | `track.bitPerfect.status === 'yes'` |
| Ring (outline only) | Playing, bit-perfect not verified | `'unverified'`, reason in tooltip and signal path |
| Amber | Not bit-perfect (conversion, volume below 0 dB on a non-USB path, …) | `'no'`, reason shown |
| Off (no lamp) | Nothing playing | `track === null` |

The lamp opens the signal path. Everything around it stays quiet: no other glow, no gradients and no coloured icons.

Spectral colour is reserved for UNDA's own identity. It may later appear only as the 1 px connecting line in the signal-path view. It is not part of this work.

## 4. Tokens

All tokens live in `src/styles/tokens.css` under `@layer tokens`. Components use only tokens, and a check (§9) rejects raw colours in `.svelte` files.

### Colour

The brief's palette is refined into a complete set. Contrast ratios are measured against `--bg`.

| Token | Value | Use | Contrast |
|---|---|---|---|
| `--bg` | `#0D0F12` | Window background | — |
| `--surface` | `#15181C` | Sidebar, player bar | — |
| `--surface-raised` | `#1C2025` | Popovers, menus | — |
| `--surface-hover` | `#22272D` | Row and button hover | — |
| `--surface-selected` | `#272D35` | Selected row, current view | — |
| `--line` | `#2A2F36` | Hairline separators | — |
| `--line-strong` | `#3A4049` | Input borders, slider track | — |
| `--text` | `#F1F2F3` | Primary | 17.1 |
| `--text-secondary` | `#9A9FA7` | Artist, supporting text | 7.2 |
| `--text-muted` | `#838A94` | Low-priority metadata (the brief's `#666D76` measures 3.7 and fails 4.5). It measures 4.7 on `--surface-raised`. On hover or selected rows it drops below 4.5, so muted text there switches to `--text-secondary`. | 5.5 |
| `--accent` | `#B8C7D9` | Focus ring, lit lamp, slider fill on hover, current item | 11.2 |
| `--warning` | `#E0B07A` | Not bit-perfect, clipping, underruns | 9.7 |
| `--danger` | `#E8958A` | Destructive actions, errors | 8.3 |
| `--art-empty` | `#1F242A` | Artwork placeholder | — |

Pure black and white are not used. The surfaces step up in lightness by about 3–4 % each. Depth comes from tone, and shadows appear only on popovers: `--shadow-pop: 0 12px 32px rgb(0 0 0 / 45%)`, with a 1 px `--line` border.

Only a dark theme is planned. A light theme is out of scope, and the brief does not ask for one.

### Typography

- **Typeface:** Manrope variable (weights 200–800, OFL). The shortlist was Manrope or Plus Jakarta Sans, and both were rendered in Lyra's palette (specimen, 2026-10-21).
  - Manrope's semi-condensed, evenly weighted shapes read as engineered rather than friendly. That fits the "machined component" brief.
  - The narrower letters keep dense track rows and the player bar uncrowded at 13–14 px.
  - Tabular figures were measured: with `tnum`, every digit is the same width (Manrope 0.62 em, Plus Jakarta Sans 0.60 em), and `:` and `.` stay narrow. Times like `11:33` therefore don't shift as they tick.
  - The latin + latin-ext files are about 40 KB, against 49 KB for Plus Jakarta Sans.
  - Manrope has no italic. Lyra doesn't use one, and any emphasis comes from weight.
  - Plus Jakarta Sans remains the fallback choice: it is wider, more geometric and has an italic.
- **Monospace:** none bundled. The few technical blocks (Diagnostics `hw_params`) use `--font-mono` (`ui-monospace`, then DejaVu Sans Mono).
- **Numerals:** times, rates, bit depths, dB values, counts and track numbers use `font-variant-numeric: tabular-nums` through a `.num` utility and in the slider and row styles.
- **Case:** sentence case everywhere. No all-caps labels and no letter-spaced eyebrows. The wordmark is the one exception.

Each row is a pair of tokens, `--type-*` for the size and `--leading-*` for the line height. Weights are `--weight-regular`, `--weight-medium` and `--weight-strong`.

| Token | Size / line height | Weight | Use |
|---|---|---|---|
| `--type-caption` | 12 / 16 | 450 | Technical metadata, timestamps |
| `--type-small` | 13 / 18 | 450 | Secondary text, table cells |
| `--type-body` | 14 / 20 | 450 | Default UI text, row titles |
| `--type-label` | 14 / 20 | 550 | Buttons, current row, section labels |
| `--type-title` | 18 / 24 | 550 | View titles ("Albums", playlist name) |
| `--type-heading` | 26 / 32 | 600 | Album and artist name in detail views |
| `--type-display` | clamp(28px, 3.2vw, 40px) / 1.15 | 600, letter-spacing −0.01em | Track title in Now Playing |

That is 7 sizes, down from 12. Only two weights are visible in practice: 450 and 550 (600 for large type).

### Space, size, radius, motion

- **Space:** a 4 px grid. `--space-1` to `--space-8` are 4, 8, 12, 16, 24, 32, 48 and 64 px.
- **Hit targets:** at least 32 × 32 px for icon buttons and 28 px for row actions. The primary play/pause button is 40 px.
- **Radius**, by hierarchy rather than one value for everything:
  - `--radius-s` 4 px: artwork, inputs, rows, small buttons.
  - `--radius-m` 6 px: buttons, popovers.
  - `--radius-round` 50 %: the play button, the lamp, the slider thumb.
- **Line:** 1 px.
- **Motion:**
  - `--dur-fast` 120 ms: hover and focus.
  - `--dur-base` 180 ms: state changes.
  - `--dur-panel` 220 ms: popovers and the signal-path panel.
  - Easing: `--ease cubic-bezier(0.2, 0, 0, 1)`.
  - Under `prefers-reduced-motion: reduce`, all durations become 0 ms.
  - No entrance animations on views.
- **Focus:** a 2 px `--accent` outline with a 2 px offset, on every focusable element, keyboard focus only (`:focus-visible`).

### Assets: no new npm dependencies

- **Fonts:** vendor the two Manrope variable files (`manrope-latin-wght-normal.woff2`, 25 KB, and `manrope-latin-ext-wght-normal.woff2`, 15 KB) into `src/assets/fonts/` together with `OFL.txt`. Load them with two `@font-face` rules split by `unicode-range`. The app never fetches fonts at runtime.
- **Icons:** vendor about 20 Lucide SVGs (ISC licence; notice kept in `src/assets/icons/LICENSE`) into one `Icon.svelte`. It takes `name` and `size` and uses one stroke width (1.5 px at 20 px).
  - Set: play, pause, skip-back, skip-forward, square (stop), list-music (queue), volume, volume-x, speaker (device), search, plus, chevron-left, chevron-down, x, arrow-up, arrow-down, settings, library, disc, user, activity (diagnostics).
  - `@lucide/svelte` 1.61 alone is 7.9 MB installed for icons we would mostly not use.

## 5. Layout and navigation

```
┌──────────────┬───────────────────────────────────────────────────────┐
│ Lyra         │ Albums                                   [⌕ Search ]  │
│              │                                                       │
│ ◫ Albums     │  ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐            │
│ ◯ Artists    │  │art │ │art │ │art │ │art │ │art │ │art │            │
│ ≡ Tracks     │  └────┘ └────┘ └────┘ └────┘ └────┘ └────┘            │
│ ♫ Playlists  │  Title   Title   …                                    │
│              │  Artist  Artist                                       │
│              │                                                       │
│              │                                                       │
│              │                                                       │
│ ⚙ Settings   │                                                       │
├──────────────┴───────────────────────────────────────────────────────┤
│ ┌──┐ Track title             ⏮  ( ▶ )  ⏭        ● PCM · 24-bit / 96 kHz│
│ └──┘ Artist — Album     1:23 ━━━━━━━━━───────── 4:56   iFi SE ▾  ─●── ☰│
└──────────────────────────────────────────────────────────────────────┘
```

**Sidebar (208 px; 64 px icon rail below 1000 px window width):**
- Albums, Artists, Tracks and Playlists become direct items. The Library tabs go away, which removes a level and the faulty tab pattern.
- The queue is not a sidebar item: it is a pane on the right (see below).
- Settings is pinned to the bottom.
- The sidebar no longer carries the device, volume or backend status.

**The views that change:**
- **Now Playing:** the player bar's artwork or title opens it. Escape or the back button returns to the previous view. It is not a sidebar item.
- **Music folders** (add, rescan, remove) move to Settings → Library. An empty library shows one action, "Add a music folder", where the albums would appear.
- **Files:** removed (2026-10-21). Lyra plays from the library only; a dropped folder joins it, and a dropped file gets a hint to drop its folder.
- **Diagnostics:** Settings → Audio → Diagnostics, for the selected device.
- **Backend connection:** shown only when it fails, as a top alert with "Retry". The smoke test's selectors move with it.
- **Home:** no Home view. There is no backend for recents or favourites (brief §11: no non-functional screens).

**Content:**
- Left-aligned, with a 32 px gutter and a maximum width of 1280 px for lists. The album grid fills the width.
- The view title sits on the content's left edge, with view actions (search, Play) on the right of the same line.

## 6. Components

The components are kept small, and each exists because it is reused.

| Component | Replaces | Notes |
|---|---|---|
| `Icon.svelte` | Unicode glyphs | Inline SVG, `aria-hidden`; the parent button carries the label |
| Button styles (CSS) | global `button`, `.small`, `.back`, `.danger` | Variants:<br>• `primary`: filled `--text` on `--bg` text, used once per view, e.g. "Play album".<br>• `secondary`: `--line-strong` border.<br>• `tertiary`: text only.<br>• `icon`: 32 px square, hover `--surface-hover`.<br>• `danger`: modifier. |
| `Slider.svelte` | seek + volume `input[type=range]` | Native range input, restyled, so keyboard and AT support come free. Track 2 px, 4 px on hover or drag. Filled part `--text`, `--accent` while active. The thumb is hidden until hover, focus or drag. `aria-valuetext` ("1:23 of 4:56", "−12.5 dB"). It holds the value while dragging so updates don't jitter. |
| `Artwork.svelte` | 4 placeholders | Fixed aspect ratio, lazy `loading="lazy"`, `--art-empty` with a centred disc icon, `--radius-s`, no border |
| Track rows (CSS) | 4 duplicate grids | `.rows` / `.row` in `styles/components.css`:<br>• 40 px rows, hairline only between groups (discs).<br>• Columns: number (tnum, right-aligned), title + artist, format, duration (tnum, right-aligned).<br>• Hover `--surface-hover`, selected `--surface-selected`, playing row `--accent` text plus a small level icon.<br>• Row actions appear on hover or focus. |
| `Popover.svelte` | AddToPlaylist menu | Anchored below or above its trigger, `--surface-raised`, `--shadow-pop`. Focus moves to the first item, arrow keys move between items, Escape or an outside click closes it and returns focus. Also used by the device picker. |
| `FormatIndicator.svelte` | `outputLabel` + verdict text | Lamp plus a short label. A button that opens the signal path. Used in the player bar and Now Playing. |

There is no dialog component: removing a folder or playlist keeps the two-click inline confirmation, which the brief prefers for simple operations. There is no state library and no router.

## 7. Screens

### Player bar (72 px)

- **Left:** 48 px artwork, title (`--type-label`), and "Artist — Album" (`--type-small`, secondary). Clicking it opens Now Playing.
- **Centre:**
  - Previous, play/pause (40 px round, `--text` filled) and next.
  - Under them, the seek slider with tabular times on both sides.
  - Stop stays visible: a smaller 32 px tertiary icon button after Next. It is also Ctrl+.
- **Right:**
  - Format indicator.
  - Device picker: the card name and a chevron, opening a popover that lists devices with "USB" and the active output.
  - Volume slider (hidden when the device has no hardware volume), with its value on hover as "−12.5 dB" or "64 %".
  - Queue button: opens or closes the queue pane (also Ctrl+5); accent and a dot while open.
- **Shuffle and repeat** flank previous and next; on, they turn accent with a dot. Repeat cycles off, all, one.
- **Time:** the length after the seek bar switches to time left on click, remembered between launches.
- **Below 1100 px:** "Artist — Album" collapses to the artist, and the volume shrinks to an icon button with a popover slider.
- **Announcements:** a visually hidden `aria-live="polite"` region announces "Playing *title*", "Paused" and errors. It does not announce position ticks.

### Format label (short, from backend state only)

| Transport | Label | Example |
|---|---|---|
| `pcm` | `PCM · {source bits}-bit / {rate} kHz` | `PCM · 24-bit / 96 kHz` |
| `native` | `DSD{n} · Native` | `DSD256 · Native` |
| `dop` | `DSD{n} · DoP` | `DSD128 · DoP` |
| `converted` | `DSD{n} → PCM {rate} kHz` | `DSD64 → PCM 88.2 kHz` (amber lamp) |

The label comes from `track.output` and the source format, never from the file's library metadata. Before the device is negotiated, it shows "Opening…".

### Signal path (popover from the lamp; full section in Now Playing)

These are vertical stages joined by a hairline. Each stage states a fact the backend reports, and none is invented.

1. **Source:** FLAC · 24-bit · 96 kHz · 2 channels. The file name is muted.
2. **Processing:** "None", or exactly what happened:
   - "Packed into DoP markers", which is lossless for the DAC.
   - "Converted to PCM: low-pass and decimation, +3 dB gain", which is not bit-perfect.
3. **Volume:** "DAC hardware volume −12.5 dB", or "Software untouched; hardware at 0 dB". The wording follows `VolumeInfo` and the verdict.
4. **Output:** ALSA `hw:CARD=SE,DEV=0` · S32_LE · 96 kHz · 2 channels.
5. **Device:** iFi USB Audio SE (USB). "Kernel confirms 96000 Hz, S32_LE" appears only when Diagnostics `hw_params` matches the output.
6. **Verdict** line: Bit-perfect / Not bit-perfect: reason / Not verified: reason.

**Engine work:** steps 2 and 3 need facts the UI must not infer. The UNDA engine (`player-core`, renamed `unda` in M5) builds a `SignalPath { stages }` from what it actually did and reports it with the status. `ipc.rs` only maps it, and the frontend only renders it. Do the rename before phase 4, so the type lands in `unda`. This is the one IPC change in this spec.

### Now Playing

- Artwork as large as the height allows, up to 560 px square, on the left.
- On the right:
  - Title (`--type-display`), artist, album and year.
  - The format indicator.
  - The full signal path, collapsed behind "Signal path" for users who don't want it.
- Transport controls are not repeated: the player bar stays.
- No blurred-artwork background: it is decoration, and `backdrop-filter` and large blurs are costly on WebKitGTK.

### Library

- **Albums:**
  - Grid of `minmax(168px, 1fr)` with a 24 px gap.
  - Title on one line, artist muted. The format badge ("DSD128", "96 kHz") is shown only for hi-res or DSD, in muted caption type.
  - Rendering uses `content-visibility: auto` per tile. Real virtualization is added only if 10k tracks / about 1k albums measure slow (A07).
- **Album detail:**
  - Header: 200 px artwork, title (`--type-heading`), artist, and year, track count, total time and format.
  - Primary "Play album", plus Add to playlist.
  - Rows below, grouped by disc.
- **Artists:** a single-column list (name, album and track counts). Opening an artist gives a grid of their albums.
- **Tracks:**
  - The search field sits in the view header, and `/` or Ctrl+F focuses it.
  - The 500-row cap becomes windowed rendering: a fixed 40 px row height makes a small hand-written virtual list possible, about 60 lines and no dependency.
  - This is done only if A07 shows the cap is a problem. Otherwise the "showing 500 of N, refine your search" note stays.
- **Playlists:** the shared rows. Up, down and remove appear as row actions on hover or focus, and Alt+↑ / Alt+↓ moves the focused row.
- **Queue pane:** 340 px on the right, sliding in over `--dur-panel` and pushing the view; open state remembered. "Now playing" with its cover, then "Up next" in play order (wrapping when repeating all), with covers. Row actions on hover or focus: Play next and Remove; the same plus Play in the right-click menu. Clear keeps only the playing track. Albums, artists and tracks get "Play next" and "Add to queue" in their right-click menus; when nothing plays, they start playback. Later: drag to reorder.

### Settings

One compact page of titled groups (built 2026-10-21). Each group is one panel of rows: the setting's name and a single line describing the current choice on the left, a segmented control on the right. There are no boxed radio lists and no paragraphs of explanation.

Groups, in order:

- **Audio:**
  - Device list with the active output and the "Ask the driver" capabilities.
  - DSD transport, DSD conversion and gain, pause before a format change.
  - Diagnostics for the selected device.
- **Library:** music folders, rescan.
- **Keyboard:** the shortcut list (read-only).
- **About:** version, engine, licences (fonts and icons).

## 8. Interaction, keyboard, accessibility

**Shortcuts** (inactive while typing in a field):

| Key | Action |
|---|---|
| Space | Play / pause |
| Ctrl+→ / Ctrl+← | Next / previous track |
| Shift+→ / Shift+← | Seek ±10 s |
| Ctrl+↑ / Ctrl+↓ | Volume ±5 (existing) |
| Ctrl+. | Stop |
| / or Ctrl+F | Search tracks |
| Ctrl+1…4 | Albums, Artists, Tracks, Playlists |
| Ctrl+5 | Open or close the queue pane |
| Ctrl+R, F5, Ctrl+P | Ignored: no reload or print in a music player |
| Ctrl+, | Settings |
| Escape | Close popover / leave Now Playing |

Space works even when a button or slider has focus; text fields keep their keys. Right click opens Lyra's own menu (`menu.svelte.ts`, `ContextMenu.svelte`); the webview's menu is off outside text fields. Focus rings show after keyboard use only.

They are handled in one `shortcuts.ts` with a pure `actionFor(event)` that unit tests cover. MPRIS and media keys stay in M5.

**Accessibility fixes** (all from the audit in §1):
- A live region for playback state.
- `aria-valuetext` on both sliders.
- `aria-current="page"` only on the current item.
- One `h1` per view and no skipped levels.
- Focus styles for inputs, selects and radios.
- A play button with a changing label and no `aria-pressed`.
- Popover focus management.
- Removing the Library tabs (they become nav items).
- Sidebar rail items have `title` and `aria-label`.

**Minimum window:** 900 × 600, as today. It is checked at 900, 1280 and 1920 px wide.

## 9. Delivery: phases and acceptance

Each phase is one PR, keeps the app working, and is checked with screenshots of the real Tauri window: before and after, at 900 and 1440 px. The baseline and the screenshot script live in the scratchpad, not the repo.

| # | Phase | Contents | Accepted when |
|---|---|---|---|
| 1 | **Foundations** | `styles/tokens.css`, `base.css`, `components.css` (`@layer`). Vendored Manrope and Icon set. Button, input and focus styles. Strip bug fixed. All `.svelte` styles moved onto tokens, with no layout change. | `scripts/check.sh` passes. `styles/tokens.test.ts` fails on raw colours, font sizes or durations in `.svelte` styles. A Vitest test checks the §4 contrast pairs ≥ 4.5 from `tokens.css`. Screens look the same apart from type and colour. |
| 2 | **Shell and navigation** | Sidebar with direct items, rail mode, settings at the bottom, backend alert only on failure. Smoke test updated. | Smoke test passes. Each view is reachable by mouse and keyboard. 900 px works. |
| 3 | **Player bar** | `Slider`, `FormatIndicator` + lamp, device popover (`Popover`), volume, queue button, live region, `shortcuts.ts`. | Keyboard-only session: play, seek, skip, volume and device change. Screen reader (Orca) announces play and pause. Shortcut unit tests. Lamp states shown for yes / unverified / no on real hardware (ZEN ONE, SE, built-in). |
| 4 | **Signal path and Now Playing** | Rust `SignalPath` in the status, plus Now Playing layout. | Stages match Diagnostics for PCM, native DSD, DoP and conversion on the SE. Engine tests for the stage list. |
| 5 | **Library screens** | Albums, album detail, artists, tracks, playlists and queue on shared rows. `Artwork`. Search shortcut. | A07 measured with 10k tracks: scrolling stays smooth, and virtualization is added only if it is not. |
| 6 | **Settings** | Grouped Settings with folders, audio, diagnostics, keyboard and about. | All existing settings still save. Diagnostics unchanged in function. |

All six phases belong to M5. Phases 1–3 also complete the former M4c-2 items (design system, accessibility, shortcuts).

## 10. Decisions (2026-10-21)

1. **Typeface:** Manrope, from a shortlist of Manrope and Plus Jakarta Sans (§4). The choice is one `@font-face`, so it is easy to swap after phase 1.
2. **Assets:** fonts and icons are vendored in the repo. Lyra is self-contained, with no npm font or icon packages and no runtime fetches.
3. **Signal path:** reported by the UNDA engine (§7).
4. **Scope:** all six phases in M5.
5. **Stop button:** kept in the player bar.
