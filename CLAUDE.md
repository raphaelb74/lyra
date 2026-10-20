# Lyra — Claude Code project instructions

## Mission
Linux-first bit-perfect audiophile player. Rust workspace + Tauri 2 + Svelte 5/TypeScript + ALSA. iFi ZEN DAC Signature is the first test device. Audio engine is independent of Tauri; never use Web Audio for playback.

## Source of truth
Read `AGENTS.md`, `docs/SPECIFICATION.md`, and `docs/IMPLEMENTATION_PLAN.md` **only when relevant**. Current milestone is M0 unless the user explicitly advances it. Check actual source and Git status before assuming work is complete. Do not blindly follow stale docs; flag contradictions.

## Model and delegation policy
- Main agent: **Opus**, solely responsible for architecture, prioritization, cross-file reasoning, final edits, and acceptance.
- **Every spawned subagent MUST explicitly use `model: haiku`. Never spawn Sonnet, Opus, or an inheriting/default-model subagent.** Use `.claude/agents/*` definitions with `model: haiku` or pass `model: "haiku"` explicitly to the Agent tool. If Haiku is unavailable, do the work in the main session or ask; do not silently upgrade.
- Delegate bounded, verifiable work: locating symbols/files, narrow API research, simple lint/test execution, focused diff review. Do not delegate architecture, security sign-off, complex concurrency reasoning, or broad refactors to Haiku.
- Avoid delegation when the task is faster to do directly or when startup/context cost outweighs savings. Usually 1–2 subagents; parallelize only independent work with clear ownership. No nested subagents.
- Every delegated prompt must specify: exact scope, allowed files, question, tools, max output, stop condition. Ask for **file:line evidence**, no raw file dumps, no generic summaries. Return <= 12 bullets or <= 500 words; escalate uncertainty to Opus.
- Do not let multiple agents edit the same files. Prefer read-only Haiku agents; Opus integrates and verifies all changes.

## Engineering rules
- Correctness > simplicity > measured performance > cleverness. Avoid speculative abstractions, unnecessary crates, wrappers, factories, state managers, and premature async.
- Rust: idiomatic ownership and Result-based errors; no `unwrap`/`expect` on recoverable production paths; no unjustified `unsafe`; document invariants around unsafe/FFI; minimize cloning and synchronization; preserve crate boundaries.
- Audio: keep control plane separate from audio data path; no avoidable allocation, filesystem/database I/O, logging, or blocking locks on the time-critical output path. Native DSD and DoP are distinct; do not claim hardware capability without ALSA evidence. Do not silently convert DSD to PCM or resample in bit-perfect mode.
- Svelte 5/TS: strict types, runes when appropriate, small explicit components, accessible interactions, no `any`/gratuitous casts, no redundant derived state or needless abstraction. Follow anti-slop principles.
- Tauri commands are adapters; no business logic in frontend or command handlers. Least-privilege capabilities and filesystem permissions.
- Test behavior, not implementation trivia. Reuse existing conventions. Never fabricate passing tests.

## Workflow
1. Inspect only relevant files, targeted searches first. For substantial tasks, state a concise plan and acceptance checks.
2. Implement the smallest coherent change. Do not expand scope or rewrite unrelated files.
3. Validate targeted checks, then broader checks when warranted: `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; frontend scripts from `package.json` (Svelte/TypeScript check, lint, build); Tauri build if available.
4. Before claiming completion, inspect diff, report changed files, tests actually run, failures/limitations, and remaining risks.
5. Never commit, push, reset, install global tools, change dependencies, or modify lockfiles without explicit approval when in review-only mode. Respect uncommitted work. Never overwrite user changes.

## Context and output budget
- Be concise. Report decisions, evidence, blockers, and outcomes—not narration or repeated file contents.
- Prefer `rg` scoped to directories over reading whole files; avoid recursive output of `target`, `node_modules`, generated artifacts.
- Read reference material on demand rather than pasting it into context. External guidance: `https://github.com/actionbook/rust-skills` and `https://github.com/dmmulroy/anti-slop`; verify applicability before using.
- After context compaction, recheck task scope and Git state; do not rely on stale summaries.

## Review-only mode
When asked to review, do not edit files. Report P0–P3 findings with exact file:line, impact, evidence, recommended fix, and M0 blocker status. Separate verified defects from design preferences. Main Opus decides verdict.
