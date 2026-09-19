use std::cell::RefCell;
use std::hint::black_box;
use std::time::Duration;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use matchmaker::action::SortOrder;
use matchmaker::frecency::FrecencySnapshot;
use matchmaker::matcher::{MatcherEngine, NucleoEngine};
use matchmaker::nucleo::Worker;
use rustc_hash::FxHashMap;

/// Generate a deterministic synthetic dataset of realistic file paths.
fn generate_dataset(size: usize) -> Vec<String> {
    let mut items = Vec::with_capacity(size);

    // Common root files
    let root_files = [
        "README.md",
        "Cargo.toml",
        "Cargo.lock",
        "LICENSE-MIT",
        "LICENSE-APACHE",
        "Makefile",
        ".gitignore",
        ".editorconfig",
        "package.json",
        "tsconfig.json",
        "docker-compose.yml",
    ];
    for f in &root_files {
        if items.len() < size {
            items.push(f.to_string());
        }
    }

    // Common directory roots
    let modules = [
        "src", "core", "cli", "lib", "engine", "render", "ui", "parser", "router", "auth",
        "database", "network", "storage", "crypto", "utils", "config", "plugin", "worker",
    ];

    let submodules = [
        "state",
        "event",
        "handler",
        "types",
        "constants",
        "view",
        "model",
        "controller",
        "service",
        "client",
        "stream",
        "cache",
        "metrics",
        "builder",
        "session",
    ];

    let extensions = ["rs", "ts", "tsx", "js", "json", "toml", "md", "yaml", "html", "css"];

    let unicode_samples = [
        "relatório_anual_2026.pdf",
        "código_fonte_núcleo.rs",
        "übersicht_märz.txt",
        "résumé_général.json",
        "日本語ドキュメント.md",
    ];

    let mut i = 0;
    while items.len() < size {
        let mod_idx = i % modules.len();
        let sub_idx = (i / modules.len()) % submodules.len();
        let ext_idx = (i / (modules.len() * submodules.len())) % extensions.len();
        let depth = (i % 6) + 1;

        let path = match depth {
            1 => {
                // Top-level direct directories or files
                if i % 3 == 0 {
                    format!("{}/", modules[mod_idx])
                } else {
                    format!("{}_{}.{}", modules[mod_idx], i, extensions[ext_idx])
                }
            }
            2 => {
                format!(
                    "{}/{}_{}.{}",
                    modules[mod_idx], submodules[sub_idx], i, extensions[ext_idx]
                )
            }
            3 => {
                format!(
                    "{}/{}/{}_{}.{}",
                    modules[mod_idx],
                    submodules[sub_idx],
                    modules[(mod_idx + 1) % modules.len()],
                    i,
                    extensions[ext_idx]
                )
            }
            4 => {
                if i % 50 == 0 {
                    format!(
                        "docs/i18n/{}/{}",
                        modules[mod_idx],
                        unicode_samples[i % unicode_samples.len()]
                    )
                } else {
                    format!(
                        "crates/{}/{}/src/component_{}.{}",
                        modules[mod_idx], submodules[sub_idx], i, extensions[ext_idx]
                    )
                }
            }
            5 => {
                format!(
                    "crates/{}/{}/src/internal/{}_{}.{}",
                    modules[mod_idx],
                    submodules[sub_idx],
                    modules[(mod_idx + 2) % modules.len()],
                    i,
                    extensions[ext_idx]
                )
            }
            _ => {
                format!(
                    "vendor/{}/packages/{}/{}/deep/node_{}.{}",
                    modules[mod_idx],
                    submodules[sub_idx],
                    modules[(mod_idx + 3) % modules.len()],
                    i,
                    extensions[ext_idx]
                )
            }
        };

        items.push(path);
        i += 1;
    }

    items
}

/// Helper to create and populate a worker with a dataset.
fn populate_worker(items: &[String]) -> Worker<String> {
    let mut worker = Worker::<String>::new_single_column();
    let injector = worker.nucleo.injector();
    for item in items {
        injector.push(item.clone(), |val, cols| {
            cols[0] = val.as_str().into();
        });
    }
    while worker.nucleo.snapshot().item_count() < items.len() as u32 {
        worker.nucleo.tick(10);
    }
    worker
}

fn bench_ingestion(c: &mut Criterion) {
    let mut group = c.benchmark_group("ingestion");
    group.measurement_time(Duration::from_secs(3));
    group.sample_size(10);

    for &size in &[10_000, 50_000] {
        let dataset = generate_dataset(size);
        group.bench_with_input(BenchmarkId::from_parameter(size), &dataset, |b, items| {
            b.iter_batched(
                || (Worker::<String>::new_single_column(), items),
                |(mut worker, batch)| {
                    let injector = worker.nucleo.injector();
                    for item in batch {
                        injector.push(item.clone(), |val, cols| {
                            cols[0] = val.as_str().into();
                        });
                    }
                    while worker.nucleo.snapshot().item_count() < batch.len() as u32 {
                        worker.nucleo.tick(10);
                    }
                    black_box(worker.nucleo.snapshot().item_count());
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

fn bench_query_matching(c: &mut Criterion) {
    let dataset = generate_dataset(50_000);
    let worker = RefCell::new(populate_worker(&dataset));

    let mut group = c.benchmark_group("query_matching_50k");
    group.measurement_time(Duration::from_secs(2));
    group.sample_size(15);

    let queries = [
        ("exact_file", "README.md"),
        ("prefix_dir", "src/"),
        ("fuzzy_short", "rnst"),
        ("subpath", "render/controller"),
        ("unicode", "relatório"),
        ("no_match", "nonexistent_query_xyz"),
    ];

    for (name, query) in queries {
        group.bench_with_input(BenchmarkId::new("query", name), &query, |b, q| {
            b.iter_batched(
                || {
                    // Reset query so the new search re-executes cleanly
                    let mut w = worker.borrow_mut();
                    w.find("");
                    while w.nucleo.tick(10).running {}
                },
                |_| {
                    let mut w = worker.borrow_mut();
                    w.find(black_box(q));
                    while w.nucleo.tick(10).running {}
                    black_box(w.nucleo.snapshot().matched_item_count());
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_incremental_refinement(c: &mut Criterion) {
    let dataset = generate_dataset(50_000);
    let worker = RefCell::new(populate_worker(&dataset));

    let mut group = c.benchmark_group("incremental_refinement_50k");
    group.measurement_time(Duration::from_secs(3));
    group.sample_size(10);

    // Simulates user typing: "s" -> "sr" -> "src" -> "src/" -> "src/controller"
    let keystrokes = ["s", "sr", "src", "src/", "src/controller"];

    group.bench_function("5_keystroke_typing_flow", |b| {
        b.iter_batched(
            || {
                let mut w = worker.borrow_mut();
                w.find("");
                while w.nucleo.tick(10).running {}
            },
            |_| {
                let mut w = worker.borrow_mut();
                for &stroke in &keystrokes {
                    w.find(black_box(stroke));
                    while w.nucleo.tick(10).running {}
                }
                black_box(w.nucleo.snapshot().matched_item_count());
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_ranking_and_sorting(c: &mut Criterion) {
    let dataset = generate_dataset(50_000);
    let mut worker = populate_worker(&dataset);

    // Execute a broad query that produces thousands of matches
    worker.find("src");
    while worker.nucleo.tick(10).running {}

    let mut group = c.benchmark_group("ranking_and_sorting_50k");
    group.measurement_time(Duration::from_secs(2));
    group.sample_size(20);

    // 1. Plain score ranking
    group.bench_function("default_score", |b| {
        b.iter(|| {
            black_box(worker.get_all_sorted());
        });
    });

    // 2. Directory-first ranking
    worker.dir_first = true;
    group.bench_function("dir_first", |b| {
        b.iter(|| {
            black_box(worker.get_all_sorted());
        });
    });

    // 3. Depth penalty ranking
    worker.depth_penalty = 15;
    group.bench_function("depth_penalty_15", |b| {
        b.iter(|| {
            black_box(worker.get_all_sorted());
        });
    });

    // 4. Frecency-boosted ranking
    let mut frec_scores = FxHashMap::default();
    for (i, item) in dataset.iter().take(500).enumerate() {
        frec_scores.insert(item.clone(), 1000 - (i as u32));
    }
    worker.frecency = true;
    worker.frecency_weight = 2;
    worker.frecency_snapshot = Some(FrecencySnapshot {
        scores: frec_scores,
        cwd: String::new(),
        home: String::new(),
    });

    group.bench_function("frecency_boosted", |b| {
        b.iter(|| {
            black_box(worker.get_all_sorted());
        });
    });

    // 5. Alphabetical sorting
    worker.set_sort_order(Some(SortOrder::Alphabetical));
    group.bench_function("alphabetical_sort", |b| {
        b.iter(|| {
            black_box(worker.get_all_sorted());
        });
    });

    group.finish();
}

fn bench_find_item_index(c: &mut Criterion) {
    let dataset = generate_dataset(50_000);
    let mut worker = populate_worker(&dataset);
    worker.dir_first = true;
    worker.depth_penalty = 15;

    worker.find("component");
    while worker.nucleo.tick(10).running {}

    let mut group = c.benchmark_group("find_item_index_50k");
    group.measurement_time(Duration::from_secs(2));
    group.sample_size(20);

    let all_matches = worker.get_all_sorted();
    let first_target = all_matches.first().copied().cloned().unwrap_or_default();
    let mid_target = all_matches
        .get(all_matches.len() / 2)
        .copied()
        .cloned()
        .unwrap_or_default();
    let non_target = "totally_nonexistent_item.txt".to_string();

    group.bench_function("head_index", |b| {
        b.iter(|| {
            black_box(worker.find_item_index(|s| s == &first_target));
        });
    });

    group.bench_function("mid_index", |b| {
        b.iter(|| {
            black_box(worker.find_item_index(|s| s == &mid_target));
        });
    });

    group.bench_function("miss_index", |b| {
        b.iter(|| {
            black_box(worker.find_item_index(|s| s == &non_target));
        });
    });

    group.finish();
}

fn bench_head_to_head(c: &mut Criterion) {
    let dataset = generate_dataset(50_000);
    let worker = RefCell::new(populate_worker(&dataset));
    let mut nucleo_engine = NucleoEngine::new();

    let mut group = c.benchmark_group("head_to_head_50k");
    group.measurement_time(Duration::from_secs(2));
    group.sample_size(15);

    let test_cases = [
        ("fuzzy_short", "rnst"),
        ("unicode", "relatório"),
        ("prefix_dir", "src/"),
        ("deep_path", "crates/parser/view"),
    ];

    for (case_name, query) in test_cases {
        // 1. Nucleo Worker (Async pipeline in Matchmaker)
        group.bench_with_input(BenchmarkId::new("nucleo_worker", case_name), &query, |b, &q| {
            b.iter_batched(
                || {
                    let mut w = worker.borrow_mut();
                    w.find("");
                    while w.nucleo.tick(10).running {}
                },
                |_| {
                    let mut w = worker.borrow_mut();
                    w.find(black_box(q));
                    while w.nucleo.tick(10).running {}
                    black_box(w.nucleo.snapshot().matched_item_count());
                },
                BatchSize::SmallInput,
            );
        });

        // 2. Nucleo Raw Matcher (Single thread)
        group.bench_with_input(BenchmarkId::new("nucleo_raw", case_name), &query, |b, &q| {
            b.iter(|| {
                black_box(nucleo_engine.search(black_box(q), &dataset));
            });
        });

        // 3. Frizbee SIMD (Single thread, typo=0)
        let frizbee_config = frizbee::Config::default();
        group.bench_with_input(BenchmarkId::new("frizbee_simd", case_name), &query, |b, &q| {
            b.iter(|| {
                let mut matcher = frizbee::Matcher::new(black_box(q), &frizbee_config);
                black_box(matcher.match_list(&dataset));
            });
        });

        // 4. Frizbee Parallel (Multi-threaded SIMD)
        group.bench_with_input(BenchmarkId::new("frizbee_parallel", case_name), &query, |b, &q| {
            b.iter(|| {
                let mut matcher = frizbee::Matcher::new(black_box(q), &frizbee_config);
                black_box(matcher.match_list_parallel(&dataset, 0));
            });
        });

        // 5. Frizbee with Typo Tolerance (typo=1)
        let mut typo_config = frizbee::Config::default();
        typo_config.max_typos = Some(1);
        group.bench_with_input(BenchmarkId::new("frizbee_typo_1", case_name), &query, |b, &q| {
            b.iter(|| {
                let mut matcher = frizbee::Matcher::new(black_box(q), &typo_config);
                black_box(matcher.match_list(&dataset));
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_ingestion,
    bench_query_matching,
    bench_incremental_refinement,
    bench_ranking_and_sorting,
    bench_find_item_index,
    bench_head_to_head
);
criterion_main!(benches);
