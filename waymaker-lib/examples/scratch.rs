use std::collections::HashSet;
use std::hint::black_box;
use std::io::{self, IsTerminal, Read};
use std::process::Command;
use std::time::{Duration, Instant};

use waymaker::config::{AutoscrollSettings, MatcherEngineType};
use waymaker::matcher::{FrizbeeEngine, MatcherEngine, NucleoEngine};
use waymaker::nucleo::Worker;
use ratatui::style::Style;

#[derive(Debug, Clone)]
struct BenchStats {
    min: Duration,
    median: Duration,
    mean: Duration,
    p95: Duration,
    max: Duration,
}

impl BenchStats {
    fn format_micros(&self) -> String {
        format!(
            "min: {:>6.1} µs | med: {:>6.1} µs | mean: {:>6.1} µs | p95: {:>6.1} µs",
            self.min.as_micros() as f64,
            self.median.as_micros() as f64,
            self.mean.as_micros() as f64,
            self.p95.as_micros() as f64,
        )
    }

    fn format_millis(&self) -> String {
        format!(
            "min: {:>6.2} ms | med: {:>6.2} ms | mean: {:>6.2} ms | p95: {:>6.2} ms",
            self.min.as_secs_f64() * 1000.0,
            self.median.as_secs_f64() * 1000.0,
            self.mean.as_secs_f64() * 1000.0,
            self.p95.as_secs_f64() * 1000.0,
        )
    }
}

fn bench_stat<F: FnMut()>(mut f: F, warmup: usize, iterations: usize) -> BenchStats {
    for _ in 0..warmup {
        f();
    }
    let mut times = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let start = Instant::now();
        f();
        times.push(start.elapsed());
    }
    times.sort();
    let min = times[0];
    let max = times[times.len() - 1];
    let median = times[times.len() / 2];
    let p95_idx = ((times.len() as f64 * 0.95) as usize).min(times.len() - 1);
    let p95 = times[p95_idx];
    let sum: Duration = times.iter().sum();
    let mean = sum / (times.len() as u32);
    BenchStats { min, median, mean, p95, max }
}

fn generate_synthetic_dataset(size: usize) -> Vec<String> {
    let mut items = Vec::with_capacity(size);
    let root_files = [
        "README.md", "Cargo.toml", "Cargo.lock", "LICENSE-MIT", "Makefile",
        ".gitignore", "package.json", "tsconfig.json", "docker-compose.yml",
    ];
    for f in &root_files {
        if items.len() < size {
            items.push(f.to_string());
        }
    }
    let modules = [
        "src", "core", "cli", "lib", "engine", "render", "ui", "parser", "router", "auth",
        "database", "network", "storage", "crypto", "utils", "config", "plugin", "worker",
    ];
    let submodules = [
        "state", "event", "handler", "types", "constants", "view", "model", "controller",
        "service", "client", "stream", "cache", "metrics", "builder", "session",
    ];
    let extensions = ["rs", "ts", "tsx", "js", "json", "toml", "md", "yaml", "html", "css"];
    let mut i = 0;
    while items.len() < size {
        let mod_idx = i % modules.len();
        let sub_idx = (i / modules.len()) % submodules.len();
        let ext_idx = (i / (modules.len() * submodules.len())) % extensions.len();
        let depth = (i % 6) + 1;
        let path = match depth {
            1 => {
                if i % 3 == 0 {
                    format!("{}/", modules[mod_idx])
                } else {
                    format!("{}_{}.{}", modules[mod_idx], i, extensions[ext_idx])
                }
            }
            2 => format!("{}/{}_{}.{}", modules[mod_idx], submodules[sub_idx], i, extensions[ext_idx]),
            3 => format!("{}/{}/{}_{}.{}", modules[mod_idx], submodules[sub_idx], modules[(mod_idx + 1) % modules.len()], i, extensions[ext_idx]),
            4 => format!("{}/{}/{}/{}_{}.{}", modules[mod_idx], submodules[sub_idx], modules[(mod_idx + 1) % modules.len()], submodules[(sub_idx + 1) % submodules.len()], i, extensions[ext_idx]),
            _ => format!("crates/{}/{}/{}/{}_{}.{}", modules[mod_idx], submodules[sub_idx], modules[(mod_idx + 2) % modules.len()], submodules[(sub_idx + 2) % submodules.len()], i, extensions[ext_idx]),
        };
        items.push(path);
        i += 1;
    }
    items
}

fn populate_worker(items: &[String], engine: MatcherEngineType) -> Worker<String> {
    let mut worker = Worker::<String>::new_single_column();
    worker.engine = engine;
    worker.dir_first = true;
    worker.depth_penalty = 15;
    let injector = worker.nucleo.injector();
    for item in items {
        injector.push(item.clone(), |val, cols| {
            cols[0] = val.clone().into();
        });
    }
    while worker.nucleo.snapshot().item_count() < items.len() as u32 {
        worker.nucleo.tick(10);
    }
    worker
}

fn main() {
    let num_cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    println!("\n╔══════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║              BENCHMARK ULTRA-DETALHADO: NUCLEO vs FRIZBEE (MATCHMAKER)                   ║");
    println!("╚══════════════════════════════════════════════════════════════════════════════════════════╝");
    println!(" • CPU Threads Lógicas:   {}", num_cpus);
    println!(" • Perfil de Execução:    Release (--release, opt-level=3)");
    println!(" • Aceleração de Vetores: SIMD AVX2 / SSE Habilitado\n");

    // -------------------------------------------------------------
    // 1. CARREGAMENTO DOS CORPORA DE TESTE
    // -------------------------------------------------------------
    // Corpus Real (do workspace atual via git ou find)
    let mut real_items = Vec::new();
    let out = Command::new("git").args(["ls-files"]).output();
    if let Ok(gout) = out {
        let text = String::from_utf8_lossy(&gout.stdout);
        real_items = text.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    }
    if real_items.is_empty() {
        real_items = generate_synthetic_dataset(2500);
    }

    let corpus_medium = generate_synthetic_dataset(25_000);
    let corpus_large = generate_synthetic_dataset(100_000);

    println!("────────────────────────────────────────────────────────────────────────────────────────────");
    println!(" [1/6] ESTATÍSTICAS DOS DATASETS DE TESTE");
    println!("────────────────────────────────────────────────────────────────────────────────────────────");
    println!(" 1. Corpus Real (Local Workspace):   {:>6} arquivos", real_items.len());
    println!(" 2. Corpus Médio (Repositório Médio): {:>6} arquivos", corpus_medium.len());
    println!(" 3. Corpus Grande (Monorepo Escala): {:>6} arquivos\n", corpus_large.len());

    // -------------------------------------------------------------
    // 2. BENCHMARK BRUTO DO ALGORITMO (MICRO-BENCHMARK)
    // -------------------------------------------------------------
    println!("────────────────────────────────────────────────────────────────────────────────────────────");
    println!(" [2/6] LATÊNCIA BRUTA DOS ALGORITMOS (Micro-benchmarks)");
    println!("────────────────────────────────────────────────────────────────────────────────────────────");

    let mut nucleo_engine = NucleoEngine::new();
    let mut frizbee_strict = FrizbeeEngine::new().with_typo_tolerance(false);
    let mut frizbee_typo = FrizbeeEngine::new().with_typo_tolerance(true);

    let test_queries = [
        ("Prefixo exato", "src/"),
        ("Subsequência", "rnst"),
        ("Caminho médio", "render/controller"),
        ("Fuzzy amplo", "state"),
    ];

    for (q_label, q_str) in test_queries {
        println!("\n▶ Query: \"{}\" ({}) sobre Corpus Médio (25.000 itens):", q_str, q_label);

        // Nucleo Single Thread
        let s_nucleo = bench_stat(|| {
            black_box(nucleo_engine.search(black_box(q_str), black_box(&corpus_medium)));
        }, 3, 20);

        // Frizbee Single Thread (Estrito)
        let s_friz_st = bench_stat(|| {
            black_box(frizbee_strict.search(black_box(q_str), black_box(&corpus_medium)));
        }, 3, 20);

        // Frizbee Multi-Thread Parallel (Estrito)
        let s_friz_par = bench_stat(|| {
            black_box(frizbee_strict.search_parallel(black_box(q_str), black_box(&corpus_medium), num_cpus));
        }, 3, 20);

        // Frizbee Typo Tolerant (max_typos = 1)
        let s_friz_typo = bench_stat(|| {
            black_box(frizbee_typo.search_parallel(black_box(q_str), black_box(&corpus_medium), num_cpus));
        }, 3, 20);

        let speedup_st = s_nucleo.mean.as_secs_f64() / s_friz_st.mean.as_secs_f64();
        let speedup_par = s_nucleo.mean.as_secs_f64() / s_friz_par.mean.as_secs_f64();

        println!("  • Nucleo (1 core):        {} (Base: 1.00x)", s_nucleo.format_millis());
        println!("  • Frizbee SIMD (1 core):   {} ({:.2}x mais rápido)", s_friz_st.format_millis(), speedup_st);
        println!("  • Frizbee SIMD ({} cores): {} ({:.2}x mais rápido)", num_cpus, s_friz_par.format_millis(), speedup_par);
        println!("  • Frizbee (Typo=1, {} c):  {}", num_cpus, s_friz_typo.format_millis());
    }

    // -------------------------------------------------------------
    // 3. BENCHMARK END-TO-END NO PIPELINE DO WORKER (`Worker::results`)
    // -------------------------------------------------------------
    println!("\n────────────────────────────────────────────────────────────────────────────────────────────");
    println!(" [3/6] PIPELINE REAL DO MATCHMAKER (`Worker::results()` com dir_first + depth_penalty)");
    println!("────────────────────────────────────────────────────────────────────────────────────────────");
    println!(" Testa o tempo total que o usuário experimenta para desenhar o primeiro frame na tela.");

    let mut worker_n_med = populate_worker(&corpus_medium, MatcherEngineType::Nucleo);
    let mut worker_f_med = populate_worker(&corpus_medium, MatcherEngineType::Frizbee);
    let mut matcher_n = nucleo::Matcher::new(nucleo::Config::DEFAULT);

    let worker_queries = ["", "src/", "render", "rnst"];
    for &q in &worker_queries {
        let label = if q.is_empty() { "Query Vazia (Startup Jump)" } else { q };
        println!("\n▶ Cenário: \"{}\" (25.000 itens):", label);

        // Nucleo Worker Pipeline
        let s_w_nucleo = bench_stat(|| {
            worker_n_med.find(black_box(q));
            while worker_n_med.nucleo.tick(10).running {}
            let _ = black_box(worker_n_med.results(
                0, 20, &[100], false, 0, Style::default(), &mut matcher_n,
                AutoscrollSettings::default(), 0, (0, false), true, false
            ));
        }, 3, 20);

        // Frizbee Worker Pipeline
        let s_w_frizbee = bench_stat(|| {
            worker_f_med.find(black_box(q));
            let _ = black_box(worker_f_med.results(
                0, 20, &[100], false, 0, Style::default(), &mut matcher_n,
                AutoscrollSettings::default(), 0, (0, false), true, false
            ));
        }, 3, 20);

        let speedup_w = s_w_nucleo.mean.as_secs_f64() / s_w_frizbee.mean.as_secs_f64();
        println!("  • Nucleo Worker Pipeline:  {}", s_w_nucleo.format_millis());
        println!("  • Frizbee Worker Pipeline: {} ({:.2}x mais rápido)", s_w_frizbee.format_millis(), speedup_w);
    }

    // -------------------------------------------------------------
    // 4. DIGITAÇÃO CONTÍNUA (STREAMING KEYSTROKE LATENCY)
    // -------------------------------------------------------------
    println!("\n────────────────────────────────────────────────────────────────────────────────────────────");
    println!(" [4/6] LATÊNCIA TECLA-A-TECLA (Simulação de Digitação Contínua)");
    println!("────────────────────────────────────────────────────────────────────────────────────────────");
    println!(" Simula o fluxo: 's' -> 'sr' -> 'src' -> 'src/' -> 'src/controller'");
    println!(" Mede se o estreitamento de busca (narrowing) do Nucleo supera o re-scan do Frizbee em 100k itens.");

    let mut worker_n_large = populate_worker(&corpus_large, MatcherEngineType::Nucleo);
    let mut worker_f_large = populate_worker(&corpus_large, MatcherEngineType::Frizbee);

    let typing_stream = ["s", "sr", "src", "src/", "src/controller"];
    println!("\n{:<16} | {:<24} | {:<24} | {:<12}", "Tecla Digitada", "Nucleo Latência (100k)", "Frizbee Latência (100k)", "Vantagem");
    println!("{:-<16}-+-{:-<24}-+-{:-<24}-+-{:-<12}", "", "", "", "");

    for &stroke in &typing_stream {
        let s_n = bench_stat(|| {
            worker_n_large.find(black_box(stroke));
            while worker_n_large.nucleo.tick(10).running {}
            let _ = black_box(worker_n_large.results(
                0, 20, &[100], false, 0, Style::default(), &mut matcher_n,
                AutoscrollSettings::default(), 0, (0, false), true, false
            ));
        }, 2, 10);

        let s_f = bench_stat(|| {
            worker_f_large.find(black_box(stroke));
            let _ = black_box(worker_f_large.results(
                0, 20, &[100], false, 0, Style::default(), &mut matcher_n,
                AutoscrollSettings::default(), 0, (0, false), true, false
            ));
        }, 2, 10);

        let ratio = s_n.mean.as_secs_f64() / s_f.mean.as_secs_f64();
        let advantage = if ratio > 1.0 {
            format!("Frizbee {:.1}x", ratio)
        } else {
            format!("Nucleo {:.1}x", 1.0 / ratio)
        };

        println!(
            " {:<15} | {:>9.2} ms (p95 {:>5.2}) | {:>9.2} ms (p95 {:>5.2}) | {:<12}",
            format!("\"{}\"", stroke),
            s_n.mean.as_secs_f64() * 1000.0,
            s_n.p95.as_secs_f64() * 1000.0,
            s_f.mean.as_secs_f64() * 1000.0,
            s_f.p95.as_secs_f64() * 1000.0,
            advantage
        );
    }

    // -------------------------------------------------------------
    // 5. TESTE DE RESILIÊNCIA A TYPOS (ERROS DE DIGITAÇÃO)
    // -------------------------------------------------------------
    println!("\n────────────────────────────────────────────────────────────────────────────────────────────");
    println!(" [5/6] RESILIÊNCIA A ERROS DE DIGITAÇÃO (Typo Tolerance)");
    println!("────────────────────────────────────────────────────────────────────────────────────────────");

    let typo_cases = [
        ("dcos", "docs (transposição de letras)"),
        ("cnofig", "config (inversão)"),
        ("redner", "render (troca de ordem)"),
        ("srevice", "service (troca comum)"),
        ("pargser", "parser (letra extra)"),
    ];

    println!("{:<10} | {:<28} | {:<15} | {:<15}", "Query Typo", "Intenção Original", "Nucleo Matches", "Frizbee Matches");
    println!("{:-<10}-+-{:-<28}-+-{:-<15}-+-{:-<15}", "", "", "", "");

    for &(typo_q, intent) in &typo_cases {
        let n_res = nucleo_engine.search(typo_q, &corpus_medium);
        let f_res = frizbee_typo.search_parallel(typo_q, &corpus_medium, num_cpus);

        println!(
            " {:<9} | {:<28} | {:>14} | {:>14}",
            typo_q,
            intent,
            if n_res.is_empty() { "0 (FALHOU)" } else { "Matches OK" },
            format!("{} encontrados", f_res.len())
        );
    }

    // -------------------------------------------------------------
    // 6. QUALIDADE E CONCORDÂNCIA DO TOP 5
    // -------------------------------------------------------------
    println!("\n────────────────────────────────────────────────────────────────────────────────────────────");
    println!(" [6/6] CONCORDÂNCIA DE RANKING E TOP 5 (Query: \"worker\")");
    println!("────────────────────────────────────────────────────────────────────────────────────────────");

    let query_quality = "worker";
    let n_matches = nucleo_engine.search(query_quality, &corpus_medium);
    let f_matches = frizbee_strict.search_parallel(query_quality, &corpus_medium, num_cpus);

    println!(" • Total de matches Nucleo:  {}", n_matches.len());
    println!(" • Total de matches Frizbee: {}\n", f_matches.len());

    let top_k = 5.min(n_matches.len()).min(f_matches.len());
    println!("{:<4} | {:<42} | {:<42}", "#", "Nucleo (Score DP)", "Frizbee (SIMD Score)");
    println!("{:-<4}-+-{:-<42}-+-{:-<42}", "", "", "");

    for i in 0..top_k {
        let n_str = &corpus_medium[n_matches[i].index as usize];
        let f_str = &corpus_medium[f_matches[i].index as usize];
        let trunc = |s: &str| if s.len() > 40 { format!("...{}", &s[s.len() - 37..]) } else { s.to_string() };
        println!("{:<4} | {:<42} | {:<42}", i + 1, trunc(n_str), trunc(f_str));
    }

    let top_20 = 20.min(n_matches.len()).min(f_matches.len());
    let n_set: HashSet<_> = n_matches.iter().take(top_20).map(|m| m.index).collect();
    let f_set: HashSet<_> = f_matches.iter().take(top_20).map(|m| m.index).collect();
    let common = n_set.intersection(&f_set).count();
    let overlap_pct = (common as f64 / top_20 as f64) * 100.0;

    println!("\n • Sobreposição no Top 20: {} de {} itens idênticos ({:.1}% de concordância)", common, top_20, overlap_pct);
    println!("════════════════════════════════════════════════════════════════════════════════════════════\n");
}
