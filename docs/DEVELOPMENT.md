# Development

Target: Linux Mint 22.3 / Ubuntu 24.04, x86_64.

## Prerequisites

System packages (Tauri 2 prerequisites, ALSA headers, desktop test tools):

```sh
sudo apt-get update
sudo apt-get install -y build-essential curl pkg-config libssl-dev libasound2-dev \
  libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf \
  desktop-file-utils xvfb xdotool webkit2gtk-driver shellcheck
```

Optional: `flac` (`sudo apt-get install flac`), only to regenerate the FLAC test fixtures. Python 3 runs the scripts and smoke tests.

Toolchains:

- **Rust** via [rustup](https://rustup.rs/). `rust-toolchain.toml` pins the version, rustfmt and Clippy; run `rustup toolchain install` once in the repository.
- **Node 24 LTS** via [nvm](https://github.com/nvm-sh/nvm). `apps/desktop/.nvmrc` selects the version (`nvm use`). Avoid Mint's packaged Node.
- **Cargo tools:** `cargo install --locked cargo-deny cargo-audit` and `cargo install --locked tauri-driver --version 2.0.5`. `cargo audit` is a quick standalone advisory scan; `cargo deny check` covers the same RustSec database plus licenses and sources.

## Run

```sh
cd apps/desktop
npm ci
npm run tauri dev     # native window with Rust backend
npm run dev           # browser-only preview; reports that Rust is unavailable
```

To play: choose the output device, drop FLAC, WAV, DSF or DFF files onto the window, and click one. DSD plays as native DSD when the DAC exposes it, otherwise as DoP. Lyra plays only files that were dropped. Click **Refresh** after plugging in a DAC (no hotplug detection yet).

Release build (fat LTO, the slowest and most memory-hungry build):

```sh
cd apps/desktop
CARGO_BUILD_JOBS=2 npm run tauri build   # .deb in target/release/bundle/deb/
```

The binary alone is `target/release/lyra-desktop`.

## Validate

```sh
scripts/check.sh
```

Runs ShellCheck on `scripts/`, Rust formatting, Clippy (warnings denied), workspace tests, `cargo deny` (licenses, RustSec advisories, crate sources; policy in `deny.toml`), Prettier, svelte-check, ESLint, Vitest, the production frontend build, a native debug `.deb` build, and `.desktop` validation. CI runs the same script on Ubuntu 24.04. No DAC is required.

Real-window smoke tests (after the native build; needs `tauri-driver`):

```sh
cargo install tauri-driver --version 2.0.5 --locked
cd apps/desktop
xvfb-run -a npm run test:desktop   # WebKitGTK window + IPC, screenshot in artifacts/
xvfb-run -a npm run test:dev       # `tauri dev` opens the window
```

Set `LYRA_NATIVE_DRIVER` if `WebKitWebDriver` is not on `PATH`. See [Tauri WebDriver](https://v2.tauri.app/develop/tests/webdriver/).

## Lint policy

- **Rust:** one policy in the root `Cargo.toml` (`[workspace.lints]`), inherited by every crate with `lints.workspace = true`. Clippy `pedantic` is on; `unwrap`/`expect`/`panic!` are denied outside tests (`clippy.toml`). Any `unsafe` block needs a `// SAFETY:` comment.
- **TypeScript/Svelte:** `apps/desktop/eslint.config.js` with typescript-eslint `strictTypeChecked` + `stylisticTypeChecked`, `eslint-plugin-svelte`, and the vendored [anti-slop](../apps/desktop/tools/anti-slop/UPSTREAM.md) rules. Warnings fail the run. Disable a rule only on one line, with a `--` reason.

## Build profiles

| Profile | Used by | Settings |
| --- | --- | --- |
| `dev` | `cargo build`, `tauri dev` | Our crates unoptimized for fast rebuilds; dependencies at `opt-level = 2` so decoding and the UI run at near-release speed. |
| `release` | `tauri build` | `opt-level = 3`, fat LTO, one codegen unit, symbols stripped. Panics unwind so a failing thread can be reported instead of killing the app. |
| `profiling` | `cargo build --profile profiling` | Release plus debug info, for `perf`/flamegraphs. |

VS Code: `.vscode/settings.json` gives rust-analyzer its own `target/rust-analyzer` directory (no lock fights or rebuild thrash with terminal builds), limits its parallelism, and excludes `target/` and `node_modules/` from file watching. On low-memory machines (8 GB), limit terminal builds with `CARGO_BUILD_JOBS=2` and run one build at a time.

Builds target generic x86-64 so the `.deb` runs on any machine. For a local-only build tuned to your CPU: `RUSTFLAGS="-C target-cpu=native" cargo build --release`.

## Assets and metadata

- `npm run icons` regenerates `src-tauri/icons/` from `apps/desktop/assets/lyra.svg`.
- `apps/desktop/src-tauri/lyra.desktop` is the Linux desktop entry template; Tauri fills in the executable and icon at packaging time.
- `scripts/record-licenses.py` refreshes `docs/licenses/` from the lockfiles. Run `cargo fetch --locked` first.

## Test fixtures

Synthetic and small; committed, so tests need no tools. Never commit real music.

| Script | Writes | Reference |
| --- | --- | --- |
| `scripts/make-pcm-fixtures.py` | `crates/format-pcm/tests/fixtures/`: WAV + FLAC, 16/44.1 and 24/96 (needs `flac`) | the WAV data chunk |
| `scripts/make-dsd-fixtures.py` | `crates/format-dsd/tests/fixtures/`: DSF + DFF, DSD64 stereo, same bits (`--multiple 128\|256\|512` for higher rates) | the DFF data chunk |

Both take `--seconds N --out artifacts/audio` for longer listening files; `artifacts/` is ignored by Git. The DSD generator is pure Python: about 5 s per second of DSD256.

## Hardware testing

No DAC is needed for `scripts/check.sh`; these tools are for real devices.

```sh
scripts/probe-dac.sh > report.txt                          # read-only ALSA/USB descriptors of every USB DAC
cargo run -p audio-alsa --example devices                  # playback devices and what they accept
cargo run -p audio-alsa --example play -- hw:CARD=ONE,DEV=0 track.flac
                                                           # plays, prints the kernel's hw_params and underruns
cargo run -p audio-alsa --example play -- hw:CARD=ONE,DEV=0 track.dsf native
                                                           # DSD transport: auto (default), native or dop
cargo run --release -p format-pcm --example verify -- *.flac   # full decode, FLAC MD5 check
cargo run --release -p format-dsd --example inspect -- *.dsf   # full DSD read, frame count, hash
```

**DAC held by PipeWire.** If Lyra reports `in use by pipewire`, switch the card's PipeWire profile off while testing and back on afterwards (`pactl list cards short` lists card names):

```sh
pactl set-card-profile <card> off
pactl set-card-profile <card> output:analog-stereo
```

**DAC volume.** Many USB DACs expose a digital volume that PipeWire leaves just below 0 dB. Lyra sets a volume within 0.5 dB of 0 dB to exactly 0 dB while it plays and restores it afterwards; lower settings are reported as not bit-perfect. Check with `amixer -c <card> contents`.

**Reports.** Record each DAC in [docs/hardware/](hardware/), one file per model: probe output, formats and rates exposed, playback results (kernel `hw_params`, underruns), and quirks. [iFi ZEN ONE](hardware/ifi-zen-one.md) is the template.

## Rules

Engineering rules for humans and agents are in [AGENTS.md](../AGENTS.md). In short: no Web Audio, no PipeWire/PulseAudio path, no silent resampling or DSD→PCM conversion, and no bit-perfect claims without fixture and hardware evidence.
