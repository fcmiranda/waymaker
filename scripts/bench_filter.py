#!/usr/bin/env python3
"""
Headless Filter Benchmark Harness: mm -f vs fzf -f
Measures latency, throughput, and memory consumption across synthetic corpora.
"""

import argparse
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time


def generate_corpus(size: int, output_path: str):
    """Generate a deterministic synthetic corpus of file paths."""
    modules = [
        "src", "core", "cli", "lib", "engine", "render", "ui", "parser",
        "router", "auth", "database", "network", "storage", "crypto",
        "utils", "config", "plugin", "worker"
    ]
    submodules = [
        "state", "event", "handler", "types", "constants", "view",
        "model", "controller", "service", "client", "stream", "cache",
        "metrics", "builder", "session"
    ]
    extensions = ["rs", "ts", "tsx", "js", "json", "toml", "md", "yaml", "html", "css"]

    root_files = [
        "README.md", "Cargo.toml", "Cargo.lock", "LICENSE-MIT", "LICENSE-APACHE",
        "Makefile", ".gitignore", ".editorconfig", "package.json", "tsconfig.json",
        "docker-compose.yml"
    ]

    with open(output_path, "w", encoding="utf-8") as f:
        for rf in root_files:
            f.write(rf + "\n")

        count = len(root_files)
        i = 0
        while count < size:
            mod_idx = i % len(modules)
            sub_idx = (i // len(modules)) % len(submodules)
            ext_idx = (i // (len(modules) * len(submodules))) % len(extensions)
            depth = (i % 6) + 1

            if depth == 1:
                if i % 3 == 0:
                    path = f"{modules[mod_idx]}/"
                else:
                    path = f"{modules[mod_idx]}_{i}.{extensions[ext_idx]}"
            elif depth == 2:
                path = f"{modules[mod_idx]}/{submodules[sub_idx]}_{i}.{extensions[ext_idx]}"
            elif depth == 3:
                nxt = modules[(mod_idx + 1) % len(modules)]
                path = f"{modules[mod_idx]}/{submodules[sub_idx]}/{nxt}_{i}.{extensions[ext_idx]}"
            elif depth == 4:
                path = f"crates/{modules[mod_idx]}/{submodules[sub_idx]}/src/component_{i}.{extensions[ext_idx]}"
            elif depth == 5:
                nxt = modules[(mod_idx + 2) % len(modules)]
                path = f"crates/{modules[mod_idx]}/{submodules[sub_idx]}/src/internal/{nxt}_{i}.{extensions[ext_idx]}"
            else:
                nxt = modules[(mod_idx + 3) % len(modules)]
                path = f"vendor/{modules[mod_idx]}/packages/{submodules[sub_idx]}/{nxt}/deep/node_{i}.{extensions[ext_idx]}"

            f.write(path + "\n")
            count += 1
            i += 1


def run_benchmark_command(cmd: list[str], input_bytes: bytes, runs: int) -> dict:
    """Run a command multiple times and collect execution statistics."""
    durations = []
    output_lines = 0

    for _ in range(runs):
        start = time.perf_counter()
        proc = subprocess.run(
            cmd,
            input=input_bytes,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        elapsed = time.perf_counter() - start
        durations.append(elapsed * 1000.0)  # ms
        if output_lines == 0 and proc.returncode in (0, 1):
            output_lines = len(proc.stdout.splitlines())

    return {
        "mean_ms": statistics.mean(durations),
        "stddev_ms": statistics.stdev(durations) if len(durations) > 1 else 0.0,
        "min_ms": min(durations),
        "max_ms": max(durations),
        "lines": output_lines,
    }


def main():
    parser = argparse.ArgumentParser(description="Matchmaker Headless Filter Benchmark Harness")
    parser.add_argument("--sizes", nargs="+", type=int, default=[10000, 100000],
                        help="Corpus sizes to benchmark (default: 10000 100000)")
    parser.add_argument("--runs", type=int, default=5,
                        help="Number of iterations per test (default: 5)")
    parser.add_argument("--mm-bin", type=str, default=None,
                        help="Path to mm binary (default: searches PATH or target/release/mm)")
    parser.add_argument("--markdown", action="store_true",
                        help="Output results as Markdown tables")
    parser.add_argument("--use-hyperfine", action="store_true",
                        help="Use hyperfine for timing if available in PATH")
    args = parser.parse_args()

    # Locate binaries
    mm_bin = args.mm_bin
    if not mm_bin:
        # Check target/release/mm first, then ~/.local/bin/mm, then PATH
        local_rel = os.path.abspath("target/release/mm")
        if os.path.exists(local_rel) and os.access(local_rel, os.X_OK):
            mm_bin = local_rel
        else:
            mm_bin = shutil.which("mm")

    if not mm_bin:
        print("Error: Could not locate 'mm' binary. Run `just install` or `cargo build --release` first.", file=sys.stderr)
        sys.exit(1)

    fzf_bin = shutil.which("fzf")

    hyperfine_bin = shutil.which("hyperfine") if args.use_hyperfine else None

    queries = [
        ("exact", "README.md"),
        ("fuzzy_short", "rnst"),
        ("subpath", "render/state"),
        ("broad_prefix", "src"),
        ("no_match", "xyz_nonexistent_token_123"),
    ]

    print(f"=== Matchmaker Filter Benchmark Harness ===")
    print(f"Matchmaker binary: {mm_bin}")
    print(f"fzf binary:        {fzf_bin or 'Not found (skipping fzf comparison)'}")
    print(f"Runs per query:    {args.runs}")
    print(f"Corpus sizes:      {args.sizes}\n")

    temp_dir = tempfile.mkdtemp(prefix="mm_bench_")
    try:
        for size in args.sizes:
            corpus_file = os.path.join(temp_dir, f"corpus_{size}.txt")
            print(f"Generating synthetic corpus of {size:,} paths...")
            generate_corpus(size, corpus_file)

            with open(corpus_file, "rb") as f:
                input_bytes = f.read()

            print(f"Corpus size: {len(input_bytes) / 1024 / 1024:.2f} MB ({size:,} lines)\n")

            if hyperfine_bin and args.use_hyperfine:
                print(f"--- Running with Hyperfine ({size:,} lines) ---")
                for q_label, query in queries:
                    print(f"\n[Query: {q_label} ('{query}')]")
                    cmd_mm = f"{mm_bin} -f '{query}' < {corpus_file}"
                    hf_cmd = [hyperfine_bin, "--runs", str(args.runs), "--warmup", "1"]
                    if fzf_bin:
                        cmd_fzf = f"{fzf_bin} -f '{query}' < {corpus_file}"
                        hf_cmd.extend(["-n", "mm -f", cmd_mm, "-n", "fzf -f", cmd_fzf])
                    else:
                        hf_cmd.extend(["-n", "mm -f", cmd_mm])
                    subprocess.run(hf_cmd)
                continue

            # Standard timing harness
            results = []

            for q_label, query in queries:
                mm_stats = run_benchmark_command([mm_bin, "-f", query], input_bytes, args.runs)

                fzf_stats = None
                if fzf_bin:
                    fzf_stats = run_benchmark_command([fzf_bin, "-f", query], input_bytes, args.runs)

                results.append({
                    "label": q_label,
                    "query": query,
                    "mm": mm_stats,
                    "fzf": fzf_stats,
                })

            # Print results
            if args.markdown:
                print(f"### Corpus: {size:,} lines\n")
                if fzf_bin:
                    print("| Query Pattern | Query | mm -f (mean ± σ) | fzf -f (mean ± σ) | Speedup vs fzf | Matches |")
                    print("|---|---|---|---|---|---|")
                    for r in results:
                        mm_t = f"{r['mm']['mean_ms']:.2f} ± {r['mm']['stddev_ms']:.2f} ms"
                        fzf_t = f"{r['fzf']['mean_ms']:.2f} ± {r['fzf']['stddev_ms']:.2f} ms"
                        ratio = r['fzf']['mean_ms'] / r['mm']['mean_ms'] if r['mm']['mean_ms'] > 0 else 1.0
                        speedup = f"**{ratio:.2f}x**" if ratio >= 1.0 else f"{ratio:.2f}x"
                        print(f"| `{r['label']}` | `{r['query']}` | {mm_t} | {fzf_t} | {speedup} | {r['mm']['lines']} |")
                else:
                    print("| Query Pattern | Query | mm -f (mean ± σ) | Min / Max | Matches |")
                    print("|---|---|---|---|---|")
                    for r in results:
                        mm_t = f"{r['mm']['mean_ms']:.2f} ± {r['mm']['stddev_ms']:.2f} ms"
                        min_max = f"{r['mm']['min_ms']:.2f} / {r['mm']['max_ms']:.2f} ms"
                        print(f"| `{r['label']}` | `{r['query']}` | {mm_t} | {min_max} | {r['mm']['lines']} |")
                print()
            else:
                print(f"{'Query Pattern':<15} {'Query':<16} {'mm -f (ms)':<20} {'fzf -f (ms)':<20} {'Speedup':<10} {'Matches'}")
                print("-" * 90)
                for r in results:
                    mm_str = f"{r['mm']['mean_ms']:.2f} ± {r['mm']['stddev_ms']:.2f}"
                    if r['fzf']:
                        fzf_str = f"{r['fzf']['mean_ms']:.2f} ± {r['fzf']['stddev_ms']:.2f}"
                        ratio = r['fzf']['mean_ms'] / r['mm']['mean_ms'] if r['mm']['mean_ms'] > 0 else 1.0
                        speedup_str = f"{ratio:.2f}x"
                    else:
                        fzf_str = "N/A"
                        speedup_str = "N/A"

                    print(f"{r['label']:<15} {r['query']:<16} {mm_str:<20} {fzf_str:<20} {speedup_str:<10} {r['mm']['lines']}")
                print()

    finally:
        shutil.rmtree(temp_dir, ignore_errors=True)


if __name__ == "__main__":
    main()
