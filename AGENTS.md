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

## Release Policy & Workflow

Releasing is the explicit "ship it" step, intentionally decoupled from coding tasks, feature implementations, and bug fixes. See the [Release Skill](.agents/skills/release/SKILL.md) for the complete procedure.

- **Strict User Mandate**: Agents must **NEVER** cut, tag, or publish a release without an explicit user instruction (e.g. "corte a release v0.1.1", "cut a patch release"). Implementing a feature or making CI green never authorizes an automatic release.
- **Hybrid Release Architecture**:
  - **Manual Trigger**: The human developer decides *when* accumulated changes on `main` justify a new public version, avoiding release churn.
  - **100% Automated Execution**: Once a release tag (`v*`) is pushed, GitHub Actions (`.github/workflows/release.yml`) builds the 5-target multi-arch cross-compilation matrix (Linux musl x86/ARM, macOS Silicon/Intel, Windows MSVC), generates SHA256 checksums, and attaches binary assets automatically.
- **SemVer Classification**:
  - **Patch (`x.y.Z`)**: Bug fixes, performance tweaks, UI refinements, test coverage expansions.
  - **Minor (`x.Y.0`)**: Backwards-compatible features, new CLI options, or presets.
  - **Major (`X.0.0`)**: Breaking changes in CLI flags, config schemas, public APIs, or serialization.
- **Pre-Release Checklist**:
  1. Working tree is clean and `just install` + tests pass (`cargo test --workspace`).
  2. Bump `version` in workspace manifests (`waymaker-cli/Cargo.toml`, `waymaker-lib/Cargo.toml`, etc.).
  3. Promote `CHANGELOG.md` `## [Unreleased]` section to `## [X.Y.Z] - YYYY-MM-DD` so `taiki-e/create-gh-release-action` automatically populates the GitHub release notes.
  4. Commit release metadata: `git commit -am "chore(release): bump version to X.Y.Z"`.
  5. Push `main` to `origin`.
  6. Create annotated tag: `git tag -a vX.Y.Z -m "Release vX.Y.Z: <summary>"`.
  7. Push tag: `git push origin vX.Y.Z`.


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
