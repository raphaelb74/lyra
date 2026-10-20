# Development

Target: Linux Mint 22.3 / Ubuntu 24.04, x86_64.

## Prerequisites

System packages (Tauri 2 prerequisites, ALSA headers for later milestones, desktop test tools):

```sh
sudo apt-get update
sudo apt-get install -y build-essential curl pkg-config libssl-dev libasound2-dev \
  libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf \
  desktop-file-utils xvfb xdotool webkit2gtk-driver
```

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

## Validate

```sh
scripts/check.sh
```

Runs Rust formatting, Clippy (warnings denied), workspace tests, `cargo deny` (licenses, RustSec advisories, crate sources; policy in `deny.toml`), Prettier, svelte-check, ESLint, Vitest, the production frontend build, a native debug `.deb` build, and `.desktop` validation. CI runs the same script on Ubuntu 24.04. No DAC is required.

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

VS Code: `.vscode/settings.json` gives rust-analyzer its own `target/rust-analyzer` directory (no lock fights or rebuild thrash with terminal builds), limits its parallelism, and excludes `target/` and `node_modules/` from file watching. On low-memory machines, also limit terminal builds: `cargo build --jobs 4`.

Builds target generic x86-64 so the `.deb` runs on any machine. For a local-only build tuned to your CPU: `RUSTFLAGS="-C target-cpu=native" cargo build --release`.

## Assets and metadata

- `npm run icons` regenerates `src-tauri/icons/` from `apps/desktop/assets/lyra.svg`.
- `apps/desktop/src-tauri/lyra.desktop` is the Linux desktop entry template; Tauri fills in the executable and icon at packaging time.
- `scripts/record-licenses.py` refreshes `docs/licenses/` from the lockfiles. Run `cargo fetch --locked` first.

## Rules

Engineering rules for humans and agents are in [AGENTS.md](../AGENTS.md). In short: no Web Audio, no PipeWire/PulseAudio path, no silent resampling or DSD→PCM conversion, and no bit-perfect claims without fixture and hardware evidence.
