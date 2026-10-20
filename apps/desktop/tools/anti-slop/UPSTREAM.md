# anti-slop (vendored)

Source: <https://github.com/dmmulroy/anti-slop> at commit `13909c9e07590f5a85d93c62712259fe14a883f7` (2026-10-20), copied from `skills/install-anti-slop/assets/anti-slop/`. License: MIT (Dillon Mulroy). The nested `vendor/eslint-stylistic/` keeps its own LICENSE and UPSTREAM.md.

Deviations:

- Removed the opt-in `effect/` rule group; Lyra does not use Effect.
- Loaded in ESLint through `eslintCompatPlugin` (see `apps/desktop/eslint.config.js`) instead of Oxlint, so one linter covers TypeScript and Svelte with type information.

This copy is project-owned. Update it deliberately: diff against a newer upstream commit and keep local changes.
