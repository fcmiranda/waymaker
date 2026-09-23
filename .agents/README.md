# Portable Agent Definitions

This directory holds tool-neutral agent definitions for the waymaker workspace.

## Purpose

- Keep the prompt body and routing metadata in one canonical place.
- Generate or copy thin wrappers for tool-specific runtimes from these files.
- Avoid coupling the reusable prompt content to one vendor's frontmatter schema.

## Layout

- `.agents/agents/*.md`: canonical agent definitions
- `.github/agents/*.agent.md`: GitHub Copilot wrappers
- `.claude/skills/<name>/SKILL.md`: Claude-style wrappers

## Canonical Frontmatter

The files in `.agents/agents/` use a neutral schema:

- `id`: stable machine-friendly identifier
- `title`: human-friendly display name
- `description`: routing summary
- `triggers`: keywords or phrases for dispatch
- `capabilities`: requested abilities such as `read`, `search`, `edit`, `execute`
- `mutability`: `read-only` or `read-write`
- `invocable`: whether the definition should be exposed directly

The Markdown body is the reusable prompt content. Tool-specific wrappers should preserve that body and only translate the metadata fields each runtime understands.