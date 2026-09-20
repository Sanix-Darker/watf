//! Small argument parser and interfaces. No subprocesses, HTTP clients, or shell eval.
use crate::{
    discover, execute, index, infer, ingest,
    packet::{Engine, Options, Packet},
    plan,
    record::{Arity, Capability, Kind},
    route, Error, Result, SCHEMA_VERSION, VERSION,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
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
  watf exec --json -- git status --short
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
Exec:    --cwd DIR, --timeout-ms N, --max-output-bytes N, --raw-output-dir DIR
Run:     --plan-file FILE, --cwd DIR, --timeout-ms N, --max-output-bytes N,
         --raw-output-dir DIR
Explain: --argv-json '["git","commit","-m","release"]'

This release never downloads models at runtime. Explicit index rebuilds write the
index. `run` executes only validated structured argv. Without a model, bare
queries return grounded evidence, while explicit indexed command syntax can be
planned deterministically.
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
    raw_output_dir: Option<PathBuf>,
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
            raw_output_dir: None,
        }
    }
}
fn parse(input: &[String]) -> Result<Args> {
    let mut args = Args::default();
    let mut index = 0;
    if let Some(first) = input.first() {
        if [
            "index", "search", "route", "plan", "exec", "run", "explain", "validate", "doctor",
            "serve", "tui",
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
            "--raw-output-dir" => args.raw_output_dir = Some(value(&mut index)?.into()),
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

enum PlanAttempt {
    Planned(Box<PlanResponse>),
    NeedsPlanning {
        route: route::Classification,
        clause_count: usize,
        uncovered_clauses: Vec<usize>,
    },
}

const MODEL_REQUIRED_ERROR: &str =
    "planning needs a local model unless the request is explicit indexed command syntax";
fn explicit_plan(
    engine: &Engine,
    query: &str,
    allow_positionals: bool,
) -> Result<Option<(plan::Draft, BTreeSet<String>)>> {
    if query.is_empty()
        || query
            .chars()
            .any(|c| matches!(c, '$' | '`' | ';' | '|' | '<' | '>' | '&' | '\n'))
    {
        return Ok(None);
    }
    let words = ingest::completion::words(query)?;
    explicit_argv_plan(engine, &words, allow_positionals)
}

fn explicit_argv_plan(
    engine: &Engine,
    words: &[String],
    allow_positionals: bool,
) -> Result<Option<(plan::Draft, BTreeSet<String>)>> {
    if words.is_empty() || words.len() > 76 {
        return Ok(None);
    }
    let Some((command, count)) = indexed_command_prefix(engine, words)? else {
        return Ok(None);
    };
    let args = &words[count..];
    let Some(flags) = explicit_option_args(engine, &command, args, allow_positionals)? else {
        return Ok(None);
    };
    Ok(Some((
        plan::Draft {
            status: plan::Status::Ok,
            steps: vec![plan::Step {
                command,
                args: args.to_vec(),
                after: plan::After::Start,
                stdout: None,
            }],
            questions: vec![],
        },
        flags,
    )))
}

fn indexed_command_prefix(engine: &Engine, words: &[String]) -> Result<Option<(String, usize)>> {
    for count in (1..=words.len().min(12)).rev() {
        let command = words[..count].join(" ");
        let ids = engine.index.ids_for_command(&command)?;
        if ids.iter().any(|&id| {
            engine
                .index
                .capability(id)
                .is_ok_and(|cap| cap.kind == Kind::Command)
        }) {
            return Ok(Some((command, count)));
        }
    }
    Ok(None)
}

fn explicit_option_args(
    engine: &Engine,
    command: &str,
    args: &[String],
    allow_positionals: bool,
) -> Result<Option<BTreeSet<String>>> {
    use crate::record::Arity;
    let caps = engine
        .index
        .ids_for_command(command)?
        .into_iter()
        .map(|id| engine.index.capability(id))
        .collect::<Result<Vec<_>>>()?;
    let allowed = caps
        .iter()
        .filter(|cap| cap.kind == Kind::Option)
        .map(|cap| cap.name.clone())
        .collect();
    if args.is_empty() {
        return Ok(Some(allowed));
    }
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" {
            return Ok(allow_positionals.then_some(allowed));
        }
        if !arg.starts_with('-') || arg == "-" {
            if allow_positionals {
                i += 1;
                continue;
            }
            return Ok(None);
        }
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), false), |(name, _)| (name, true));
        let Some(cap) = caps
            .iter()
            .find(|cap| cap.kind == Kind::Option && cap.matches_flag(name))
        else {
            return Ok(None);
        };
        let values = match cap.arity {
            Arity::None if !inline => 0,
            Arity::One => usize::from(!inline),
            Arity::Two => 2 - usize::from(inline),
            Arity::Optional => 0,
            _ => return Ok(None),
        };
        if i + values >= args.len() {
            return Ok(None);
        }
        i += values + 1;
    }
    Ok(Some(allowed))
}

pub fn plan_request(
    engine: &mut Engine,
    query: &str,
    search: &Options,
    model: Option<&infer::Options>,
    allow_uninstalled: bool,
) -> Result<PlanResponse> {
    match plan_attempt(engine, query, search, model, allow_uninstalled)? {
        PlanAttempt::Planned(response) => Ok(*response),
        PlanAttempt::NeedsPlanning { .. } => Err(Error::message(MODEL_REQUIRED_ERROR)),
    }
}

fn plan_attempt(
    engine: &mut Engine,
    query: &str,
    search: &Options,
    model: Option<&infer::Options>,
    allow_uninstalled: bool,
) -> Result<PlanAttempt> {
    if let Some(response) = exact_plan_request(engine, query, allow_uninstalled, false)? {
        return Ok(PlanAttempt::Planned(Box::new(response)));
    }
    let mut retrieval = search.clone();
    retrieval.max_bytes = 32768;
    retrieval.limit = 32;
    let packet = engine.lookup(query, &retrieval)?;
    let route = route::classify(&packet, query);
    if let Some(response) = routed_plan_request(
        engine,
        query,
        &route,
        &packet,
        allow_uninstalled,
        matches!(route.reason, "retrieval_margin" | "explicit_scope_terms")
            && crate::text::clauses(query).len() == 1,
    )? {
        return Ok(PlanAttempt::Planned(Box::new(response)));
    }
    if route.reason == "multi_clause_requires_planning" {
        if let Some(command) = route::decisive_command(&route, packet.clause_count) {
            let clauses = crate::text::clauses(query);
            let clause_routes = clauses
                .iter()
                .enumerate()
                .map(|(index, clause)| {
                    let mut local = packet.clone();
                    local
                        .evidence
                        .retain(|evidence| evidence.clauses.contains(&index));
                    for evidence in &mut local.evidence {
                        evidence.clauses.clear();
                        evidence.clauses.push(0);
                    }
                    local.clause_count = 1;
                    local.uncovered_clauses = if local.evidence.is_empty() {
                        vec![0]
                    } else {
                        vec![]
                    };
                    route::classify(&local, clause)
                })
                .collect::<Vec<_>>();
            if clause_routes.iter().all(|classification| {
                classification.status == "resolved"
                    && classification.command.as_deref() == Some(command)
            }) {
                let single_scope = route::Classification {
                    status: "resolved",
                    command: Some(command.to_owned()),
                    candidates: route.candidates.clone(),
                    reason: "grounded_single_scope",
                };
                if let Some(response) = routed_plan_request(
                    engine,
                    query,
                    &single_scope,
                    &packet,
                    allow_uninstalled,
                    false,
                )? {
                    return Ok(PlanAttempt::Planned(Box::new(response)));
                }
            }
        }
        if let Some(response) =
            grounded_multi_scope_plan(engine, query, &packet, allow_uninstalled)?
        {
            return Ok(PlanAttempt::Planned(Box::new(response)));
        }
    }
    let mut synthesis_commands: BTreeSet<_> = route
        .candidates
        .iter()
        .filter(|candidate| candidate.matched_terms >= 2)
        .map(|candidate| candidate.command.clone())
        .collect();
    if synthesis_commands.is_empty() {
        if let Some(candidate) = route.candidates.first() {
            synthesis_commands.insert(candidate.command.clone());
        }
    }
    let mut synthesis_packet = packet.clone();
    synthesis_packet
        .evidence
        .retain(|evidence| synthesis_commands.contains(&evidence.command));
    synthesis_packet.uncovered_clauses = (0..synthesis_packet.clause_count)
        .filter(|clause| {
            !synthesis_packet
                .evidence
                .iter()
                .any(|evidence| evidence.clauses.contains(clause))
        })
        .collect();
    let Some(model) = model else {
        return Ok(PlanAttempt::NeedsPlanning {
            route,
            clause_count: synthesis_packet.clause_count,
            uncovered_clauses: synthesis_packet.uncovered_clauses,
        });
    };
    let context = plan::context_for(&engine.index, &synthesis_packet, route.command.as_deref())?;
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
    let command_clauses = Some(synthesis_packet.evidence.iter().fold(
        BTreeMap::<String, BTreeSet<usize>>::new(),
        |mut map, evidence| {
            map.entry(evidence.command.clone())
                .or_default()
                .extend(evidence.clauses.iter().copied());
            map
        },
    ));
    let report = plan::validate(
        &engine.index,
        &generated.draft,
        &engine.inventory,
        &plan::ValidationOptions {
            allow_uninstalled,
            allowed_commands,
            allowed_flags,
            required_flags: model_required_flags(query, &context),
            command_clauses,
            required_literals: model_required_literals(query, &context),
        },
    )?;
    Ok(PlanAttempt::Planned(Box::new(PlanResponse {
        report,
        inference: generated.metrics,
        route,
    })))
}

fn quoted_literals(query: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut quote = None;
    let mut start = 0;
    let mut escaped = false;
    for (index, ch) in query.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(delimiter) = quote {
            if ch == delimiter {
                if index > start {
                    out.insert(query[start..index].to_owned());
                }
                quote = None;
            }
        } else if ch == '\'' || ch == '"' {
            quote = Some(ch);
            start = index + ch.len_utf8();
        }
    }
    out
}

fn model_required_literals(query: &str, context: &plan::Context) -> BTreeSet<String> {
    let mut required = quoted_literals(query);
    let mut grounded = BTreeSet::new();
    for command in &context.commands {
        grounded.extend(crate::text::unique_terms(&command.command));
        grounded.extend(crate::text::unique_terms(&command.summary));
        for option in &command.options {
            grounded.extend(crate::text::unique_terms(&option.name));
            grounded.extend(crate::text::unique_terms(&option.description));
            for alias in &option.aliases {
                grounded.extend(crate::text::unique_terms(alias));
            }
        }
    }
    for raw in query.split_whitespace() {
        if raw.contains(['\'', '"']) {
            continue;
        }
        let word = raw.trim_matches(|c: char| matches!(c, ',' | ';' | ':' | '(' | ')' | '[' | ']'));
        if word.is_empty() {
            continue;
        }
        if [
            "check", "current", "display", "get", "its", "please", "print", "run", "show",
        ]
        .iter()
        .any(|generic| word.eq_ignore_ascii_case(generic))
        {
            continue;
        }
        let terms = crate::text::tokens(word);
        if terms.is_empty() {
            continue;
        }
        let explained = crate::text::query_terms(word)
            .keys()
            .any(|term| grounded.contains(term));
        if !explained {
            required.insert(word.to_owned());
        }
    }
    required
}

fn model_required_flags(
    query: &str,
    context: &plan::Context,
) -> BTreeMap<String, BTreeSet<String>> {
    let command_terms = context
        .commands
        .iter()
        .flat_map(|command| {
            crate::text::unique_terms(&command.command)
                .into_iter()
                .chain(crate::text::unique_terms(&command.summary))
        })
        .collect::<BTreeSet<_>>();
    let mut required = BTreeMap::<String, BTreeSet<String>>::new();
    for term in crate::text::tokens(query) {
        if command_terms.contains(&term) {
            continue;
        }
        let mut matches = Vec::new();
        for command in &context.commands {
            for option in &command.options {
                let mut terms = crate::text::unique_terms(&option.name);
                terms.extend(crate::text::unique_terms(&option.description));
                for alias in &option.aliases {
                    terms.extend(crate::text::unique_terms(alias));
                }
                if terms.contains(&term) {
                    matches.push((&command.command, &option.name));
                }
            }
        }
        matches.sort_unstable();
        matches.dedup();
        if matches.len() == 1 {
            let (command, flag) = matches[0];
            required
                .entry(command.clone())
                .or_default()
                .insert(flag.clone());
        }
    }
    required
}

fn grounded_multi_scope_plan(
    engine: &Engine,
    query: &str,
    packet: &Packet,
    allow_uninstalled: bool,
) -> Result<Option<PlanResponse>> {
    let clauses = crate::text::clauses(query);
    if clauses.len() < 2 || clauses.len() > plan::MAX_STEPS {
        return Ok(None);
    }
    let mut steps = Vec::with_capacity(clauses.len());
    let mut candidates = Vec::with_capacity(clauses.len());
    for (clause_index, clause) in clauses.iter().enumerate() {
        let mut local = packet.clone();
        local.evidence.retain(|e| e.clauses.contains(&clause_index));
        for evidence in &mut local.evidence {
            evidence.clauses.clear();
            evidence.clauses.push(0);
        }
        local.clause_count = 1;
        local.uncovered_clauses = if local.evidence.is_empty() {
            vec![0]
        } else {
            vec![]
        };
        let classification = route::classify(&local, clause);
        let Some(response) = routed_plan_request(
            engine,
            clause,
            &classification,
            &local,
            allow_uninstalled,
            false,
        )?
        else {
            return Ok(None);
        };
        if !response.report.accepted || response.report.steps.len() != 1 {
            return Ok(None);
        }
        let mut step = response.report.steps[0].clone();
        if !step.args.is_empty() || step.stdout.is_some() {
            return Ok(None);
        }
        step.after = if steps.is_empty() {
            plan::After::Start
        } else {
            plan::After::Success
        };
        steps.push(step);
        if let Some(candidate) = classification.candidates.first() {
            candidates.push(candidate.clone());
        }
    }
    let allowed_commands = steps.iter().map(|step| step.command.clone()).collect();
    let allowed_flags = steps
        .iter()
        .map(|step| (step.command.clone(), BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    let draft = plan::Draft {
        status: plan::Status::Ok,
        steps,
        questions: vec![],
    };
    let report = plan::validate(
        &engine.index,
        &draft,
        &engine.inventory,
        &plan::ValidationOptions {
            allow_uninstalled,
            allowed_commands: Some(allowed_commands),
            allowed_flags: Some(allowed_flags),
            ..Default::default()
        },
    )?;
    Ok(report.accepted.then_some(PlanResponse {
        report,
        inference: infer::Metrics::default(),
        route: route::Classification {
            status: "resolved",
            command: None,
            candidates,
            reason: "grounded_multi_scope",
        },
    }))
}

fn routed_plan_request(
    engine: &Engine,
    query: &str,
    route: &route::Classification,
    packet: &Packet,
    allow_uninstalled: bool,
    allow_no_value_options: bool,
) -> Result<Option<PlanResponse>> {
    let Some(command) = route.command.as_deref() else {
        return Ok(None);
    };
    let Some(scope) = packet
        .evidence
        .iter()
        .find(|e| e.command == command && e.kind == Kind::Command && e.clauses.contains(&0))
    else {
        return Ok(None);
    };
    let mut grounded = crate::text::unique_terms(command);
    grounded.extend(crate::text::unique_terms(&scope.name));
    grounded.extend(crate::text::unique_terms(&scope.summary));
    for alias in &scope.aliases {
        grounded.extend(crate::text::unique_terms(alias));
    }
    let generic = route::GENERIC_INTENT_TERMS;
    let args = if allow_no_value_options {
        if routed_option_negated(query) {
            return Ok(None);
        }
        let Some(args) =
            routed_no_value_option_args(engine, query, command, packet, &grounded, generic)?
        else {
            return Ok(None);
        };
        args
    } else if crate::text::unique_terms(query)
        .iter()
        .all(|term| grounded.contains(term) || generic.contains(&term.as_str()))
    {
        vec![]
    } else {
        return Ok(None);
    };
    let selected_flags = args.iter().cloned().collect::<BTreeSet<_>>();
    let draft = plan::Draft {
        status: plan::Status::Ok,
        steps: vec![plan::Step {
            command: command.to_owned(),
            args,
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
            allow_uninstalled,
            allowed_commands: Some([command.to_owned()].into_iter().collect()),
            allowed_flags: Some(
                [(command.to_owned(), selected_flags.clone())]
                    .into_iter()
                    .collect(),
            ),
            required_flags: [(command.to_owned(), selected_flags)].into_iter().collect(),
            command_clauses: Some([(command.to_owned(), [0].into_iter().collect())].into()),
            ..Default::default()
        },
    )?;
    Ok(Some(PlanResponse {
        report,
        inference: infer::Metrics::default(),
        route: route.clone(),
    }))
}

fn routed_option_negated(query: &str) -> bool {
    crate::text::tokens(query)
        .iter()
        .any(|term| matches!(term.as_str(), "no" | "not" | "without"))
}

fn routed_no_value_option_args(
    engine: &Engine,
    query: &str,
    command: &str,
    packet: &Packet,
    grounded: &BTreeSet<String>,
    generic: &[&str],
) -> Result<Option<Vec<String>>> {
    let query_tokens = crate::text::tokens(query);
    let query_terms = query_tokens.iter().cloned().collect::<BTreeSet<_>>();
    let mut canonical_matches = BTreeMap::<String, (Arity, BTreeSet<String>)>::new();
    for evidence in packet
        .evidence
        .iter()
        .filter(|e| e.command == command && e.kind == Kind::Option && e.clauses.contains(&0))
    {
        let name_terms = crate::text::tokens(&evidence.name)
            .into_iter()
            .filter(|term| !term.starts_with('='))
            .collect::<BTreeSet<_>>();
        if name_terms.len() < 2 || !name_terms.iter().all(|term| query_terms.contains(term)) {
            continue;
        }
        let cap = engine.index.capability(evidence.doc)?;
        if cap.kind != Kind::Option || cap.command_key() != command {
            return Ok(None);
        }
        canonical_matches.insert(cap.name, (cap.arity, name_terms));
    }
    if canonical_matches.len() > 1 {
        return Ok(None);
    }
    let canonical_match = canonical_matches.into_iter().next();
    if canonical_match
        .as_ref()
        .is_some_and(|(_, (arity, _))| *arity != Arity::None)
    {
        return Ok(None);
    }

    let mut args = Vec::new();
    let mut seen = BTreeSet::new();
    for term in query_tokens {
        if let Some((name, (_, terms))) = &canonical_match {
            if terms.contains(&term) {
                if seen.insert(name.clone()) {
                    args.push(name.clone());
                }
                continue;
            }
        }
        if grounded.contains(&term) || generic.contains(&term.as_str()) {
            continue;
        }
        let mut matches = BTreeMap::<String, Arity>::new();
        for evidence in packet
            .evidence
            .iter()
            .filter(|e| e.command == command && e.kind == Kind::Option && e.clauses.contains(&0))
        {
            let mut terms = crate::text::unique_terms(&evidence.name);
            terms.extend(crate::text::unique_terms(&evidence.summary));
            for alias in &evidence.aliases {
                terms.extend(crate::text::unique_terms(alias));
            }
            if terms.contains(&term) {
                let cap = engine.index.capability(evidence.doc)?;
                if cap.kind != Kind::Option || cap.command_key() != command {
                    return Ok(None);
                }
                matches.insert(cap.name, cap.arity);
            }
        }
        if matches.len() != 1 {
            return Ok(None);
        }
        let (name, arity) = matches.into_iter().next().unwrap();
        if arity != Arity::None {
            return Ok(None);
        }
        if seen.insert(name.clone()) {
            args.push(name);
        }
    }
    Ok(Some(args))
}

fn exact_plan_request(
    engine: &Engine,
    query: &str,
    allow_uninstalled: bool,
    allow_positionals: bool,
) -> Result<Option<PlanResponse>> {
    let Some((draft, flags)) = explicit_plan(engine, query, allow_positionals)? else {
        return Ok(None);
    };
    exact_draft_response(engine, draft, flags, allow_uninstalled).map(Some)
}

fn exact_argv_plan_request(
    engine: &Engine,
    argv: &[String],
    allow_uninstalled: bool,
) -> Result<Option<PlanResponse>> {
    if argv.is_empty() || argv.len() > 76 {
        return Ok(None);
    }
    let Some((command, count)) = indexed_command_prefix(engine, argv)? else {
        return Ok(None);
    };
    let flags = engine
        .index
        .ids_for_command(&command)?
        .into_iter()
        .map(|id| engine.index.capability(id))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|cap| cap.kind == Kind::Option)
        .map(|cap| cap.name)
        .collect();
    let draft = plan::Draft {
        status: plan::Status::Ok,
        steps: vec![plan::Step {
            command,
            args: argv[count..].to_vec(),
            after: plan::After::Start,
            stdout: None,
        }],
        questions: vec![],
    };
    exact_draft_response(engine, draft, flags, allow_uninstalled).map(Some)
}

fn exact_draft_response(
    engine: &Engine,
    draft: plan::Draft,
    flags: BTreeSet<String>,
    allow_uninstalled: bool,
) -> Result<PlanResponse> {
    let command = draft.steps[0].command.clone();
    let root = command.split(' ').next().unwrap_or("");
    let route = route::Classification {
        status: "resolved",
        command: Some(command.clone()),
        candidates: vec![route::Candidate {
            command: command.clone(),
            score: 0.0,
            clause_coverage: 1,
            matched_terms: 0,
            program_available: engine.inventory.contains_key(root),
        }],
        reason: "exact_syntax",
    };
    let report = plan::validate(
        &engine.index,
        &draft,
        &engine.inventory,
        &plan::ValidationOptions {
            allow_uninstalled,
            allowed_commands: Some([command.clone()].into_iter().collect()),
            allowed_flags: Some([(command, flags)].into_iter().collect()),
            ..Default::default()
        },
    )?;
    Ok(PlanResponse {
        report,
        inference: infer::Metrics::default(),
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

#[derive(Debug, Default)]
enum Presence<T> {
    #[default]
    Absent,
    Present(T),
}

impl<T> Presence<T> {
    fn value(&self) -> Option<&T> {
        match self {
            Self::Absent => None,
            Self::Present(value) => Some(value),
        }
    }

    fn is_present(&self) -> bool {
        matches!(self, Self::Present(_))
    }
}

fn present<'de, D, T>(deserializer: D) -> std::result::Result<Presence<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Presence::Present)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    #[serde(default = "schema_one")]
    schema_version: u32,
    id: String,
    #[serde(default = "search_op")]
    op: String,
    #[serde(default)]
    query: String,
    #[serde(default, deserialize_with = "present")]
    argv: Presence<Option<Vec<String>>>,
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
    #[serde(default, deserialize_with = "present")]
    plan: Presence<Option<plan::Draft>>,
    #[serde(default)]
    cwd: Option<PathBuf>,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u64,
    #[serde(default = "default_output_bytes")]
    max_output_bytes: usize,
    #[serde(default)]
    raw_output_dir: Option<PathBuf>,
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
fn default_timeout_ms() -> u64 {
    30_000
}
fn default_output_bytes() -> usize {
    1024
}

struct CompactExecution<'a> {
    id: &'a str,
    status: &'a str,
    exit_codes: &'a [Option<i32>],
    timed_out: bool,
    elapsed_ms: serde_json::Value,
    raw_paths: &'a [&'a Path],
    raw_prefix: Option<&'a Path>,
    raw_files: usize,
}

fn compact_execution_response(status: CompactExecution<'_>, max_bytes: usize) -> Result<Vec<u8>> {
    let fallback = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "id": status.id,
        "status": status.status,
        "exit_codes": status.exit_codes,
        "timed_out": status.timed_out,
        "elapsed_ms": status.elapsed_ms,
        "output_omitted": true,
    });
    if !status.raw_paths.is_empty() {
        let mut with_paths = fallback.clone();
        with_paths["raw_paths"] = serde_json::to_value(status.raw_paths)?;
        let mut bytes = serde_json::to_vec(&with_paths)?;
        bytes.push(b'\n');
        if bytes.len() <= max_bytes {
            return Ok(bytes);
        }
    }
    let mut with_count = fallback;
    if status.raw_files > 0 {
        if let Some(prefix) = status.raw_prefix {
            with_count["raw_prefix"] = serde_json::json!(prefix);
        }
        with_count["raw_files"] = serde_json::json!(status.raw_files);
    }
    let mut bytes = serde_json::to_vec(&with_count)?;
    bytes.push(b'\n');
    if bytes.len() > max_bytes {
        return Err(Error::message(
            "response budget too small for execution status",
        ));
    }
    Ok(bytes)
}

fn execution_budget(id: &str, max_bytes: usize, raw_output_dir: Option<&Path>) -> Result<()> {
    if !(256..=1_048_576).contains(&max_bytes) {
        return Err(Error::message("max-bytes must be 256..1048576"));
    }
    let worst_exit_codes = vec![Some(i32::MIN); 8];
    let worst_prefix =
        raw_output_dir.map(|dir| dir.join(format!("watf-{}-{}", std::process::id(), u64::MAX)));
    compact_execution_response(
        CompactExecution {
            id,
            status: "timeout",
            exit_codes: &worst_exit_codes,
            timed_out: true,
            elapsed_ms: serde_json::json!(u128::MAX.to_string()),
            raw_paths: &[],
            raw_prefix: worst_prefix.as_deref(),
            raw_files: raw_output_dir.map_or(0, |_| 16),
        },
        max_bytes,
    )
    .map(|_| ())
}

fn bounded_execution_response(
    response: serde_json::Value,
    id: &str,
    execution: &execute::Execution,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(&response)?;
    bytes.push(b'\n');
    if bytes.len() <= max_bytes {
        return Ok(bytes);
    }
    let raw_paths: Vec<&Path> = execution
        .steps
        .iter()
        .flat_map(|step| [&step.stdout.raw_path, &step.stderr.raw_path])
        .filter_map(|path| path.as_deref())
        .collect();
    let raw_prefix = raw_paths
        .first()
        .and_then(|path| path.parent())
        .filter(|prefix| {
            raw_paths
                .iter()
                .all(|path| path.parent().is_some_and(|parent| parent == *prefix))
        });
    let exit_codes = execution
        .steps
        .iter()
        .map(|step| step.exit_code)
        .collect::<Vec<_>>();
    compact_execution_response(
        CompactExecution {
            id,
            status: execution.status,
            exit_codes: &exit_codes,
            timed_out: execution.steps.iter().any(|step| step.timed_out),
            elapsed_ms: serde_json::json!(execution.elapsed_ms),
            raw_paths: &raw_paths,
            raw_prefix,
            raw_files: raw_paths.len(),
        },
        max_bytes,
    )
}

#[derive(Serialize)]
struct ExactStepResult<'a> {
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    timed_out: bool,
    stdout: &'a execute::Output,
    #[serde(skip_serializing_if = "Option::is_none")]
    stderr: Option<&'a execute::Output>,
}

#[derive(Serialize)]
struct ExactExecution<'a> {
    steps: [ExactStepResult<'a>; 1],
    elapsed_ms: u128,
}

fn exact_execution(execution: &execute::Execution) -> Result<ExactExecution<'_>> {
    let step = execution
        .steps
        .first()
        .ok_or_else(|| Error::message("exact execution returned no step"))?;
    if execution.steps.len() != 1 {
        return Err(Error::message("exact execution returned multiple steps"));
    }
    Ok(ExactExecution {
        steps: [ExactStepResult {
            exit_code: step.exit_code,
            timed_out: step.timed_out,
            stdout: &step.stdout,
            stderr: (step.stderr.bytes != 0 || !step.stderr.text.is_empty())
                .then_some(&step.stderr),
        }],
        elapsed_ms: execution.elapsed_ms,
    })
}

struct ResolveMetadata<'a> {
    id: &'a str,
    route_reason: &'a str,
    validation_scope: &'a str,
    resolved_argv: &'a [Vec<String>],
}

#[derive(Serialize)]
struct AbstainCandidate {
    command: String,
    score: f32,
    program_available: bool,
    clause_coverage: usize,
}

fn resolve_abstention_response(
    id: &str,
    route: &route::Classification,
    clause_count: usize,
    uncovered_clauses: &[usize],
    max_bytes: usize,
) -> Result<Vec<u8>> {
    if !(256..=1_048_576).contains(&max_bytes) {
        return Err(Error::message("max-bytes must be 256..1048576"));
    }
    let mut candidates = route
        .candidates
        .iter()
        .take(4)
        .map(|candidate| AbstainCandidate {
            command: candidate.command.clone(),
            score: candidate.score,
            program_available: candidate.program_available,
            clause_coverage: candidate.clause_coverage,
        })
        .collect::<Vec<_>>();
    let mut candidates_truncated = route.candidates.len() > candidates.len();
    loop {
        let response = serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "id": id,
            "status": "abstained",
            "resolution": "deterministic",
            "abstain_reason": "planning_required",
            "route_reason": route.reason,
            "model_calls": 0,
            "clause_count": clause_count,
            "uncovered_clauses": uncovered_clauses,
            "candidates": candidates,
            "candidates_truncated": candidates_truncated,
        });
        let mut bytes = serde_json::to_vec(&response)?;
        bytes.push(b'\n');
        if bytes.len() <= max_bytes {
            return Ok(bytes);
        }
        if candidates.pop().is_none() {
            return Err(Error::message(
                "response budget too small for abstention metadata",
            ));
        }
        candidates_truncated = true;
    }
}

struct ResolveCompact<'a> {
    status: &'a str,
    exit_codes: &'a [Option<i32>],
    skipped: &'a [bool],
    timed_out: bool,
    elapsed_ms: serde_json::Value,
    streams: serde_json::Value,
    raw_paths: &'a [&'a Path],
    raw_prefix: Option<&'a Path>,
    raw_files: usize,
}

fn resolve_compact_response(
    metadata: &ResolveMetadata<'_>,
    compact: ResolveCompact<'_>,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    let fallback = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "id": metadata.id,
        "status": compact.status,
        "resolution": "deterministic",
        "route_reason": metadata.route_reason,
        "model_calls": 0,
        "validation_scope": metadata.validation_scope,
        "resolved_argv": metadata.resolved_argv,
        "step_count": metadata.resolved_argv.len(),
        "exit_codes": compact.exit_codes,
        "skipped": compact.skipped,
        "timed_out": compact.timed_out,
        "elapsed_ms": compact.elapsed_ms,
        "streams": compact.streams,
        "output_omitted": true,
    });
    if !compact.raw_paths.is_empty() {
        let mut with_paths = fallback.clone();
        with_paths["raw_paths"] = serde_json::to_value(compact.raw_paths)?;
        let mut bytes = serde_json::to_vec(&with_paths)?;
        bytes.push(b'\n');
        if bytes.len() <= max_bytes {
            return Ok(bytes);
        }
    }
    let mut with_count = fallback;
    if compact.raw_files > 0 {
        if let Some(prefix) = compact.raw_prefix {
            with_count["raw_prefix"] = serde_json::json!(prefix);
        }
        with_count["raw_files"] = serde_json::json!(compact.raw_files);
    }
    let mut bytes = serde_json::to_vec(&with_count)?;
    bytes.push(b'\n');
    if bytes.len() > max_bytes {
        return Err(Error::message(
            "response budget too small for resolved execution status",
        ));
    }
    Ok(bytes)
}

fn resolve_execution_budget(
    metadata: &ResolveMetadata<'_>,
    max_bytes: usize,
    raw_output_dir: Option<&Path>,
) -> Result<()> {
    if !(256..=1_048_576).contains(&max_bytes) {
        return Err(Error::message("max-bytes must be 256..1048576"));
    }
    let step_count = metadata.resolved_argv.len();
    let exit_codes = vec![Some(i32::MIN); step_count];
    let skipped = vec![true; step_count];
    let streams = serde_json::Value::Array(
        (0..step_count)
            .map(|_| {
                serde_json::json!({
                    "stdout": {"bytes": usize::MAX, "truncated": true},
                    "stderr": {"bytes": usize::MAX, "truncated": true},
                })
            })
            .collect(),
    );
    let worst_prefix =
        raw_output_dir.map(|dir| dir.join(format!("watf-{}-{}", std::process::id(), u64::MAX)));
    resolve_compact_response(
        metadata,
        ResolveCompact {
            status: "timeout",
            exit_codes: &exit_codes,
            skipped: &skipped,
            timed_out: true,
            elapsed_ms: serde_json::json!(u128::MAX.to_string()),
            streams,
            raw_paths: &[],
            raw_prefix: worst_prefix.as_deref(),
            raw_files: raw_output_dir.map_or(0, |_| step_count.saturating_mul(2)),
        },
        max_bytes,
    )
    .map(|_| ())
}

fn resolved_argv(report: &plan::Report) -> Vec<Vec<String>> {
    report
        .steps
        .iter()
        .map(|step| {
            step.command
                .split_whitespace()
                .map(str::to_owned)
                .chain(step.args.iter().cloned())
                .collect()
        })
        .collect()
}

fn resolve_execution_response(
    metadata: &ResolveMetadata<'_>,
    execution: &execute::Execution,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    let steps = execution
        .steps
        .iter()
        .map(|step| {
            let mut value = serde_json::json!({
                "exit_code": step.exit_code,
                "stdout": step.stdout,
            });
            if step.stderr.bytes != 0 || !step.stderr.text.is_empty() {
                value["stderr"] = serde_json::to_value(&step.stderr)?;
            }
            if step.timed_out {
                value["timed_out"] = serde_json::json!(true);
            }
            if step.skipped {
                value["skipped"] = serde_json::json!(true);
            }
            Ok(value)
        })
        .collect::<Result<Vec<_>>>()?;
    let response = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "id": metadata.id,
        "status": execution.status,
        "resolution": "deterministic",
        "route_reason": metadata.route_reason,
        "model_calls": 0,
        "validation_scope": metadata.validation_scope,
        "resolved_argv": metadata.resolved_argv,
        "step_count": metadata.resolved_argv.len(),
        "execution": {
            "steps": steps,
            "elapsed_ms": execution.elapsed_ms,
        },
    });
    let mut bytes = serde_json::to_vec(&response)?;
    bytes.push(b'\n');
    if bytes.len() <= max_bytes {
        return Ok(bytes);
    }

    let raw_paths: Vec<&Path> = execution
        .steps
        .iter()
        .flat_map(|step| [&step.stdout.raw_path, &step.stderr.raw_path])
        .filter_map(|path| path.as_deref())
        .collect();
    let raw_prefix = raw_paths
        .first()
        .and_then(|path| path.parent())
        .filter(|prefix| {
            raw_paths
                .iter()
                .all(|path| path.parent().is_some_and(|parent| parent == *prefix))
        });
    let exit_codes = execution
        .steps
        .iter()
        .map(|step| step.exit_code)
        .collect::<Vec<_>>();
    let skipped = execution
        .steps
        .iter()
        .map(|step| step.skipped)
        .collect::<Vec<_>>();
    let streams = serde_json::Value::Array(
        execution
            .steps
            .iter()
            .map(|step| {
                serde_json::json!({
                    "stdout": {"bytes": step.stdout.bytes, "truncated": step.stdout.truncated},
                    "stderr": {"bytes": step.stderr.bytes, "truncated": step.stderr.truncated},
                })
            })
            .collect(),
    );
    resolve_compact_response(
        metadata,
        ResolveCompact {
            status: execution.status,
            exit_codes: &exit_codes,
            skipped: &skipped,
            timed_out: execution.steps.iter().any(|step| step.timed_out),
            elapsed_ms: serde_json::json!(execution.elapsed_ms),
            streams,
            raw_paths: &raw_paths,
            raw_prefix,
            raw_files: raw_paths.len(),
        },
        max_bytes,
    )
}

fn execute_draft(
    engine: &Engine,
    draft: &plan::Draft,
    cwd: PathBuf,
    timeout_ms: u64,
    max_output_bytes: usize,
    raw_output_dir: Option<PathBuf>,
) -> Result<(plan::Report, execute::Execution)> {
    let report = plan::validate(
        &engine.index,
        draft,
        &engine.inventory,
        &plan::ValidationOptions::default(),
    )?;
    if !report.accepted {
        return Err(Error::message(format!(
            "plan rejected: {}",
            report.errors.join("; ")
        )));
    }
    let execution = execute::execute(
        &report,
        &engine.inventory,
        &execute::Options {
            cwd,
            timeout_ms,
            max_output_bytes,
            raw_output_dir,
        },
    )?;
    Ok((report, execution))
}

/// Persistent foreground engine. stdin/stdout only, no sockets or daemonization.
pub fn serve<R: BufRead, W: Write>(engine: &mut Engine, mut input: R, mut output: W) -> Result<()> {
    while let Some(line) = ingest::bounded_line(&mut input, 65536)? {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let response = (|| -> Result<Vec<u8>> {
            let request: Request = serde_json::from_slice(&line)?;
            if request.schema_version != 1 || request.id.len() > 64 {
                return Err(Error::message(
                    "unsupported schema or request id longer than 64 bytes",
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
            match request.op.as_str() {
                "search" => {
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
                }
                "route" => {
                    let packet = engine.lookup(&request.query, &options)?;
                    let classification = route::classify(&packet, &request.query);
                    let mut bytes = serde_json::to_vec(&serde_json::json!({
                        "schema_version": SCHEMA_VERSION,
                        "id": request.id,
                        "route": classification,
                    }))?;
                    bytes.push(b'\n');
                    if bytes.len() > request.max_bytes {
                        return Err(Error::message("response budget too small"));
                    }
                    Ok(bytes)
                }
                "run" => {
                    execution_budget(
                        &request.id,
                        request.max_bytes,
                        request.raw_output_dir.as_deref(),
                    )?;
                    let draft = request
                        .plan
                        .value()
                        .and_then(Option::as_ref)
                        .ok_or_else(|| Error::message("run request needs plan"))?;
                    let cwd = match request.cwd {
                        Some(path) => path,
                        None => std::env::current_dir()?,
                    };
                    let (report, execution) = execute_draft(
                        engine,
                        draft,
                        cwd,
                        request.timeout_ms,
                        request.max_output_bytes,
                        request.raw_output_dir,
                    )?;
                    let response = serde_json::json!({
                        "schema_version": SCHEMA_VERSION,
                        "id": request.id,
                        "status": execution.status,
                        "validation_scope": report.validation_scope,
                        "warnings": report.warnings,
                        "evidence": report.evidence,
                        "execution": execution,
                    });
                    bounded_execution_response(response, &request.id, &execution, request.max_bytes)
                }
                "exec" => {
                    execution_budget(
                        &request.id,
                        request.max_bytes,
                        request.raw_output_dir.as_deref(),
                    )?;
                    let planned = match request.argv.value().and_then(Option::as_deref) {
                        Some(argv) => exact_argv_plan_request(engine, argv, false)?,
                        None => exact_plan_request(engine, &request.query, false, true)?,
                    }
                    .ok_or_else(|| Error::message("exec needs explicit indexed command syntax"))?;
                    if !planned.report.accepted {
                        return Err(Error::message(format!(
                            "plan rejected: {}",
                            planned.report.errors.join("; ")
                        )));
                    }
                    let cwd = match request.cwd {
                        Some(path) => path,
                        None => std::env::current_dir()?,
                    };
                    let execution = execute::execute(
                        &planned.report,
                        &engine.inventory,
                        &execute::Options {
                            cwd,
                            timeout_ms: request.timeout_ms,
                            max_output_bytes: request.max_output_bytes,
                            raw_output_dir: request.raw_output_dir,
                        },
                    )?;
                    let response = serde_json::json!({
                        "schema_version": SCHEMA_VERSION,
                        "id": request.id,
                        "status": execution.status,
                        "resolution": "exact",
                        "model_calls": 0,
                        "validation_scope": planned.report.validation_scope,
                        "execution": exact_execution(&execution)?,
                    });
                    bounded_execution_response(response, &request.id, &execution, request.max_bytes)
                }
                "resolve_exec" => {
                    if request.argv.is_present() || request.plan.is_present() {
                        return Err(Error::message("resolve_exec rejects argv and plan"));
                    }
                    let planned = match plan_attempt(engine, &request.query, &options, None, false)?
                    {
                        PlanAttempt::Planned(planned) => *planned,
                        PlanAttempt::NeedsPlanning {
                            route,
                            clause_count,
                            uncovered_clauses,
                        } => {
                            return resolve_abstention_response(
                                &request.id,
                                &route,
                                clause_count,
                                &uncovered_clauses,
                                request.max_bytes,
                            );
                        }
                    };
                    if !planned.report.accepted
                        || planned.report.status != plan::Status::Ok
                        || planned.report.steps.is_empty()
                        || planned.inference.prompt_tokens != 0
                        || planned.inference.generated_tokens != 0
                        || planned.inference.load_ms != 0
                        || planned.inference.prefill_ms != 0
                        || planned.inference.generation_ms != 0
                    {
                        return Err(Error::message(
                            "resolve_exec requires an accepted deterministic ok plan",
                        ));
                    }
                    let argv = resolved_argv(&planned.report);
                    let metadata = ResolveMetadata {
                        id: &request.id,
                        route_reason: planned.route.reason,
                        validation_scope: planned.report.validation_scope,
                        resolved_argv: &argv,
                    };
                    resolve_execution_budget(
                        &metadata,
                        request.max_bytes,
                        request.raw_output_dir.as_deref(),
                    )?;
                    let cwd = match request.cwd {
                        Some(path) => path,
                        None => std::env::current_dir()?,
                    };
                    let execution = execute::execute(
                        &planned.report,
                        &engine.inventory,
                        &execute::Options {
                            cwd,
                            timeout_ms: request.timeout_ms,
                            max_output_bytes: request.max_output_bytes,
                            raw_output_dir: request.raw_output_dir,
                        },
                    )?;
                    resolve_execution_response(&metadata, &execution, request.max_bytes)
                }
                _ => Err(Error::message("unsupported operation")),
            }
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
            let cwd = match &args.cwd {
                Some(path) => path.clone(),
                None => std::env::current_dir()?,
            };
            let (report, execution) = execute_draft(
                &engine,
                &read_plan(file)?,
                cwd,
                args.timeout_ms,
                args.max_output_bytes,
                args.raw_output_dir.clone(),
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
        "exec" => {
            let planned = exact_argv_plan_request(&engine, &args.query, false)?
                .ok_or_else(|| Error::message("exec needs explicit indexed command syntax"))?;
            if !planned.report.accepted {
                return Err(Error::message(format!(
                    "plan rejected: {}",
                    planned.report.errors.join("; ")
                )));
            }
            let cwd = match &args.cwd {
                Some(path) => path.clone(),
                None => std::env::current_dir()?,
            };
            let execution = execute::execute(
                &planned.report,
                &engine.inventory,
                &execute::Options {
                    cwd,
                    timeout_ms: args.timeout_ms,
                    max_output_bytes: args.max_output_bytes,
                    raw_output_dir: args.raw_output_dir.clone(),
                },
            )?;
            if args.json {
                print_json(&serde_json::json!({
                    "schema_version": SCHEMA_VERSION,
                    "status": execution.status,
                    "resolution": "exact",
                    "model_calls": 0,
                    "validation_scope": planned.report.validation_scope,
                    "execution": exact_execution(&execution)?,
                }))?;
            } else {
                println!("{}", serde_json::to_string_pretty(&execution)?);
            }
            if execution.status == "ok" {
                0
            } else {
                3
            }
        }
        "explain" => {
            explain(&engine, &args, &query)?;
            0
        }
        "route" => {
            let packet = engine.lookup(&query, &args.search)?;
            let classification = route::classify(&packet, &query);
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
            let model = args
                .model
                .as_ref()
                .map(|_| model_options(&args))
                .transpose()?;
            let response = plan_request(
                &mut engine,
                &query,
                &args.search,
                model.as_ref(),
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
                Some(&model_options(&args)?),
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
    use crate::record::{Arity, Source, SourceKind};

    fn capability(kind: Kind, name: &str, arity: Arity) -> Capability {
        Capability {
            id: format!("test/demo#{name}"),
            command: vec!["demo".into()],
            kind,
            name: name.into(),
            summary: "test capability".into(),
            aliases: vec![],
            arity,
            required: false,
            value_type: None,
            choices: vec![],
            source: Source {
                kind: SourceKind::Builtin,
                reference: "test".into(),
                version: None,
                sha256: None,
                modified_unix: None,
                bytes: None,
            },
        }
    }

    #[test]
    fn model_literals_keep_only_unexplained_query_values() {
        let context = plan::Context {
            commands: vec![
                plan::CommandContext {
                    command: "git add".into(),
                    summary: "Stage selected files or all working tree changes".into(),
                    options: vec![],
                },
                plan::CommandContext {
                    command: "git commit".into(),
                    summary: "Create a Git commit with a message".into(),
                    options: vec![plan::OptionContext {
                        name: "--message".into(),
                        aliases: vec!["-m".into()],
                        arity: Arity::One,
                        required: false,
                        description: "Use this commit message".into(),
                    }],
                },
                plan::CommandContext {
                    command: "docker compose up".into(),
                    summary: "Create start rebuild compose application service containers".into(),
                    options: vec![plan::OptionContext {
                        name: "--detach".into(),
                        aliases: vec!["-d".into()],
                        arity: Arity::None,
                        required: false,
                        description: "Run containers in the background".into(),
                    }],
                },
                plan::CommandContext {
                    command: "docker compose logs".into(),
                    summary: "Display and follow compose service container logs".into(),
                    options: vec![],
                },
            ],
            uncovered_clauses: vec![],
        };
        assert_eq!(
            model_required_literals("stage src and commit with message 'release'", &context),
            ["release".into(), "src".into()].into()
        );
        assert_eq!(
            model_required_literals(
                "rebuild the compose backend in the background and then follow its logs",
                &context,
            ),
            ["backend".into()].into()
        );
        assert_eq!(
            model_required_flags(
                "rebuild the compose backend in the background and then follow its logs",
                &context,
            ),
            [("docker compose up".into(), ["--detach".into()].into())].into()
        );
    }

    #[test]
    fn explicit_plan_never_guesses_positionals() {
        let path = std::env::temp_dir().join(format!(
            "watf-explicit-{}-{}.widx",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        index::build(
            &path,
            vec![
                capability(Kind::Command, "demo", Arity::Unknown),
                capability(Kind::Option, "--output", Arity::One),
            ],
        )
        .unwrap();
        let engine = Engine::new(index::Index::open(&path, false).unwrap());
        assert!(explicit_plan(&engine, "demo", false).unwrap().is_some());
        assert!(explicit_plan(&engine, "demo --output file", false)
            .unwrap()
            .is_some());
        assert!(explicit_plan(&engine, "demo file", false)
            .unwrap()
            .is_none());
        assert!(explicit_plan(&engine, "demo file", true).unwrap().is_some());
        assert!(explicit_plan(&engine, "demo -- --literal", true)
            .unwrap()
            .is_some());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn execution_budget_accepts_exact_compact_envelope_only() {
        let raw_dir = Path::new("/tmp/watf-budget-prefix");
        let worst_prefix = raw_dir.join(format!("watf-{}-{}", std::process::id(), u64::MAX));
        let exit_codes = vec![Some(i32::MIN); 8];
        let exact = compact_execution_response(
            CompactExecution {
                id: "budget",
                status: "timeout",
                exit_codes: &exit_codes,
                timed_out: true,
                elapsed_ms: serde_json::json!(u128::MAX.to_string()),
                raw_paths: &[],
                raw_prefix: Some(&worst_prefix),
                raw_files: 16,
            },
            usize::MAX,
        )
        .unwrap()
        .len();

        assert!(execution_budget("budget", exact, Some(raw_dir)).is_ok());
        assert!(execution_budget("budget", exact - 1, Some(raw_dir)).is_err());
    }

    #[test]
    fn decisive_grounded_route_skips_inference_but_unknown_literal_escalates() {
        let path = std::env::temp_dir().join(format!(
            "watf-route-plan-{}-{}.widx",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let mut command = capability(Kind::Command, "demo", Arity::Unknown);
        command.summary = "show current demo working tree status".into();
        command.aliases = vec!["without".into()];
        let mut short = capability(Kind::Option, "--short", Arity::None);
        short.summary = "short output".into();
        let mut porcelain = capability(Kind::Option, "--porcelain", Arity::One);
        porcelain.summary = "machine output".into();
        let mut terse = capability(Kind::Option, "--terse", Arity::None);
        terse.summary = "terse output".into();
        let mut brief = capability(Kind::Option, "--brief", Arity::None);
        brief.summary = "terse output".into();
        let mut hidden = capability(Kind::Option, "--hidden", Arity::None);
        hidden.summary = "hidden output".into();
        let mut show_current = capability(Kind::Option, "--show-current", Arity::None);
        show_current.summary = "print the current branch".into();

        let scoped = |scope: &str, kind, name: &str, arity| {
            let mut cap = capability(kind, name, arity);
            cap.id = format!("test/{scope}#{name}");
            cap.command = vec![scope.into()];
            cap.summary = if kind == Kind::Command {
                format!("inspect {scope} branch state")
            } else {
                "test option".into()
            };
            cap
        };
        let value_command = scoped("value-demo", Kind::Command, "value-demo", Arity::Unknown);
        let value_option = scoped("value-demo", Kind::Option, "--show-current", Arity::One);
        let ambiguous_command = scoped(
            "ambiguous-demo",
            Kind::Command,
            "ambiguous-demo",
            Arity::Unknown,
        );
        let ambiguous_a = scoped(
            "ambiguous-demo",
            Kind::Option,
            "--show-current",
            Arity::None,
        );
        let ambiguous_b = scoped(
            "ambiguous-demo",
            Kind::Option,
            "--current-show",
            Arity::None,
        );
        let alias_command = scoped("alias-demo", Kind::Command, "alias-demo", Arity::Unknown);
        let mut alias_option = scoped("alias-demo", Kind::Option, "--print-current", Arity::None);
        alias_option.aliases = vec!["--show-current".into()];
        index::build(
            &path,
            vec![
                command,
                short,
                porcelain,
                terse,
                brief,
                hidden,
                show_current,
                value_command,
                value_option,
                ambiguous_command,
                ambiguous_a,
                ambiguous_b,
                alias_command,
                alias_option,
            ],
        )
        .unwrap();
        let mut engine = Engine::new(index::Index::open(&path, false).unwrap());

        let planned = plan_request(
            &mut engine,
            "show demo working tree status",
            &Options::default(),
            None,
            true,
        )
        .unwrap();
        assert!(planned.report.accepted);
        assert_eq!(planned.report.steps[0].command, "demo");
        assert_eq!(planned.inference.prompt_tokens, 0);
        assert_eq!(planned.inference.generated_tokens, 0);

        let planned = plan_request(
            &mut engine,
            "show current demo working tree status",
            &Options::default(),
            None,
            true,
        )
        .unwrap();
        assert!(planned.report.accepted);
        assert_eq!(planned.report.steps[0].args, ["--show-current"]);
        assert_eq!(planned.inference.prompt_tokens, 0);
        assert_eq!(planned.inference.generated_tokens, 0);

        let planned = plan_request(
            &mut engine,
            "short show current demo working tree status short",
            &Options::default(),
            None,
            true,
        )
        .unwrap();
        assert!(planned.report.accepted);
        assert_eq!(planned.report.steps[0].args, ["--short", "--show-current"]);

        let planned = plan_request(
            &mut engine,
            "show demo working tree status short",
            &Options::default(),
            None,
            true,
        )
        .unwrap();
        assert!(planned.report.accepted);
        assert_eq!(planned.report.steps[0].command, "demo");
        assert_eq!(planned.report.steps[0].args, ["--short"]);
        assert_eq!(planned.inference.prompt_tokens, 0);
        assert_eq!(planned.inference.generated_tokens, 0);

        let mut packet = engine
            .lookup("show demo working tree status hidden", &Options::default())
            .unwrap();
        assert!(engine
            .index
            .ids_for_command("demo")
            .unwrap()
            .iter()
            .any(|&id| engine.index.capability(id).unwrap().name == "--hidden"));
        packet.evidence.retain(|e| e.name != "--hidden");
        assert!(routed_plan_request(
            &engine,
            "show demo working tree status hidden",
            &route::Classification {
                status: "resolved",
                command: Some("demo".into()),
                candidates: vec![],
                reason: "retrieval_margin",
            },
            &packet,
            true,
            true,
        )
        .unwrap()
        .is_none());

        let error = plan_request(
            &mut engine,
            "show current demo working tree status without the current option",
            &Options::default(),
            None,
            true,
        )
        .unwrap_err();
        assert!(error.to_string().contains("planning needs a local model"));

        let error = plan_request(
            &mut engine,
            "show demo working tree status machine",
            &Options::default(),
            None,
            true,
        )
        .unwrap_err();
        assert!(error.to_string().contains("planning needs a local model"));

        let error = plan_request(
            &mut engine,
            "show demo working tree status terse",
            &Options::default(),
            None,
            true,
        )
        .unwrap_err();
        assert!(error.to_string().contains("planning needs a local model"));

        let error = plan_request(
            &mut engine,
            "show demo working tree status src",
            &Options::default(),
            None,
            true,
        )
        .unwrap_err();
        assert!(error.to_string().contains("planning needs a local model"));

        let planned = plan_request(
            &mut engine,
            "show current alias-demo branch",
            &Options::default(),
            None,
            true,
        )
        .unwrap();
        assert!(planned.report.accepted);
        assert!(planned.report.steps[0].args.is_empty());

        for query in [
            "show current",
            "show current value-demo branch",
            "show current ambiguous-demo branch",
        ] {
            let error =
                plan_request(&mut engine, query, &Options::default(), None, true).unwrap_err();
            assert!(error.to_string().contains("planning needs a local model"));
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn grounded_multi_clause_single_scope_skips_inference() {
        let path = std::env::temp_dir().join(format!(
            "watf-route-multi-plan-{}-{}.widx",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let mut command = capability(Kind::Command, "demo", Arity::Unknown);
        command.summary = "inspect demo state and show demo status".into();
        let mut records = vec![command];
        records.extend(ingest::builtin().unwrap().into_iter().filter(|cap| {
            matches!(
                cap.command_key().as_str(),
                "docker logs" | "docker compose logs"
            )
        }));
        index::build(&path, records).unwrap();
        let mut engine = Engine::new(index::Index::open(&path, false).unwrap());
        engine.inventory.insert(
            "docker".into(),
            discover::Executable {
                path: "/bin/true".into(),
                bytes: 0,
                modified_unix: None,
            },
        );

        let planned = plan_request(
            &mut engine,
            "inspect demo state and show demo status",
            &Options::default(),
            None,
            true,
        )
        .unwrap();
        assert!(planned.report.accepted);
        assert_eq!(planned.report.steps[0].command, "demo");
        assert_eq!(planned.route.reason, "grounded_single_scope");
        assert_eq!(planned.inference.generated_tokens, 0);

        let planned = plan_request(
            &mut engine,
            "show docker compose logs then display docker compose logs",
            &Options::default(),
            None,
            true,
        )
        .unwrap();
        assert!(planned.report.accepted);
        assert_eq!(planned.report.steps[0].command, "docker compose logs");
        assert_eq!(planned.route.reason, "grounded_single_scope");

        let planned = plan_request(
            &mut engine,
            "show docker logs then show docker logs",
            &Options::default(),
            None,
            true,
        )
        .unwrap();
        assert!(planned.report.accepted);
        assert!(!planned.report.steps.is_empty());
        assert!(planned
            .report
            .steps
            .iter()
            .all(|step| step.command == "docker logs"));

        let planned = plan_request(
            &mut engine,
            "show docker logs then show docker compose logs",
            &Options::default(),
            None,
            true,
        )
        .unwrap();
        assert!(planned.report.accepted);
        assert_eq!(
            planned
                .report
                .steps
                .iter()
                .map(|step| step.command.as_str())
                .collect::<Vec<_>>(),
            ["docker logs", "docker compose logs"]
        );

        for query in [
            "show logs then show logs",
            "show logs and display logs",
            "show logs then print logs",
        ] {
            let error =
                plan_request(&mut engine, query, &Options::default(), None, true).unwrap_err();
            assert!(error.to_string().contains("planning needs a local model"));
        }

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn grounded_multi_scope_skips_inference_but_literal_escalates() {
        let path = std::env::temp_dir().join(format!(
            "watf-route-multi-scope-plan-{}-{}.widx",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let mut inspect = capability(Kind::Command, "alpha", Arity::Unknown);
        inspect.id = "test/alpha#alpha".into();
        inspect.command = vec!["alpha".into()];
        inspect.summary = "inspect lunar cache".into();
        let mut display = capability(Kind::Command, "beta", Arity::Unknown);
        display.id = "test/beta#beta".into();
        display.command = vec!["beta".into()];
        display.summary = "display ocean queue".into();
        index::build(&path, vec![inspect, display]).unwrap();
        let mut engine = Engine::new(index::Index::open(&path, false).unwrap());

        let planned = plan_request(
            &mut engine,
            "inspect lunar cache, then display ocean queue",
            &Options::default(),
            None,
            true,
        )
        .unwrap();
        assert!(planned.report.accepted);
        assert_eq!(planned.report.steps.len(), 2);
        assert_eq!(planned.report.steps[0].command, "alpha");
        assert_eq!(planned.report.steps[1].command, "beta");
        assert_eq!(planned.report.steps[1].after, plan::After::Success);
        assert_eq!(planned.route.reason, "grounded_multi_scope");
        assert_eq!(planned.inference.generated_tokens, 0);

        let error = plan_request(
            &mut engine,
            "inspect lunar cache, then display ocean queue src",
            &Options::default(),
            None,
            true,
        )
        .unwrap_err();
        assert!(error.to_string().contains("planning needs a local model"));

        let _ = std::fs::remove_file(path);
    }

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

    #[test]
    fn exec_accepts_literal_command_options() {
        let a = parse(&[
            "exec".into(),
            "--json".into(),
            "--".into(),
            "git".into(),
            "status".into(),
            "--short".into(),
        ])
        .unwrap();
        assert_eq!(a.op, "exec");
        assert_eq!(a.query, ["git", "status", "--short"]);
        assert!(a.json);
    }
}
