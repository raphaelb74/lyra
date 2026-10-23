# Lyra agent instructions

Read `docs/SPECIFICATION.md` and `docs/IMPLEMENTATION_PLAN.md` before modifying code.

## Engineering principles

- Linux first; Rust stable edition 2024; strict TypeScript; Svelte 5; Tauri 2.
- Prefer small cohesive crates and explicit error types; no `unwrap()`/`expect()` in production playback paths.
- Keep ALSA calls and disk I/O off the UI thread; no network services or telemetry by default.
- Playback must not use Web Audio, GStreamer, PulseAudio, or PipeWire. The only audio sink in v1 is direct ALSA hardware.
- Treat native DSD support as runtime-detected. Implement DoP as a transport mode, not a sample conversion.
- Do not silently fall back to PCM conversion, change rates, or apply software volume. Show output format and transport in UI.
- Volume, volume levelling and caps act only on the DAC's own hardware volume, never on samples; levelling is off by default and stays off during DoP.
- No allocation, file I/O, blocking synchronization, or log formatting in the steady-state realtime writer loop. Use bounded queues; document underrun behavior.
- Prefer safe Rust. Any `unsafe` needs a narrowly scoped justification and tests.
- Efficiency is a product goal: no polling where an event exists; background work (scans, loudness measuring) is bounded to what the user needs now and runs at idle priority. Measure before and after a performance change.
- Hot crates (decoders, DSD conversion, loudness measuring) are optimised in the dev profile (`Cargo.toml`), so `npm run tauri dev` behaves like a release build.
- A new Tauri command goes in `generate_handler!` (`src-tauri/src/lib.rs`), in `build.rs` and in `capabilities/default.json`; a test checks the three agree.
- Avoid GPL/AGPL dependencies unless explicitly approved. Record third-party licenses.
- Use rustfmt, clippy, cargo test; format and type-check frontend before completing a milestone.
- Do not claim bit-perfect output until supported by test vectors and ALSA/hardware verification.

## Workflow

1. Implement one milestone at a time and keep workspace compiling.
2. Add unit tests for parsing, sample ordering, DoP markers, channel interleave, and track transitions.
3. Use synthetic DSD fixtures checked into test assets (not copyrighted music).
4. Mock ALSA in CI; require physical DAC for hardware acceptance tests.
5. Document hardware/firmware, kernel, ALSA format, and measured outcome in test reports.
6. Update documentation and check acceptance criteria with every milestone.

## Definition of done

- `scripts/check.sh` passes (fmt, clippy `-D warnings`, Rust and UI tests, lint, cargo-deny, .deb build); close `tauri dev` first, it holds the binary the .deb build replaces
- `npm run test:desktop` under `xvfb-run` for UI or IPC changes
- No regression in bit-perfect fixtures; no unapproved DSP; UI error states handled.
