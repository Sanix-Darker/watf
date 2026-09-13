use super::{After, Draft, Report, Status, StepEvidence, MAX_ARGS, MAX_LITERAL, MAX_STEPS};
use crate::{
    discover::{self, Executable, Freshness},
    index::Index,
    record::{Arity, Capability, Kind},
    Result, SCHEMA_VERSION,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default, Clone)]
pub struct ValidationOptions {
    pub allow_uninstalled: bool,
    /// When provided, synthesis may use only these retrieved command scopes.
    pub allowed_commands: Option<BTreeSet<String>>,
    /// When provided, reject flags outside the actual model evidence context.
    pub allowed_flags: Option<BTreeMap<String, BTreeSet<String>>>,
}

fn literal(value: &str) -> bool {
    value.len() <= MAX_LITERAL && !value.chars().any(char::is_control)
}
fn option<'a>(caps: &'a [Capability], name: &str) -> Option<&'a Capability> {
    caps.iter()
        .filter(|r| r.matches_flag(name))
        .max_by_key(|r| r.source.kind.priority())
}
fn check_value(cap: &Capability, value: &str, errors: &mut Vec<String>) {
    if !cap.choices.is_empty() && !cap.choices.iter().any(|v| v == value) {
        errors.push(format!(
            "{}: value is absent from the documented enum",
            cap.name
        ));
    }
}
fn record_option(cap: &Capability, seen: &mut BTreeSet<String>, ids: &mut BTreeSet<String>) {
    seen.insert(cap.name.clone());
    ids.insert(cap.id.clone());
}

fn flags(
    args: &[String],
    caps: &[Capability],
    allowed: Option<&BTreeSet<String>>,
    errors: &mut Vec<String>,
    ids: &mut BTreeSet<String>,
) {
    let mut seen = BTreeSet::new();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" {
            break;
        }
        if !arg.starts_with('-') || arg == "-" {
            index += 1;
            continue;
        }
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(a, b)| (a, Some(b)));
        let mut exact = option(caps, name);
        let mut attached = inline.map(str::to_owned);
        // Conservative POSIX short-option clusters. Multi-letter options such
        // as find -mtime and ffmpeg -c:v were already checked as exact names.
        if exact.is_none()
            && inline.is_none()
            && arg.starts_with('-')
            && !arg.starts_with("--")
            && arg.len() > 2
            && arg.is_ascii()
        {
            let mut cursor = 1;
            while cursor < arg.len() {
                let flag = format!("-{}", char::from(arg.as_bytes()[cursor]));
                let Some(cap) = option(caps, &flag) else {
                    errors.push(format!("undocumented flag {flag} in {arg}"));
                    break;
                };
                if allowed.is_some_and(|set| !set.contains(&cap.name)) {
                    errors.push(format!("{} was not in the synthesis evidence", cap.name));
                }
                if cap.arity == Arity::None {
                    record_option(cap, &mut seen, ids);
                    cursor += 1;
                } else {
                    exact = Some(cap);
                    if cursor + 1 < arg.len() {
                        attached = Some(arg[cursor + 1..].to_owned());
                    }
                    break;
                }
            }
            if exact.is_none() {
                index += 1;
                continue;
            }
        }
        let Some(cap) = exact else {
            errors.push(format!("undocumented flag {name}"));
            index += 1;
            continue;
        };
        if allowed.is_some_and(|set| !set.contains(&cap.name)) {
            errors.push(format!("{} was not in the synthesis evidence", cap.name));
        }
        record_option(cap, &mut seen, ids);
        match cap.arity {
            Arity::None => {
                if attached.is_some() {
                    errors.push(format!("{} does not take a value", cap.name));
                }
            }
            Arity::Unknown => errors.push(format!(
                "{} has unknown argument arity; import better local documentation",
                cap.name
            )),
            Arity::Optional => {
                if let Some(value) = &attached {
                    check_value(cap, value, errors);
                }
            }
            Arity::One | Arity::Two => {
                let count = if cap.arity == Arity::Two { 2 } else { 1 };
                let mut values = usize::from(attached.is_some());
                if let Some(value) = &attached {
                    check_value(cap, value, errors);
                }
                while values < count {
                    if let Some(value) = args.get(index + 1) {
                        // A value may start with '-', for example a negative
                        // number or commit message. It is still one literal argv.
                        index += 1;
                        values += 1;
                        check_value(cap, value, errors);
                    } else {
                        errors.push(format!("{} needs {count} value(s)", cap.name));
                        break;
                    }
                }
            }
            Arity::Many => {
                let mut values = usize::from(attached.is_some());
                if let Some(value) = &attached {
                    check_value(cap, value, errors);
                }
                while let Some(value) = args.get(index + 1) {
                    if value.starts_with('-') {
                        break;
                    }
                    index += 1;
                    values += 1;
                    check_value(cap, value, errors);
                }
                if values == 0 {
                    errors.push(format!("{} requires at least one value", cap.name));
                }
            }
        }
        index += 1;
    }
    for cap in caps.iter().filter(|r| r.kind == Kind::Option && r.required) {
        if !seen.contains(&cap.name) {
            errors.push(format!("missing required option {}", cap.name));
        }
    }
}

pub fn validate(
    index: &Index,
    draft: &Draft,
    inventory: &BTreeMap<String, Executable>,
    options: &ValidationOptions,
) -> Result<Report> {
    let mut report = Report {
        schema_version: SCHEMA_VERSION,
        status: draft.status,
        accepted: false,
        executed: false,
        approval_required: true,
        validation_scope: "documented_command_and_option_surface_not_semantic_correctness",
        steps: draft.steps.clone(),
        evidence: vec![],
        errors: vec![],
        warnings: vec![],
        questions: draft.questions.clone(),
        shell: None,
        shell_dialect: "bash",
    };
    if draft.questions.len() > 8 || draft.questions.iter().any(|q| !literal(q)) {
        report.errors.push("invalid questions".to_owned());
    }
    if draft.status != Status::Ok {
        if !draft.steps.is_empty() {
            report
                .errors
                .push("non-ok plans cannot contain executable steps".to_owned());
        }
        return Ok(report);
    }
    if !draft.questions.is_empty() {
        report
            .errors
            .push("ok plan cannot have unresolved questions".to_owned());
    }
    if draft.steps.is_empty() || draft.steps.len() > MAX_STEPS {
        report
            .errors
            .push("plan must contain 1..8 steps".to_owned());
        return Ok(report);
    }
    let mut snapshots = false;
    for (n, step) in draft.steps.iter().enumerate() {
        let mut errors = Vec::new();
        if (n == 0 && step.after != After::Start) || (n > 0 && step.after == After::Start) {
            errors.push("invalid dependency relation".to_owned());
        }
        if step.after == After::Pipe {
            report.warnings.push(format!("step {}: pipeline runs concurrently; Bash pipefail treats nonzero producers as failures", n + 1));
            if n == 0 || draft.steps[n - 1].stdout.is_some() {
                errors.push("invalid pipeline producer".to_owned());
            }
        }
        if step.args.len() > MAX_ARGS
            || !literal(&step.command)
            || step.args.iter().any(|a| !literal(a))
        {
            errors.push("argument limits or control-character policy violated".to_owned());
        }
        if let Some(output) = &step.stdout {
            if output.path.is_empty() || !literal(&output.path) {
                errors.push("invalid redirect path".to_owned());
            }
            report.warnings.push(format!(
                "step {}: stdout redirection modifies {}",
                n + 1,
                crate::text::compact(&output.path, 120)
            ));
        }
        if options
            .allowed_commands
            .as_ref()
            .is_some_and(|set| !set.contains(&step.command))
        {
            errors.push("command absent from retrieved synthesis evidence".to_owned());
        }
        let caps: Vec<_> = index
            .ids_for_command(&step.command)?
            .into_iter()
            .map(|id| index.capability(id))
            .collect::<Result<_>>()?;
        let command = caps.iter().find(|r| r.kind == Kind::Command);
        let mut ids = BTreeSet::new();
        let root = step.command.split(' ').next().unwrap_or("");
        let available = inventory.contains_key(root);
        if !available && !options.allow_uninstalled {
            errors.push(format!(
                "{root} was not found in an absolute PATH directory"
            ));
        }
        if let Some(command) = command {
            ids.insert(command.id.clone());
        } else {
            errors.push("unknown canonical command scope".to_owned());
        }
        let allowed = options
            .allowed_flags
            .as_ref()
            .and_then(|set| set.get(&step.command));
        flags(&step.args, &caps, allowed, &mut errors, &mut ids);
        for cap in caps.iter().filter(|r| ids.contains(&r.id)) {
            match discover::freshness(&cap.source) {
                Freshness::Changed | Freshness::Missing => {
                    errors.push(format!("stale or missing source for {}", cap.name))
                }
                Freshness::Snapshot | Freshness::Unknown => snapshots = true,
                Freshness::MetadataUnchanged => {}
            }
        }
        if root == "git" && step.command == "git commit" {
            report.warnings.push("git commit may include changes already staged before this plan; inspect git status first".to_owned());
        }
        if matches!(root, "rg" | "grep" | "diff")
            || step.args.iter().any(|a| a == "-detailed-exitcode")
        {
            report.warnings.push(format!("step {}: nonzero exit status may be an ordinary no-match or change result, not an execution error", n + 1));
        }
        if step
            .args
            .iter()
            .any(|a| a.contains("$(") || a.contains('`') || a.contains('*') || a.contains('$'))
        {
            report.warnings.push(format!("step {}: arguments are literal; shell substitutions, variables and globs will not expand", n + 1));
        }
        if matches!(root, "rm" | "mv" | "chmod" | "kill")
            || step.args.iter().any(|a| {
                matches!(
                    a.as_str(),
                    "--delete"
                        | "-delete"
                        | "--force"
                        | "-f"
                        | "--volumes"
                        | "--auto-approve"
                        | "-auto-approve"
                )
            })
            || step.command.contains("prune")
            || step.command == "terraform apply"
            || step.command == "kubectl delete"
        {
            report.warnings.push(format!(
                "step {}: potentially destructive capability requires explicit review",
                n + 1
            ));
        }
        if matches!(
            root,
            "aws" | "curl" | "kubectl" | "docker" | "podman" | "terraform"
        ) || matches!(step.command.as_str(), "git push" | "git pull" | "git fetch")
        {
            report.warnings.push(format!("step {}: proposed command may access the network or external infrastructure; watf itself remains offline", n + 1));
        }
        report.evidence.push(StepEvidence {
            step: n,
            ids: ids.into_iter().collect(),
            program_available: available,
        });
        report
            .errors
            .extend(errors.into_iter().map(|e| format!("step {}: {e}", n + 1)));
    }
    if snapshots {
        report.warnings.push("bundled or unversioned documentation is not proof of compatibility with the installed binary".to_owned());
    }
    report.warnings.push("positional arguments, resource existence, permissions, shell state, option interactions and task intent are not proven by this validator".to_owned());
    report.warnings.sort();
    report.warnings.dedup();
    if report.errors.is_empty() {
        match super::render(draft) {
            Ok(shell) => {
                report.shell = Some(shell);
                report.accepted = true;
            }
            Err(e) => report.errors.push(e.to_string()),
        }
    }
    Ok(report)
}
