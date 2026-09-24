# Waymaker Agent Guide

This workspace is a Rust monorepo with four main crates:

- `waymaker-cli`: command-line entrypoint, clap parsing, config loading, preset registration, and CLI-specific actions.
- `waymaker-lib`: core picker library, renderer, event loop, previewer, and dynamic handler system.
- `waymaker-partial`: partial-struct merge traits and tests that power config layering.
- `waymaker-partial-macros`: proc macros that generate partial structs and derive merge helpers.

Prefer linking to existing docs instead of restating them:

- Architecture and event flow: [waymaker-lib/ARCHITECTURE.md](waymaker-lib/ARCHITECTURE.md)
- CLI override syntax: [waymaker-cli/assets/docs/options.md](waymaker-cli/assets/docs/options.md)
- Bind syntax: [waymaker-cli/assets/docs/binds.md](waymaker-cli/assets/docs/binds.md)
- Template placeholders: [waymaker-cli/assets/docs/template.md](waymaker-cli/assets/docs/template.md)
- Partial-config behavior: [waymaker-partial/README.md](waymaker-partial/README.md)

## Working Rules

- **Mandatory Global Build**: Always run `just install` after ANY new feature, bug fix, refactor, or alteration before finalizing. It builds the release workspace and updates the global binary at `$HOME/.local/bin/wm`. Targeted tests complement this build but do not replace it.
- Prefer narrow validation first: `cargo test -p <crate>` before `cargo test --workspace` when a change is crate-local.
- Use `just preview -- --help` or `cargo run -p waymaker-cli -F experimental -- <args>` when validating CLI behavior.
- Use `dprint fmt` or `dprint check` for Markdown and TOML edits.
- Keep config-related changes aligned with the partial-merge model instead of patching around deserialization or override behavior.
- When changing picker behavior, trace the path through event -> action -> renderer/handler rather than only editing a UI surface.
- Treat `waymaker-partial-macros` as sensitive code: prefer small changes and validate with targeted tests because the generated behavior is easy to regress.

## Commit Policy

- After implementing a feature, fix, or alteration, ensure the global build (`just install`) succeeds, then create a git commit for that change.
- Do not amend commits unless explicitly requested.
- If unrelated worktree changes make a clean feature commit ambiguous, stop and ask before committing.

## Navigation Hints

- Start in `waymaker-cli/src/config.rs` and `waymaker-lib/src/config.rs` for config shape questions.
- Start in `waymaker-lib/src/action.rs`, `waymaker-lib/src/matchmaker.rs`, and [waymaker-lib/ARCHITECTURE.md](waymaker-lib/ARCHITECTURE.md) for action flow or TUI behavior.
- Start in `waymaker-cli/assets/config.toml` and `waymaker-cli/assets/presets/` for default UX and preset behavior.
- Start in `waymaker-partial/tests/` for expected merge semantics and macro edge cases.

## Custom Agents

- Use the `Waymaker Architecture Navigator` agent for event flow, previewer, renderer, and feature-routing questions.
- Use the `Waymaker Config Surgeon` agent for TOML config, CLI overrides, presets, and template substitution work.
- Use the `Waymaker Bind Auditor` agent for semantic triggers, mode-scoped binds, and conflict analysis.
- Use the `Waymaker Partial Merge Specialist` agent for `waymaker-partial` and macro-generated partial-struct behavior.
