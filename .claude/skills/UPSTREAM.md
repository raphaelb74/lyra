# Vendored skills

## Rust

Copied unchanged from <https://github.com/actionbook/rust-skills> at commit `5c40d3ad785193231b7d0dbfb8e1eb447e5edd94` (2026-10-20). License: MIT, as declared in the upstream plugin manifest (the repository has no LICENSE file).

Included: `m01-ownership`, `m06-error-handling`, `m07-concurrency`, `m10-performance`, `m15-anti-pattern`, `unsafe-checker`.

Deliberately not installed: the plugin hook (fires on almost every prompt), the `@actionbookdev/mcp@latest` MCP server (unpinned remote code), browser-automation agents, and domain skills unrelated to Lyra.

Project override: `m07-concurrency` recommends tokio for I/O-bound work. Lyra's audio engine uses dedicated threads and bounded channels instead (see `docs/SPECIFICATION.md` §3). Async is fine in the Tauri adapter and, if measured to help, the library scanner; never on the audio path.

## Frontend design

`frontend-design` copied unchanged from <https://github.com/anthropics/claude-code/blob/main/plugins/frontend-design/skills/frontend-design/SKILL.md> at commit `d014fc0fa642` (2026-10-21). Its front matter points to a `LICENSE.txt` that the plugin does not include, and the repository declares no license.

Project override: the skill's freedom to pick a new look applies to the M4c-2 design system and the M5 design overhaul. Elsewhere, build on the tokens in `apps/desktop/src/app.css`, keep Lyra's accessibility rules, and keep copy in the plain voice of the existing UI.
