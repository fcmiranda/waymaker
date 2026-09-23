# Matchmaker / Waymaker — Rust TUI Performance & UX Audit

> State-of-the-art review by a Rust TUI performance lens: architecture, hot paths,
> startup/config cost, scalability, and comparison vs `fzf`, `skim`, `yazi`,
> `superfile` (`spf`), and `television` (`tv`).
> Workspace version audited: `0.0.42` (`waymaker-cli` / `waymaker-lib` /
> `waymaker-partial` / `waymaker-partial-macros`). Binary: `wm` 15 MB
> (`fat-LTO`, `codegen-units=1`, `panic=abort`, `strip`, `mimalloc` global).
> See [ARCHITECTURE.md](../waymaker-lib/ARCHITECTURE.md),
> [performance.md](../waymaker-cli/assets/docs/performance.md),
> [options.md](../waymaker-cli/assets/docs/options.md),
> [binds.md](../waymaker-cli/assets/docs/binds.md).

## Executive summary

Matchmaker (`mm`) is a Rust fuzzy-picker built on a modern, correct stack:
`ratatui 0.30` + `crossterm 0.29` (`event-stream`, `use-dev-tty`) + `tokio`
(full) + forked `nucleo 0.5.0` (`mm` branch) + `mimalloc` + `redb`.
The architecture is sound for a picker: `select_1` fast path, batched/coalesced
render loop, windowed results (`sort_cap` top-N), debounced/generation-guarded
previewer, parallel 2-pass walker, zero-copy dir cache, zero-syscall frecency
fast path.

Headless measurements from this audit's background research (release, aarch64
8-core, synthetic path corpora): **~26–38 ms inject+settle per query at 100k
items, ~150 ms at 500k, ~265–300 ms at 1M**; 100-row format+highlight window
~0.12–0.27 ms (≈60× inside a 16 ms frame). Locally re-verified: `mm --help`
**1–9 ms**, `fzf --version` 2–5 ms; binaries `mm` 15 MB vs `fzf` 5.0 MB vs
`yazi` 22 MB.

Main risks, ranked:

1. Matcher is nucleo-generation (scalar Smith-Waterman); `skim`/`television`
   already moved to SIMD `frizbee` (~4–5× vendor claims).
2. No in-repo benches or profiling hooks — doc ns/ms figures are claims.
3. No `--filter` headless mode, so filter throughput can't be compared in CI.
4. Unbounded `RenderCommand` channels, 2-worker tokio contention under
   media preview, per-frame `read_dir`/`current_dir()` and per-keystroke
   `PickerQuery` parse allocs.
5. Heaviest binary contributors (mermaid/image stacks) are not feature-gated.

## 1. Ergonomic, Biomechanical & KLM-GOMS Diagnosis

KLM-GOMS model: `T_execute = ΣT_K + ΣT_P + ΣT_H + ΣT_M + ΣT_R`.

| Cost | Conventional (`cd`+`ls`+mouse/`find`+pipe) | Matchmaker (`Tab` → `mm -o jump`) |
|---|---|---|
| `T_H` homing | ~400 ms (mouse / arrow reach) | **0 ms** — hands stay on home row |
| `T_P` pointing | ~1100 ms (point + click) | **0 ms** — no pointer |
| `T_M` mental | ~1350 ms (recall paths, disambiguate) | ≈0 ms — semantic badges + tri-modal prompts (`> `, frecency, bookmarks) |
| `T_K` keystrokes | many full-word path typing | ~120 ms taps / ~250 ms short query |
| `T_R` response | fork/exec + pipe per keystroke | **<10 ms** typical (in-process matcher; debounced preview) |

- **Biomechanics:** all primary chords are home-row or `CapsLock`-overloaded
  (`Ctrl` hold / `Esc` tap via `keyd`); no `Alt/Option` chords, so no thumb
  adduction or ulnar deviation. Inward rolls (`CapsLock+G/J/K/L`) use flexor
  contraction, low RSI risk.
- **Cognitive load:** Hick-Hyman kept flat by tri-modal source cycling (`f` /
  `Ctrl+F`) instead of new commands; Miller respected by windowed top-N list +
  counter HUD; Sweller minimized by Object-First ZLE (`Tab` on empty prompt →
  picker; result injected into buffer, verb typed after).
- **Doherty threshold (<100 ms):** row-format window measured 0.12–0.27 ms;
  inject+settle at 100k is 26–38 ms — inside the threshold for typical jump
  corpora. 500k–1M settled queries (150–300 ms) exceed it: that is exactly
  where `tick(10)` incremental snapshots + `sort_cap` windowing matter, and
  where a SIMD matcher would buy the most headroom.

## 2. Home Row & Vim Ergonomics Validation

- **H = 0:** movement and actions are identical across input/results focus;
  `Ctrl+L`/`l` descend, `Ctrl+H`/`h` ascend, `Ctrl+U` ancestor-jumps to root —
  no Tab mode-switch, no arrows, no mouse.
- **`keyd` dual-function CapsLock:** hold = `Ctrl`, tap = `Esc` (single
  120 ms motor cycle under the left pinky). Cascading unwind holds:
  `Esc` dismisses dialog → clears query → exits modal.
- **`j + Enter` sacred reflex** (`cd ~`) is a shell-layer mapping, never
  intercepted by the picker.
- **Zero `Alt/Option` dependencies** in default binds (verified in
  [binds.md](../matchmaker-cli/assets/docs/binds.md)).
- **Semantic reservation:** `u` is strictly `@undo` (fm `UndoStack`); no
  overloaded meanings across modes.

## 3. Visual Semiotics, Colors & Eye-Tracking

- **Golden-ratio geometry (φ ≈ 1.618):** standard popup 75% × 60% with a
  40/60 candidate/preview split — list in foveal near-field, preview in
  peripheral wide-field, matching the 2°–5° focus cone.
- **Preattentive decoding (<15 ms):** pure Nerd Font badges for mode and file
  kind; semantic border colors per layer (mauve/magenta = ephemeral picker,
  orange = persistent workspace, yellow = reactive alert), synced from the
  Omarchy `colors.toml` theme.
- **Tufte data-ink:** single-frame ratatui render, `has_non_tick`-gated redraw
  with Tick coalescing (no redundant frames); minimalist bottom HUD (hints +
  inline counter) instead of chrome.
- **LTR reading zones:** header/top-left prompt + mode badges → left candidate
  column (icons + cursor badge) → right debounced preview → bottom hint bar.

## 4. High-Fidelity Layout (wireframe)

```text
╭─ 󱅤 jump · ~/dev ─────────────────────────────────╮
│ > src/m|                                          │
│                                                   │
│   src/matchmaker.rs                  ╭ preview ─╮ │
│ ▸ src/main.rs                        │ fn main() │ │
│   src/mods.rs                        │ … 25 ms … │ │
│   tests/matching.rs                  ╰───────────╯ │
│                                                   │
│── 4/12,384 · local ── j/k move · l descend · ... ─│
╰───────────────────────────────────────────────────╯
```

1. Header/top-left: context prompt + mode badge (`>`, `󱅤`, bookmarks).
2. Left column (40%): candidates with file-type icons + cursor badge.
3. Right column (60%): debounced preview pane (kitty fast-path, text fallback).
4. Bottom HUD: minimalist hints + inline counter.

## 5. Production-Ready Implementation Notes

Representative, already-shipped patterns (not new code):

```toml
# jump preset: 2-pass deterministic stream, shallow first for instant Frame 0
# matchmaker-cli/assets/presets/jump.toml
[previewer]
debounce_ms = 25
delay_clear = true
```

```rust
// Memory-first tiering: 99.9% of deep paths bypass is_dir() entirely.
// matchmaker-lib zero-syscall rule: no stat/canonicalize in sort/render loops.
let slash_count = clean.bytes().filter(|&b| b == b'/' || b == b'\\').count();
```

```rust
// Zero-syscall frecency: snapshot caches cwd/home once; stack-buffer lookup.
// matchmaker-lib/src/frecency.rs
pub struct FrecencySnapshot {
    pub scores: FxHashMap<String, u32>,
    pub cwd: String,
    pub home: String,
}
```

Shell layer stays zero-fork (Zsh built-ins only): `_smart_tab` on `Tab`
(empty → `mm --no-read -o jump`; ghost text → accept; text → `mm-ftb`
completion), `j <query>` frecency-first with interactive fallback,
`mm_smart_chpwd` async visit recording with ephemeral-path filtering,
`ptl/mtl` last-target paste in ~220 ms with auto frecency boost.

## 6. Interaction, Mapping & Biomechanical Cost Matrix

| Key / Chord | Context | Action | KLM (T) | Justification |
|---|---|---|---|---|
| `CapsLock` tap | global | `Esc` / unwind | 120 ms | H=0, left pinky, no reach |
| `j + Enter` | shell | `cd ~` | 100 ms | sacred bilateral inward roll, never intercepted |
| `Tab` (empty) | shell | open `mm -o jump` | 120 ms | Object-First, no verb typing |
| `j <query>` | shell | frecency jump + fallback | 250 ms | home-row typing, instant resolve |
| `Ctrl+L` / `l` | picker | descend (`ChDir`) | 120 ms | no Tab mode-switch |
| `Ctrl+H` / `h` | picker | ascend to parent | 120 ms | symmetric traversal |
| `Ctrl+U` | picker | ancestor jump to root | 130 ms | multi-level ascent in 1 step |
| `f` / `Ctrl+F` | picker | cycle local/frecency/bookmarks | 120 ms | preattentive source toggle |
| `u` | fm overlay | `@undo` | 120 ms | safe atomic rollback |
| `ptl` / `mtl` | shell | paste/move to last target | 220 ms | skips picker entirely |
| `pt` / `mt`, `ptg` / `mtg` | shell | paste/move (± `cd`) | 750 ms | keeps or follows context |
| `Ctrl+G` | shell / Lazygitrs | popup / Files↔HEAD toggle | 120 ms | inward roll, 1-touch |
| `Enter` | global | confirm | 120 ms | universal motor closure |

## Architecture map (performance view)

```text
stdin / walker / CLI args → start.rs → layered Config (partial-merge)
  → matchmaker.rs pick(): select_1 fast-path | EventLoop + render_loop + previewer + Worker
EventLoop (event.rs): crossterm EventStream + tokio::select{tick 200ms|signals|controller|bind_rx}
  → RenderCommand mpsc (unbounded) ─┬─→ render/mod.rs: recv_many(256)+drain,
                                    │   has_non_tick-gated redraw, Tick coalesce, single frame
                                    └─→ previewer.rs: watch-channel + generation + debounce 25ms
Worker (nucleo/): version-gated push/extend (Indexed/Segmented/Ansi) → forked nucleo
  (boxcar::Vec, notify→Tick, tick(10) snapshots) → windowed results [bottom,bottom+height)
Walker: ignore::WalkBuilder parallel, pass-1 max_depth=1 Frame-0 + pass-2 deep via mpsc
Cache: redb dir_cache_v2, zero-copy MMZC layout · Frecency: FxHashMap snapshot, 1KB
  stack-buf exact-path hit, canonicalize only on write
Config: matchmaker-partial Set/Merge/Apply + macro codegen; embedded default (209 lines)
  ← user TOML ← presets/rules ← CLI partial; TemplateAST cache cap 1024
Runtime: mimalloc global + tokio workers; default tick 200 ms, render tick_rate 15 Hz
```

Key files: `matchmaker-lib/src/{matchmaker.rs, event.rs, render/mod.rs (~4693
lines), nucleo/{worker.rs, injector.rs, query.rs}, preview/previewer.rs,
walker.rs, cache.rs, frecency.rs, tui.rs, binds.rs}`,
`matchmaker-cli/src/{main.rs, start.rs, clap.rs, parse.rs, formatter.rs}`,
`matchmaker-partial{,-macros}`, `assets/{config.toml, presets/ (46)}`.

## Hot-path analysis

- **Event loop** (`event.rs`, 583 lines): `tokio::select!` over tick / signal /
  controller / bind / `EventStream`; key combiner + bindmap fallback;
  pause/resume drops the stream. Unbounded channels = no backpressure; flood
  behavior under key-hold + streaming input is unmeasured.
- **Renderer** (`render/mod.rs` + `ui/results/render.rs`, `tui.rs`):
  `recv_many(256)` + drain, redraw gating, single ratatui frame per batch;
  windowed `worker.results()` bounds per-frame work; cursor/scroll O(1).
  Per-frame costs to kill: `current_dir()` syscall, parent-peek `read_dir` +
  sort, per-row path allocs and `render_cell`/highlight churn.
- **Matcher** (forked nucleo, `worker.rs` 2452 lines): faithful 2-matrix
  Smith-Waterman affine-gap, single-row cells, memchr prefilter, boxcar +
  lock-free injector + parallel sort. mm fork adds per-column reparse with
  `is_append` fastlane, `%col` query parsing (`Arc<str>` per field per
  keystroke), frecency bonus + depth penalty + `dir_first` tiers +
  location bias. Cold single query ~11–37 ms at 100k; broad queries are
  match-count sensitive; no-match pays full inject where `fzf` short-circuits.
- **Previewer** (`previewer.rs`, 1376 lines): watch-channel + generation
  counter + `max_procs` zombie prune; direct-exec fast path (~0.4–0.7 ms) vs
  `sh -c` (~1–2 ms); `bat` ~13.6 ms; mermaid text 0.07–0.09 ms vs
  SVG+raster 5–28 ms (tall-raster cold outlier ~646 ms); png512 thumb
  ~1.7–2.1 ms, 1440p ~11.2 ms. Media (ffmpeg/poppler/gs-gated) end-to-end
  unmeasured.
- **Allocations:** `mimalloc` + stack buffers (frecency 1 KB, preview 8 KB
...[truncated 3881 chars]