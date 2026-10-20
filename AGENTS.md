# Lyra agent instructions

Read `docs/SPECIFICATION.md` and `docs/IMPLEMENTATION_PLAN.md` before modifying code.

## Engineering principles

- Linux first; Rust stable edition 2024; strict TypeScript; Svelte 5; Tauri 2.
- Prefer small cohesive crates and explicit error types; no `unwrap()`/`expect()` in production playback paths.
- Keep ALSA calls and disk I/O off the UI thread; no network services or telemetry by default.
- Playback must not use Web Audio, GStreamer, PulseAudio, or PipeWire. The only audio sink in v1 is direct ALSA hardware.
- Treat native DSD support as runtime-detected. Implement DoP as a transport mode, not a sample conversion.
- Do not silently fall back to PCM conversion, change rates, or apply software volume. Show output format and transport in UI.
- No allocation, file I/O, blocking synchronization, or log formatting in the steady-state realtime writer loop. Use bounded queues; document underrun behavior.
- Prefer safe Rust. Any `unsafe` needs a narrowly scoped justification and tests.
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

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`
- `npm run check` and `npm run build` in `apps/desktop`
- No regression in bit-perfect fixtures; no unapproved DSP; UI error states handled.
