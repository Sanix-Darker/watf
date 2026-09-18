//! Small argument parser and interfaces. No subprocesses, HTTP clients, or shell eval.
use crate::{
    discover, execute, index, infer, ingest,
    packet::{Engine, Options, Packet},
    plan,
    record::{Capability, Kind},
    route, Error, Result, SCHEMA_VERSION, VERSION,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, BufRead, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    time::Instant,
};

pub const HELP: &str = r#"watf: what tf ?
Local CLI capability harness for AI agents with optional native planning.

  watf "stage changes, commit with message 'release', then build containers"
  watf search --json --max-bytes 4096 "restart backend and follow its logs"
  watf route --json "show current git status"
  watf plan --model /path/model.gguf --json "your complete intent"
  watf run --plan-file plan.json --json
  watf explain 'git commit -m "release"'
  watf validate --plan-file plan.json --json
  watf index [--catalog data/catalog.jsonl.gz] [--no-system]
  watf doctor [--verify] [--json]
  watf serve                 Foreground JSONL agent protocol
  watf tui                   Optional native terminal UI

General: --index FILE, --no-mmap, --json, --stats, --help, --version
Search:  --max-bytes N, --limit N, --catalog, --fields, --installed-only,
         --command 'docker compose up', --no-project-hints
Index:   --catalog FILE, --no-catalog, --no-system, --man-dir DIR,
         --man-file FILE, --help-file FILE, --doc-file FILE, --completion-file FILE,
         --completion-shell fish|bash|zsh, --command SCOPE, --import FILE
Plan:    --model FILE, --context N, --output-tokens N, --threads N,
         --allow-uninstalled
Run:     --plan-file FILE, --cwd DIR, --timeout-ms N, --max-output-bytes N
Explain: --argv-json '["git","commit","-m","release"]'

This release never downloads or executes commands at runtime. Explicit index
rebuilds write the index. Without a model, bare queries return grounded evidence,
not guessed commands.
Use -- before a query whose first word or argument looks like a watf option.
"#;

#[derive(Debug)]
struct Args {
    op: String,
    query: Vec<String>,
    index: Option<PathBuf>,
    json: bool,
    stats: bool,
    mmap: bool,
    search: Options,
    catalog_path: Option<PathBuf>,
    no_catalog: bool,
    no_system: bool,
    man_dirs: Vec<PathBuf>,
    man_files: Vec<PathBuf>,
    help_files: Vec<PathBuf>,
    doc_files: Vec<PathBuf>,
    completion_files: Vec<PathBuf>,
    completion_shell: String,
    imports: Vec<PathBuf>,
    model: Option<PathBuf>,
    context: u32,
    output_tokens: u32,
    threads: Option<i32>,
    allow_uninstalled: bool,
    verify: bool,
    plan_file: Option<PathBuf>,
    argv_json: Option<String>,
    cwd: Option<PathBuf>,
    timeout_ms: u64,
    max_output_bytes: usize,
}
impl Default for Args {
    fn default() -> Self {
        Self {
            op: "ask".to_owned(),
            query: vec![],
            index: None,
            json: false,
            stats: false,
            mmap: true,
            search: Options::default(),
            catalog_path: None,
            no_catalog: false,
            no_system: false,
            man_dirs: vec![],
            man_files: vec![],
            help_files: vec![],
            doc_files: vec![],
            completion_files: vec![],
            completion_shell: "fish".to_owned(),
            imports: vec![],
            model: std::env::var_os("WATF_MODEL").map(PathBuf::from),
            context: 4096,
            output_tokens: 768,
            threads: None,
            allow_uninstalled: false,
            verify: false,
            plan_file: None,
            argv_json: None,
            cwd: None,
            timeout_ms: 30_000,
            max_output_bytes: 4096,
        }
    }
}
fn parse(input: &[String]) -> Result<Args> {
    let mut args = Args::default();
    let mut index = 0;
    if let Some(first) = input.first() {
        if [
            "index", "search", "route", "plan", "run", "explain", "validate", "doctor", "serve",
            "tui",
        ]
        .contains(&first.as_str())
        {
            args.op = first.clone();
            index = 1;
        }
    }
    while index < input.len() {
        let key = &input[index];
        if key == "--" {
            args.query.extend_from_slice(&input[index + 1..]);
            break;
        }
        let value = |index: &mut usize| -> Result<String> {
            *index += 1;
            input
                .get(*index)
                .cloned()
                .ok_or_else(|| Error::message(format!("{key} needs a value")))
        };
        match key.as_str() {
            "--help" | "-h" => args.op = "help".to_owned(),
            "--version" | "-V" => args.op = "version".to_owned(),
            "--index" => args.index = Some(value(&mut index)?.into()),
            "--json" => args.json = true,
            "--stats" => args.stats = true,
            "--no-mmap" => args.mmap = false,
            "--max-bytes" => args.search.max_bytes = number(&value(&mut index)?, key)?,
            "--limit" => args.search.limit = number(&value(&mut index)?, key)?,
            "--catalog" if args.op == "index" => {
                args.catalog_path = Some(value(&mut index)?.into())
            }
            "--catalog" => args.search.catalog = true,
            "--fields" => args.search.fields = true,
            "--installed-only" => args.search.installed_only = true,
            "--command" => args.search.command = Some(value(&mut index)?),
            "--no-project-hints" => args.search.project_hints = false,
            "--no-system" => args.no_system = true,
            "--no-catalog" => args.no_catalog = true,
            "--man-dir" => args.man_dirs.push(value(&mut index)?.into()),
            "--man-file" => args.man_files.push(value(&mut index)?.into()),
            "--help-file" => args.help_files.push(value(&mut index)?.into()),
            "--doc-file" => args.doc_files.push(value(&mut index)?.into()),
            "--completion-file" => args.completion_files.push(value(&mut index)?.into()),
            "--completion-shell" => args.completion_shell = value(&mut index)?,
            "--import" => args.imports.push(value(&mut index)?.into()),
            "--model" => args.model = Some(value(&mut index)?.into()),
            "--context" => args.context = number(&value(&mut index)?, key)?,
            "--output-tokens" => args.output_tokens = number(&value(&mut index)?, key)?,
            "--threads" => args.threads = Some(number(&value(&mut index)?, key)?),
            "--allow-uninstalled" => args.allow_uninstalled = true,
            "--verify" => args.verify = true,
            "--plan-file" => args.plan_file = Some(value(&mut index)?.into()),
            "--argv-json" => args.argv_json = Some(value(&mut index)?),
            "--cwd" => args.cwd = Some(value(&mut index)?.into()),
            "--timeout-ms" => args.timeout_ms = number(&value(&mut index)?, key)?,
            "--max-output-bytes" => args.max_output_bytes = number(&value(&mut index)?, key)?,
            _ if key.starts_with('-') => {
                return Err(Error::message(format!(
                    "unknown option {key}; use -- before literal query arguments"
                )))
            }
            _ => args.query.push(key.clone()),
        }
        index += 1;
    }
    if args.query.iter().map(String::len).sum::<usize>() > crate::text::MAX_QUERY_BYTES {
        return Err(Error::message("query is too large"));
    }
    Ok(args)
}
fn number<T: std::str::FromStr>(value: &str, option: &str) -> Result<T> {
    value
        .parse()
        .map_err(|_| Error::message(format!("invalid numeric value for {option}")))
}
fn index_path(args: &Args) -> Result<PathBuf> {
    match &args.index {
        Some(path) => Ok(path.clone()),
        None => Ok(discover::data_dir()?.join("index.widx")),
    }
}
fn scope(args: &Args) -> Result<Vec<String>> {
    let scope = args
        .search
        .command
        .as_deref()
        .ok_or_else(|| Error::message("this import requires --command SCOPE"))?;
    let words: Vec<_> = scope.split_whitespace().map(str::to_owned).collect();
    if words.is_empty() || words.len() > 12 {
        return Err(Error::message("invalid command scope"));
    }
    Ok(words)
}
fn print_json(value: &impl Serialize) -> Result<()> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    serde_json::to_writer(&mut stdout, value)?;
    stdout.write_all(b"\n")?;
    Ok(())
}
fn print_packet(packet: &Packet, json: bool) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    if json {
        out.write_all(&packet.json()?)?;
        return Ok(());
    }
    writeln!(
        out,
        "{}: {} evidence records, {} bytes",
        packet.status,
        packet.evidence.len(),
        packet.json()?.len()
    )?;
    for evidence in &packet.evidence {
        writeln!(
            out,
            "\n{}  {}{}",
            crate::text::compact(&evidence.command, 160),
            crate::text::compact(&evidence.name, 160),
            if evidence.program_available {
                ""
            } else {
                " [program not detected]"
            }
        )?;
        writeln!(out, "  {}", evidence.summary)?;
        if let Some(source) = packet.sources.get(evidence.source) {
            writeln!(
                out,
                "  source: {}",
                crate::text::compact(&source.source.reference, 240)
            )?;
        }
    }
    if !packet.uncovered_clauses.is_empty() {
        writeln!(
            out,
            "\nUncovered clause indexes: {:?}. Narrow the query or import documentation.",
            packet.uncovered_clauses
        )?;
    }
    Ok(())
}
fn index_command(args: &Args, path: &Path) -> Result<()> {
    let start = Instant::now();
    let mut records = ingest::builtin()?;
    if !args.no_catalog {
        let default = discover::data_dir()?.join("catalog.jsonl.gz");
        if let Some(catalog) = args
            .catalog_path
            .as_ref()
            .or_else(|| default.is_file().then_some(&default))
        {
            records.extend(ingest::import_jsonl(catalog)?);
        }
    }
    for path in &args.imports {
        records.extend(ingest::import_jsonl(path)?);
    }
    let command = if args.search.command.is_some() {
        Some(scope(args)?)
    } else {
        None
    };
    for path in &args.man_files {
        records.extend(ingest::man::read(path, command.as_deref())?);
    }
    for path in &args.help_files {
        records.extend(ingest::help::read(path, &scope(args)?)?);
    }
    for path in &args.doc_files {
        records.extend(ingest::docs::read(path, &scope(args)?)?);
    }
    for path in &args.completion_files {
        records.extend(ingest::completion::read(
            path,
            &scope(args)?,
            &args.completion_shell,
        )?);
    }
    let mut directories = args.man_dirs.clone();
    if !args.no_system {
        directories.extend([
            PathBuf::from("/usr/share/man"),
            PathBuf::from("/usr/local/share/man"),
        ]);
    }
    let (local, scan) = ingest::scan_man_dirs(&directories)?;
    records.extend(local);
    let result = index::build(path, records)?;
    if args.json {
        print_json(
            &serde_json::json!({"schema_version":SCHEMA_VERSION,"index":path,"build":result,"scan":scan,"elapsed_ms":start.elapsed().as_millis()}),
        )?;
    } else {
        eprintln!(
            "indexed {} records, {} terms, {} bytes in {} ms",
            result.records,
            result.terms,
            result.bytes,
            start.elapsed().as_millis()
        );
        for warning in scan.warnings {
            eprintln!("skipped: {}", crate::text::compact(&warning, 300));
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct PlanResponse {
    #[serde(flatten)]
    pub report: plan::Report,
    pub inference: infer::Metrics,
    pub route: route::Classification,
}
pub fn plan_request(
    engine: &mut Engine,
    query: &str,
    search: &Options,
    model: &infer::Options,
    allow_uninstalled: bool,
) -> Result<PlanResponse> {
    let mut retrieval = search.clone();
    retrieval.max_bytes = 32768;
    retrieval.limit = 32;
    let packet = engine.lookup(query, &retrieval)?;
    let route = route::classify(&packet);
    let context = plan::context_for(&engine.index, &packet, route.command.as_deref())?;
    let generated = infer::generate(query, &context, model)?;
    let allowed_commands = Some(context.commands.iter().map(|c| c.command.clone()).collect());
    let allowed_flags = Some(
        context
            .commands
            .iter()
            .map(|c| {
                (
                    c.command.clone(),
                    c.options.iter().map(|o| o.name.clone()).collect(),
                )
            })
            .collect(),
    );
    let report = plan::validate(
        &engine.index,
        &generated.draft,
        &engine.inventory,
        &plan::ValidationOptions {
            allow_uninstalled,
            allowed_commands,
            allowed_flags,
        },
    )?;
    Ok(PlanResponse {
        report,
        inference: generated.metrics,
        route,
    })
}
fn model_options(args: &Args) -> Result<infer::Options> {
    let path = args
        .model
        .clone()
        .ok_or_else(|| Error::message("set --model or WATF_MODEL to a local Qwen3 GGUF file"))?;
    let mut options = infer::Options::new(path);
    options.context_tokens = args.context;
    options.output_tokens = args.output_tokens;
    if let Some(threads) = args.threads {
        options.threads = threads;
    }
    Ok(options)
}
fn print_report(report: &plan::Report) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    if let Some(shell) = &report.shell {
        writeln!(out, "{shell}")?;
    }
    for error in &report.errors {
        writeln!(out, "Rejected: {}", crate::text::compact(error, 400))?;
    }
    for question in &report.questions {
        writeln!(out, "Question: {}", crate::text::compact(question, 400))?;
    }
    for warning in &report.warnings {
        writeln!(out, "Review: {}", crate::text::compact(warning, 400))?;
    }
    Ok(())
}
fn read_plan(path: &Path) -> Result<plan::Draft> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(128 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    plan::Draft::parse(&bytes)
}

/// A literal argv explanation is intentionally not a general shell interpreter.
fn explain(engine: &Engine, args: &Args, query: &str) -> Result<()> {
    let words: Vec<String> = if let Some(json) = &args.argv_json {
        serde_json::from_str(json)?
    } else {
        if query
            .chars()
            .any(|c| matches!(c, '$' | '`' | ';' | '|' | '<' | '>' | '&' | '\n'))
        {
            return Err(Error::message("explain accepts a single literal command, not shell programs; use --argv-json for literal special characters"));
        }
        ingest::completion::words(query)?
    };
    if words.is_empty() || words.len() > 76 {
        return Err(Error::message("explain needs 1..76 literal argv elements"));
    }
    let mut found = None;
    for count in (1..=words.len().min(12)).rev() {
        let key = words[..count].join(" ");
        let ids = engine.index.ids_for_command(&key)?;
        if ids.iter().any(|&id| {
            engine
                .index
                .capability(id)
                .is_ok_and(|c| c.kind == Kind::Command)
        }) {
            found = Some((key, count));
            break;
        }
    }
    let (command, count) = found.ok_or_else(|| Error::message("command scope not indexed"))?;
    let draft = plan::Draft {
        status: plan::Status::Ok,
        steps: vec![plan::Step {
            command: command.clone(),
            args: words[count..].to_vec(),
            after: plan::After::Start,
            stdout: None,
        }],
        questions: vec![],
    };
    let report = plan::validate(
        &engine.index,
        &draft,
        &engine.inventory,
        &plan::ValidationOptions {
            allow_uninstalled: true,
            ..Default::default()
        },
    )?;
    let selected: BTreeSet<_> = report
        .evidence
        .iter()
        .flat_map(|e| e.ids.iter().cloned())
        .collect();
    let evidence: Vec<Capability> = engine
        .index
        .ids_for_command(&command)?
        .into_iter()
        .map(|id| engine.index.capability(id))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|r| selected.contains(&r.id))
        .collect();
    if args.json {
        print_json(
            &serde_json::json!({"schema_version":SCHEMA_VERSION,"mode":"explain","argv":words,"evidence":evidence,"validation":report}),
        )?;
    } else {
        for cap in evidence {
            println!(
                "{}: {}\n  source: {}",
                crate::text::compact(&cap.name, 160),
                crate::text::compact(&cap.summary, 500),
                crate::text::compact(&cap.source.reference, 240)
            );
        }
        for error in report.errors {
            eprintln!("unverified: {}", crate::text::compact(&error, 400));
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    #[serde(default = "schema_one")]
    schema_version: u32,
    id: String,
    #[serde(default = "search_op")]
    op: String,
    query: String,
    #[serde(default = "default_bytes")]
    max_bytes: usize,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    catalog: bool,
    #[serde(default)]
    fields: bool,
    #[serde(default)]
    installed_only: bool,
    #[serde(default)]
    command: Option<String>,
}
fn schema_one() -> u32 {
    1
}
fn search_op() -> String {
    "search".to_owned()
}
fn default_bytes() -> usize {
    4096
}
fn default_limit() -> usize {
    16
}

/// Persistent foreground engine. stdin/stdout only, no sockets or daemonization.
pub fn serve<R: BufRead, W: Write>(engine: &mut Engine, mut input: R, mut output: W) -> Result<()> {
    while let Some(line) = ingest::bounded_line(&mut input, 65536)? {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let response = (|| -> Result<Vec<u8>> {
            let request: Request = serde_json::from_slice(&line)?;
            if request.schema_version != 1 || request.op != "search" || request.id.len() > 64 {
                return Err(Error::message(
                    "unsupported schema/op or request id longer than 64 bytes",
                ));
            }
            let options = Options {
                max_bytes: request.max_bytes,
                limit: request.limit,
                catalog: request.catalog,
                fields: request.fields,
                installed_only: request.installed_only,
                command: request.command,
                project_hints: false,
            };
            let mut packet = engine.lookup(&request.query, &options)?;
            let extra = serde_json::to_vec(&request.id)?.len() + 6;
            // Reserve a few bytes for the full-envelope token estimate changing width.
            packet.fit(request.max_bytes.saturating_sub(extra + 8))?;
            #[derive(Serialize)]
            struct Response<'a> {
                id: &'a str,
                #[serde(flatten)]
                packet: Packet,
            }
            let mut response = Response {
                id: &request.id,
                packet,
            };
            for _ in 0..4 {
                response.packet.estimated_tokens =
                    (serde_json::to_vec(&response)?.len() + 1).div_ceil(4);
            }
            let mut bytes = serde_json::to_vec(&response)?;
            bytes.push(b'\n');
            if bytes.len() > request.max_bytes {
                return Err(Error::message("response budget too small"));
            }
            Ok(bytes)
        })();
        match response {
            Ok(bytes) => output.write_all(&bytes)?,
            Err(error) => {
                serde_json::to_writer(
                    &mut output,
                    &serde_json::json!({"schema_version":1,"status":"error","error":crate::text::compact_bytes(&error.to_string(),96)}),
                )?;
                output.write_all(b"\n")?;
            }
        }
        output.flush()?;
    }
    Ok(())
}

pub fn run(input: Vec<String>) -> Result<i32> {
    let args = parse(&input)?;
    if args.op == "help" {
        print!("{HELP}");
        return Ok(0);
    }
    if args.op == "version" {
        println!("watf {VERSION}");
        return Ok(0);
    }
    if input.is_empty() && (!cfg!(feature = "tui") || !io::stdin().is_terminal()) {
        print!("{HELP}");
        return Ok(0);
    }
    let path = index_path(&args)?;
    if args.op == "index" {
        index_command(&args, &path)?;
        return Ok(0);
    }
    let mut engine = Engine::open(&path, args.mmap)?;
    let query = args.query.join(" ");
    let start = Instant::now();
    let status = match args.op.as_str() {
        "doctor" => {
            if args.verify {
                engine.index.verify()?;
            }
            let info = serde_json::json!({"schema_version":SCHEMA_VERSION,"version":VERSION,"index":path,
                "records":engine.index.len(),"terms":engine.index.term_count(),"index_bytes":engine.index.file_bytes(),
                "installed_programs":engine.inventory.len(),"integrity_verified":args.verify,
                "local_llm_compiled":cfg!(feature="local-llm"),"tui_compiled":cfg!(feature="tui"),
                "model_exists":args.model.as_ref().is_some_and(|p|p.is_file()),"network_client":false,"executor":true});
            if args.json {
                print_json(&info)?;
            } else {
                println!("{}", serde_json::to_string_pretty(&info)?);
            }
            0
        }
        "serve" => {
            serve(&mut engine, io::stdin().lock(), io::stdout().lock())?;
            0
        }
        "tui" => {
            tui(&mut engine, &args)?;
            0
        }
        "validate" => {
            let file = args
                .plan_file
                .as_ref()
                .ok_or_else(|| Error::message("validate needs --plan-file"))?;
            let report = plan::validate(
                &engine.index,
                &read_plan(file)?,
                &engine.inventory,
                &plan::ValidationOptions {
                    allow_uninstalled: args.allow_uninstalled,
                    ..Default::default()
                },
            )?;
            if args.json {
                print_json(&report)?;
            } else {
                print_report(&report)?;
            }
            if report.accepted {
                0
            } else {
                3
            }
        }
        "run" => {
            let file = args
                .plan_file
                .as_ref()
                .ok_or_else(|| Error::message("run needs --plan-file"))?;
            let draft = read_plan(file)?;
            let report = plan::validate(
                &engine.index,
                &draft,
                &engine.inventory,
                &plan::ValidationOptions {
                    allow_uninstalled: false,
                    ..Default::default()
                },
            )?;
            if !report.accepted {
                if args.json {
                    print_json(&report)?;
                } else {
                    print_report(&report)?;
                }
                3
            } else {
                let cwd = match &args.cwd {
                    Some(path) => path.clone(),
                    None => std::env::current_dir()?,
                };
                let execution = execute::execute(
                    &report,
                    &engine.inventory,
                    &execute::Options {
                        cwd,
                        timeout_ms: args.timeout_ms,
                        max_output_bytes: args.max_output_bytes,
                    },
                )?;
                #[derive(Serialize)]
                struct RunResponse<'a> {
                    schema_version: u32,
                    validation: &'a plan::Report,
                    execution: &'a execute::Execution,
                }
                if args.json {
                    print_json(&RunResponse {
                        schema_version: SCHEMA_VERSION,
                        validation: &report,
                        execution: &execution,
                    })?;
                } else {
                    println!("{}", serde_json::to_string_pretty(&execution)?);
                }
                if execution.status == "ok" {
                    0
                } else {
                    3
                }
            }
        }
        "explain" => {
            explain(&engine, &args, &query)?;
            0
        }
        "route" => {
            let packet = engine.lookup(&query, &args.search)?;
            let classification = route::classify(&packet);
            if args.json {
                print_json(&classification)?;
            } else {
                println!("{}", serde_json::to_string_pretty(&classification)?);
            }
            if classification.command.is_some() {
                0
            } else {
                3
            }
        }
        "plan" => {
            let response = plan_request(
                &mut engine,
                &query,
                &args.search,
                &model_options(&args)?,
                args.allow_uninstalled,
            )?;
            if args.json {
                print_json(&response)?;
            } else {
                print_report(&response.report)?;
            }
            if response.report.accepted {
                0
            } else {
                3
            }
        }
        "ask" if query.is_empty() => {
            tui(&mut engine, &args)?;
            0
        }
        "ask" if args.model.is_some() && cfg!(feature = "local-llm") => {
            let response = plan_request(
                &mut engine,
                &query,
                &args.search,
                &model_options(&args)?,
                args.allow_uninstalled,
            )?;
            if args.json {
                print_json(&response)?;
            } else {
                print_report(&response.report)?;
            }
            if response.report.accepted {
                0
            } else {
                3
            }
        }
        "ask" | "search" => {
            let packet = engine.lookup(&query, &args.search)?;
            if args.op == "ask" && !args.json {
                eprintln!("No local planner loaded. Returning documentation evidence, not a synthesized command.");
            }
            print_packet(&packet, args.json)?;
            if args.stats {
                eprintln!("{}", serde_json::to_string(&packet.stats)?);
            }
            if packet.evidence.is_empty() {
                3
            } else {
                0
            }
        }
        _ => return Err(Error::message("unknown operation")),
    };
    if args.stats {
        eprintln!("request_ms={}", start.elapsed().as_millis());
    }
    Ok(status)
}
fn tui(engine: &mut Engine, args: &Args) -> Result<()> {
    #[cfg(feature = "tui")]
    {
        let model = if args.model.is_some() {
            Some(model_options(args)?)
        } else {
            None
        };
        crate::tui::run(engine, &args.search, model.as_ref())
    }
    #[cfg(not(feature = "tui"))]
    {
        let _ = (engine, args);
        Err(Error::message(
            "build with --features tui for the terminal interface",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bare_intent_needs_no_command() {
        let a = parse(&["stage changes then start containers".into()]).unwrap();
        assert_eq!(a.op, "ask");
        assert!(a.search.command.is_none());
    }
    #[test]
    fn catalog_path_only_for_index() {
        let a = parse(&["index".into(), "--catalog".into(), "file.gz".into()]).unwrap();
        assert_eq!(a.catalog_path, Some("file.gz".into()));
    }
    #[test]
    fn unknown_options_are_errors() {
        assert!(parse(&["--execute".into()]).is_err());
    }
    #[test]
    fn literal_separator() {
        let a = parse(&["search".into(), "--".into(), "--weird".into()]).unwrap();
        assert_eq!(a.query, ["--weird"]);
    }
}
