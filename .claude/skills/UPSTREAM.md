# Vendored Rust skills

Copied unchanged from <https://github.com/actionbook/rust-skills> at commit `5c40d3ad785193231b7d0dbfb8e1eb447e5edd94` (2026-10-20). License: MIT, as declared in the upstream plugin manifest (the repository has no LICENSE file).

Included: `m01-ownership`, `m06-error-handling`, `m07-concurrency`, `m10-performance`, `m15-anti-pattern`, `unsafe-checker`.

Deliberately not installed: the plugin hook (fires on almost every prompt), the `@actionbookdev/mcp@latest` MCP server (unpinned remote code), browser-automation agents, and domain skills unrelated to Lyra.

Project override: `m07-concurrency` recommends tokio for I/O-bound work. Lyra's audio engine uses dedicated threads and bounded channels instead (see `docs/SPECIFICATION.md` §3). Async is fine in the Tauri adapter and, if measured to help, the library scanner; never on the audio path.
