//! Real measurements only. Use an existing index to exclude indexing from peak RSS.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    time::Instant,
};
use watf::{
    cli, index, ingest,
    packet::{Engine, Options},
    plan,
    record::Kind,
    route,
};
fn percentile(values: &[u128], p: usize) -> u128 {
    values[(values.len().saturating_sub(1) * p) / 100]
}
fn rss_kib() -> Option<usize> {
    fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find(|l| l.starts_with("VmHWM:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}
fn run() -> watf::Result<()> {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let supplied = std::env::var_os("WATF_BENCH_INDEX").map(PathBuf::from);
    let temporary = std::env::temp_dir().join(format!("watf-bench-{}.widx", std::process::id()));
    let path = supplied.as_deref().unwrap_or(&temporary);
    let mut build_stats = None;
    if supplied.is_none() {
        let mut c = ingest::builtin()?;
        c.extend(ingest::import_jsonl(&base.join("data/catalog.jsonl.gz"))?);
        build_stats = Some(index::build(path, c)?);
    }
    let start = Instant::now();
    let index = index::Index::open(path, true)?;
    let open_us = start.elapsed().as_micros();
    let records = index.len();
    let bytes = index.file_bytes();
    let mut engine = Engine::new(index);
    let cases: Vec<serde_json::Value> = include_str!("../examples/complex.jsonl")
        .lines()
        .map(serde_json::from_str)
        .collect::<std::result::Result<_, _>>()?;
    let rounds = std::env::var("WATF_BENCH_ROUNDS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(10)
        .clamp(1, 1000);
    let mut times = Vec::new();
    let mut total_bytes = 0;
    let mut total_evidence = 0usize;
    let mut truncated_packets = 0usize;
    let mut scanned = 0;
    let mut reference_hits = 0;
    let mut reference_total = 0;
    let mut route_times = Vec::new();
    let mut route_resolved = 0usize;
    let mut route_reference_matches = 0usize;
    let mut simple_routes = 0usize;
    let mut simple_route_matches = 0usize;
    let mut simple_plan_resolved = 0usize;
    let mut simple_plan_matches = 0usize;
    let mut simple_route_reasons = BTreeMap::<&'static str, usize>::new();
    let mut simple_margin_abstentions = Vec::new();
    let mut planner_expected = 0usize;
    let mut planner_current_hits = 0usize;
    let mut planner_top1_hits = 0usize;
    let mut planner_top2_hits = 0usize;
    let mut planner_context_bytes = 0usize;
    let mut planner_missing = BTreeMap::<String, usize>::new();
    let mut planner_retrieval_missing = BTreeMap::<String, usize>::new();
    let mut complex_deterministic_plans = Vec::new();
    let exact_cases = [
        "git status",
        "git status --short",
        "cargo build --release --locked",
        "docker compose up --detach --build",
        "systemctl status --no-pager",
    ];
    let mut exact_times = Vec::with_capacity(exact_cases.len() * rounds);
    let simple_cases: Vec<_> = ingest::builtin()?
        .into_iter()
        .filter(|cap| cap.kind == Kind::Command && cap.root() != "aws")
        .collect();
    for capability in &simple_cases {
        let packet = engine.lookup(
            &capability.summary,
            &Options {
                project_hints: false,
                ..Default::default()
            },
        )?;
        let classification = route::classify(&packet, &capability.summary);
        if let Some(command) = classification.command {
            simple_routes += 1;
            if command == capability.command_key() {
                simple_route_matches += 1;
            }
        } else {
            *simple_route_reasons
                .entry(classification.reason)
                .or_default() += 1;
            if classification.reason == "insufficient_margin" {
                simple_margin_abstentions.push(serde_json::json!({
                    "expected": capability.command_key(),
                    "candidates": classification.candidates.iter().take(2).map(|candidate| serde_json::json!({
                        "command": candidate.command,
                        "score": candidate.score,
                        "matched_terms": candidate.matched_terms,
                        "clause_coverage": candidate.clause_coverage,
                    })).collect::<Vec<_>>(),
                }));
            }
        }
        if let Ok(planned) = cli::plan_request(
            &mut engine,
            &capability.summary,
            &Options {
                project_hints: false,
                ..Default::default()
            },
            None,
            true,
        ) {
            simple_plan_resolved += 1;
            simple_plan_matches += usize::from(
                planned.report.steps.len() == 1
                    && planned.report.steps[0].command == capability.command_key(),
            );
        }
    }
    for _ in 0..rounds {
        for query in exact_cases {
            let start = Instant::now();
            let response = cli::plan_request(
                &mut engine,
                query,
                &Options {
                    project_hints: false,
                    ..Default::default()
                },
                None,
                true,
            )?;
            exact_times.push(start.elapsed().as_micros());
            assert!(response.report.accepted);
            assert_eq!(response.route.reason, "exact_syntax");
            assert_eq!(response.inference.generated_tokens, 0);
        }
    }
    for case in &cases {
        let query = case["intent"].as_str().unwrap();
        let options = Options {
            project_hints: false,
            catalog: case["catalog"].as_bool().unwrap_or(false),
            ..Default::default()
        };
        let _ = engine.lookup(query, &options)?;
        if let Ok(planned) = cli::plan_request(&mut engine, query, &options, None, true) {
            complex_deterministic_plans.push(serde_json::json!({
                "id": case["id"],
                "reason": planned.route.reason,
                "commands": planned.report.steps.iter().map(|step| step.command.as_str()).collect::<Vec<_>>(),
            }));
        }
        let packet = engine.lookup(
            query,
            &Options {
                max_bytes: 32768,
                limit: 32,
                ..options
            },
        )?;
        let context = plan::context(&engine.index, &packet)?;
        planner_context_bytes += serde_json::to_vec(&context)?.len();
        let retrieved: BTreeSet<_> = packet.evidence.iter().map(|e| e.command.as_str()).collect();
        let current: BTreeSet<_> = context
            .commands
            .iter()
            .map(|c| c.command.as_str())
            .collect();
        let mut per_clause = vec![Vec::<(&str, f32)>::new(); packet.clause_count];
        for evidence in &packet.evidence {
            for &clause in &evidence.clauses {
                if per_clause[clause]
                    .iter()
                    .all(|(command, _)| *command != evidence.command)
                {
                    per_clause[clause].push((&evidence.command, evidence.score));
                }
            }
        }
        for candidates in &mut per_clause {
            candidates.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(b.0)));
        }
        let top1: BTreeSet<_> = per_clause
            .iter()
            .filter_map(|candidates| candidates.first().map(|c| c.0))
            .collect();
        let top2: BTreeSet<_> = per_clause
            .iter()
            .flat_map(|candidates| candidates.iter().take(2).map(|c| c.0))
            .collect();
        for command in case["expected_commands"].as_array().unwrap() {
            let command = command.as_str().unwrap();
            planner_expected += 1;
            planner_current_hits += usize::from(current.contains(command));
            planner_top1_hits += usize::from(top1.contains(command));
            planner_top2_hits += usize::from(top2.contains(command));
            if !retrieved.contains(command) {
                *planner_retrieval_missing
                    .entry(command.to_owned())
                    .or_default() += 1;
            }
            if !current.contains(command) {
                *planner_missing.entry(command.to_owned()).or_default() += 1;
            }
        }
    }
    let elapsed = Instant::now();
    for _ in 0..rounds {
        for case in &cases {
            let query = case["intent"].as_str().unwrap();
            let options = Options {
                project_hints: false,
                catalog: case["catalog"].as_bool().unwrap_or(false),
                ..Default::default()
            };
            let start = Instant::now();
            let packet = engine.lookup(query, &options)?;
            let json = packet.json()?;
            times.push(start.elapsed().as_micros());
            let route_start = Instant::now();
            let classification = route::classify(&packet, query);
            route_times.push(route_start.elapsed().as_micros());
            total_bytes += json.len();
            total_evidence += packet.evidence.len();
            truncated_packets += usize::from(packet.truncated);
            scanned += packet.stats.postings_scanned;
            std::hint::black_box(&json);
            let scopes: BTreeSet<_> = packet.evidence.iter().map(|e| e.command.as_str()).collect();
            for command in case["expected_commands"].as_array().unwrap() {
                reference_total += 1;
                if scopes.contains(command.as_str().unwrap()) {
                    reference_hits += 1;
                }
            }
            if let Some(command) = classification.command.as_deref() {
                route_resolved += 1;
                if case["expected_commands"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|expected| expected.as_str() == Some(command))
                {
                    route_reference_matches += 1;
                }
            }
        }
    }
    let elapsed_secs = elapsed.elapsed().as_secs_f64();
    times.sort_unstable();
    route_times.sort_unstable();
    exact_times.sort_unstable();
    let planner_context = serde_json::json!({
        "reference_commands": planner_expected,
        "current_context_reference_recall": planner_current_hits as f64 / planner_expected as f64,
        "top1_per_clause_reference_recall": planner_top1_hits as f64 / planner_expected as f64,
        "top2_per_clause_reference_recall": planner_top2_hits as f64 / planner_expected as f64,
        "current_context_mean_json_bytes": planner_context_bytes / cases.len(),
        "missing_reference_command_counts": planner_missing,
        "retrieval_missing_reference_command_counts": planner_retrieval_missing,
        "simple_resolved_rate": simple_plan_resolved as f64 / simple_cases.len() as f64,
        "simple_match_when_resolved": if simple_plan_resolved == 0 { 0.0 } else { simple_plan_matches as f64 / simple_plan_resolved as f64 },
        "complex_deterministic_plans": complex_deterministic_plans,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version":1,"engine":"rust_native","version":watf::VERSION,"arch":std::env::consts::ARCH,"os":std::env::consts::OS,
            "records":records,"index_bytes":bytes,"queries":times.len(),"rounds":rounds,"open_us_page_cache_unspecified":open_us,
            "warm_p50_us":percentile(&times,50),"warm_p95_us":percentile(&times,95),"warm_p99_us":percentile(&times,99),
            "route_p50_us":percentile(&route_times,50),"route_p95_us":percentile(&route_times,95),
            "route_resolved_rate":route_resolved as f64/times.len() as f64,
            "route_reference_match_when_resolved":if route_resolved==0 {0.0} else {route_reference_matches as f64/route_resolved as f64},
            "route_self_summary_probe":true,
            "route_simple_cases":simple_cases.len(),
            "route_simple_resolved_rate":simple_routes as f64/simple_cases.len() as f64,
            "route_simple_match_when_resolved":if simple_routes==0 {0.0} else {simple_route_matches as f64/simple_routes as f64},
            "route_simple_unresolved_reasons":simple_route_reasons,
            "route_simple_margin_abstentions":simple_margin_abstentions,
            "exact_plan_cases":exact_times.len(),
            "exact_plan_p50_us":percentile(&exact_times,50),
            "exact_plan_p95_us":percentile(&exact_times,95),
            "exact_plan_model_calls":0,
            "planner_context":planner_context,
            "queries_per_second":times.len() as f64/elapsed_secs,"mean_packet_bytes":total_bytes/times.len(),
            "mean_evidence":total_evidence as f64/times.len() as f64,
            "packet_truncated_rate":truncated_packets as f64/times.len() as f64,
            "mean_posting_visits":scanned/times.len(),"reference_scope_recall":reference_hits as f64/reference_total as f64,
            "reference_scope_recall_is_not_plan_accuracy":true,"peak_rss_kib":rss_kib(),"rss_includes_index_build":supplied.is_none(),
            "model_loaded":false,"build":build_stats
        }))?
    );
    drop(engine);
    if supplied.is_none() {
        let _ = fs::remove_file(temporary);
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("benchmark: {error}");
        std::process::exit(1);
    }
}
