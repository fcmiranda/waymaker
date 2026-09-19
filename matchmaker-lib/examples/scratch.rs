use std::collections::HashSet;
use std::io::{self, IsTerminal, Read};
use std::process::Command;

use matchmaker::matcher::{FrizbeeEngine, MatcherEngine, NucleoEngine};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let query = args.get(1).map(|s| s.as_str()).unwrap_or("config");

    println!("============================================================");
    println!(" Matchmaker Engine Comparison Harness: Nucleo vs Frizbee");
    println!(" Query: \"{}\"", query);
    println!("============================================================\n");

    // 1. Ingest items from stdin or dynamically run `mm -o jump -f ""`
    let mut input_text = String::new();
    if !io::stdin().is_terminal() {
        let _ = io::stdin().read_to_string(&mut input_text);
    }

    if input_text.is_empty() {
        println!("→ Coletando lista de arquivos via `mm -o jump -f \"\"`...");
        let output = Command::new("mm")
            .args(["-o", "jump", "-f", ""])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                input_text = String::from_utf8_lossy(&out.stdout).to_string();
            }
            _ => {
                // Fallback to git ls-files or find
                let out = Command::new("git").args(["ls-files"]).output();
                if let Ok(gout) = out {
                    input_text = String::from_utf8_lossy(&gout.stdout).to_string();
                }
            }
        }
    }

    let items: Vec<String> = input_text
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    println!("→ Total de itens no corpus do Jump: {}\n", items.len());

    if items.is_empty() {
        eprintln!("Erro: nenhum item encontrado no corpus.");
        return;
    }

    // 2. Executar Nucleo Engine
    let mut nucleo = NucleoEngine::new();
    let nucleo_matches = nucleo.search(query, &items);

    // 3. Executar Frizbee Engine (Estrito: max_typos = 0)
    let mut frizbee_strict = FrizbeeEngine::new().with_typo_tolerance(false);
    let frizbee_strict_matches = frizbee_strict.search(query, &items);

    // 4. Executar Frizbee Engine (Tolerante: max_typos = 1)
    let mut frizbee_typo = FrizbeeEngine::new().with_typo_tolerance(true);
    let frizbee_typo_matches = frizbee_typo.search(query, &items);

    println!("------------------------------------------------------------");
    println!(" 1. Resumo de Correspondências (Matches)");
    println!("------------------------------------------------------------");
    println!(" • Nucleo (Atual):            {} matches", nucleo_matches.len());
    println!(" • Frizbee (SIMD Estrito):     {} matches", frizbee_strict_matches.len());
    println!(" • Frizbee (Typo Tolerant):    {} matches\n", frizbee_typo_matches.len());

    // 5. Comparação do Top 10
    println!("------------------------------------------------------------------------------------------------------------------------");
    println!(" 2. Comparação Lado a Lado (Top 10)");
    println!("------------------------------------------------------------------------------------------------------------------------");
    println!("{:<4} | {:<36} | {:<36} | {:<36}", "#", "Nucleo (Atual)", "Frizbee (Estrito)", "Frizbee (Typo 1)");
    println!("{:-<4}-+-{:-<36}-+-{:-<36}-+-{:-<36}", "", "", "", "");

    for rank in 0..10 {
        let n_item = nucleo_matches.get(rank).map(|m| items[m.index as usize].as_str()).unwrap_or("-");
        let fs_item = frizbee_strict_matches.get(rank).map(|m| items[m.index as usize].as_str()).unwrap_or("-");
        let ft_item = frizbee_typo_matches.get(rank).map(|m| items[m.index as usize].as_str()).unwrap_or("-");

        // Truncate if longer than 35 chars
        let trunc = |s: &str| -> String {
            if s.len() > 35 {
                format!("...{}", &s[s.len() - 32..])
            } else {
                s.to_string()
            }
        };

        println!(
            "{:<4} | {:<36} | {:<36} | {:<36}",
            rank + 1,
            trunc(n_item),
            trunc(fs_item),
            trunc(ft_item)
        );
    }
    println!("------------------------------------------------------------------------------------------------------------------------\n");

    // 6. Métricas de Concordância (Overlap & Rank Match)
    let top_n = 10.min(nucleo_matches.len()).min(frizbee_strict_matches.len());
    if top_n > 0 {
        let nucleo_set: HashSet<_> = nucleo_matches.iter().take(top_n).map(|m| m.index).collect();
        let frizbee_set: HashSet<_> = frizbee_strict_matches.iter().take(top_n).map(|m| m.index).collect();

        let common_count = nucleo_set.intersection(&frizbee_set).count();
        let overlap_pct = (common_count as f64 / top_n as f64) * 100.0;

        let mut exact_pos = 0;
        for i in 0..top_n {
            if nucleo_matches[i].index == frizbee_strict_matches[i].index {
                exact_pos += 1;
            }
        }

        println!("------------------------------------------------------------");
        println!(" 3. Métricas de Concordância (Top {})", top_n);
        println!("------------------------------------------------------------");
        println!(" • Itens compartilhados no Top {}: {} de {} ({:.1}%)", top_n, common_count, top_n, overlap_pct);
        println!(" • Itens na mesma posição exata:   {} de {} ({:.1}%)\n", exact_pos, top_n, (exact_pos as f64 / top_n as f64) * 100.0);
    }
}
