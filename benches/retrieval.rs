//! Real measurements only. Use an existing index to exclude indexing from peak RSS.
use std::{collections::BTreeSet, fs, path::PathBuf, time::Instant};
use watf::{
    index, ingest,
    packet::{Engine, Options},
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
    let mut scanned = 0;
    let mut reference_hits = 0;
    let mut reference_total = 0;
    let mut route_times = Vec::new();
    let mut route_resolved = 0usize;
    let mut route_reference_matches = 0usize;
    let mut simple_routes = 0usize;
    let mut simple_route_matches = 0usize;
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
        if let Some(command) = route::classify(&packet).command {
            simple_routes += 1;
            if command == capability.command_key() {
                simple_route_matches += 1;
            }
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
            let classification = route::classify(&packet);
            route_times.push(route_start.elapsed().as_micros());
            total_bytes += json.len();
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
            "queries_per_second":times.len() as f64/elapsed_secs,"mean_packet_bytes":total_bytes/times.len(),
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
