mod common;
use common::*;
use std::collections::BTreeMap;
use watf::plan::{self, After, Draft, Redirect, RedirectMode, Status, Step, ValidationOptions};
fn step(command: &str, args: &[&str], after: After) -> Step {
    Step {
        command: command.into(),
        args: args.iter().map(|s| (*s).into()).collect(),
        after,
        stdout: None,
    }
}
fn draft(steps: Vec<Step>) -> Draft {
    Draft {
        status: Status::Ok,
        steps,
        questions: vec![],
    }
}
fn check(d: &Draft) -> plan::Report {
    let (_, i) = builtins();
    plan::validate(
        &i,
        d,
        &BTreeMap::new(),
        &ValidationOptions {
            allow_uninstalled: true,
            ..Default::default()
        },
    )
    .unwrap()
}
#[test]
fn multiple_tools_without_user_command() {
    let r = check(&draft(vec![
        step("git add", &["-A"], After::Start),
        step("git commit", &["-m", "release"], After::Success),
        step(
            "docker compose up",
            &["--build", "--detach"],
            After::Success,
        ),
    ]));
    assert!(r.accepted, "{:?}", r.errors);
    assert!(!r.executed);
    assert!(r.approval_required);
    assert!(r.shell.unwrap().contains("&&"));
}
#[test]
fn hallucinated_flag_is_rejected() {
    assert!(
        !check(&draft(vec![step(
            "docker compose up",
            &["--turbo"],
            After::Start
        )]))
        .accepted
    );
}
#[test]
fn sibling_flag_does_not_leak() {
    assert!(
        !check(&draft(vec![step(
            "git add",
            &["--message", "x"],
            After::Start
        )]))
        .accepted
    );
}
#[test]
fn unknown_command_is_rejected() {
    assert!(!check(&draft(vec![step("imaginary", &[], After::Start)])).accepted);
}
#[test]
fn missing_value_is_rejected() {
    assert!(!check(&draft(vec![step("git commit", &["-m"], After::Start)])).accepted);
}
#[test]
fn equals_value_is_supported() {
    assert!(
        check(&draft(vec![step(
            "git commit",
            &["--message=hello"],
            After::Start
        )]))
        .accepted
    );
}
#[test]
fn short_attached_value_is_supported() {
    assert!(check(&draft(vec![step("git commit", &["-mhello"], After::Start)])).accepted);
}
#[test]
fn short_clusters_are_checked() {
    assert!(
        check(&draft(vec![step(
            "tar",
            &["-czf", "a.tar.gz", "src"],
            After::Start
        )]))
        .accepted
    );
}
#[test]
fn short_cluster_unknown_flag_rejected() {
    assert!(
        !check(&draft(vec![step(
            "tar",
            &["-cQf", "a.tar.gz", "src"],
            After::Start
        )]))
        .accepted
    );
}
#[test]
fn case_sensitive_short_flags() {
    assert!(!check(&draft(vec![step("git add", &["-a"], After::Start)])).accepted);
}
#[test]
fn no_value_flag_rejects_equals() {
    assert!(!check(&draft(vec![step("git add", &["--all=yes"], After::Start)])).accepted);
}
#[test]
fn literal_subshell_does_not_execute() {
    let r = check(&draft(vec![step(
        "git commit",
        &["-m", "$(touch /tmp/watf-do-not-run)"],
        After::Start,
    )]));
    assert!(r.accepted);
    assert!(r.shell.unwrap().contains("'$(touch /tmp/watf-do-not-run)'"));
}
#[test]
fn apostrophes_are_quoted() {
    let r = check(&draft(vec![step(
        "git commit",
        &["-m", "it's done"],
        After::Start,
    )]));
    assert!(r.shell.unwrap().contains("'it'\\''s done'"));
}
#[test]
fn control_characters_are_rejected() {
    assert!(
        !check(&draft(vec![step(
            "git commit",
            &["-m", "bad\nmessage"],
            After::Start
        )]))
        .accepted
    );
}
#[test]
fn long_literals_are_rejected() {
    let mut s = step("git commit", &["-m"], After::Start);
    s.args.push("x".repeat(4097));
    assert!(!check(&draft(vec![s])).accepted);
}
#[test]
fn missing_initial_relation_is_rejected() {
    assert!(!check(&draft(vec![step("git status", &[], After::Success)])).accepted);
}
#[test]
fn repeated_start_is_rejected() {
    assert!(
        !check(&draft(vec![
            step("git status", &[], After::Start),
            step("git log", &[], After::Start)
        ]))
        .accepted
    );
}
#[test]
fn plan_step_limit_is_enforced() {
    assert!(
        !check(&draft(
            (0..9)
                .map(|i| step(
                    "git status",
                    &[],
                    if i == 0 { After::Start } else { After::Success }
                ))
                .collect()
        ))
        .accepted
    );
}
#[test]
fn pipefail_is_explicit() {
    let r = check(&draft(vec![
        step("rg", &["TODO"], After::Start),
        step("sort", &[], After::Pipe),
    ]));
    assert!(r.accepted);
    assert!(r.shell.unwrap().starts_with("(set -o pipefail;"));
}
#[test]
fn redirected_pipeline_producer_is_rejected() {
    let mut s = step("rg", &["TODO"], After::Start);
    s.stdout = Some(Redirect {
        mode: RedirectMode::Truncate,
        path: "out".into(),
    });
    assert!(!check(&draft(vec![s, step("sort", &[], After::Pipe)])).accepted);
}
#[test]
fn redirection_is_literal() {
    let mut s = step("git status", &[], After::Start);
    s.stdout = Some(Redirect {
        mode: RedirectMode::Truncate,
        path: "$(bad) report.txt".into(),
    });
    assert!(check(&draft(vec![s]))
        .shell
        .unwrap()
        .contains("> '$(bad) report.txt'"));
}
#[test]
fn always_is_not_success() {
    let r = check(&draft(vec![
        step("git status", &[], After::Start),
        step("docker compose ps", &[], After::Always),
    ]));
    assert!(r.shell.unwrap().contains(";\n"));
}
#[test]
fn no_match_exit_status_warning() {
    let r = check(&draft(vec![
        step("rg", &["TODO"], After::Start),
        step("git status", &[], After::Success),
    ]));
    assert!(r.warnings.iter().any(|w| w.contains("no-match")));
}
#[test]
fn existing_staged_changes_warning() {
    assert!(
        check(&draft(vec![step("git commit", &["-m", "x"], After::Start)]))
            .warnings
            .iter()
            .any(|w| w.contains("already staged"))
    );
}
#[test]
fn destructive_actions_require_review() {
    assert!(
        check(&draft(vec![step("rm", &["-rf", "target"], After::Start)]))
            .warnings
            .iter()
            .any(|w| w.contains("destructive"))
    );
}
#[test]
fn needs_input_has_no_shell() {
    let r = check(&Draft {
        status: Status::NeedsInput,
        steps: vec![],
        questions: vec!["Which file?".into()],
    });
    assert!(!r.accepted);
    assert!(r.shell.is_none());
}
#[test]
fn non_ok_steps_are_rejected() {
    let mut d = draft(vec![step("git status", &[], After::Start)]);
    d.status = Status::Unsupported;
    assert!(!check(&d).errors.is_empty());
}
#[test]
fn unknown_ir_fields_are_rejected() {
    assert!(Draft::parse(br#"{"status":"ok","steps":[],"questions":[],"execute":true}"#).is_err());
}
#[test]
fn uninstalled_rejected_by_default() {
    let (_, i) = builtins();
    let r = plan::validate(
        &i,
        &draft(vec![step("git status", &[], After::Start)]),
        &BTreeMap::new(),
        &ValidationOptions::default(),
    )
    .unwrap();
    assert!(!r.accepted);
}
#[test]
fn scoped_evidence_is_enforced() {
    let (_, i) = builtins();
    let r = plan::validate(
        &i,
        &draft(vec![step("git status", &[], After::Start)]),
        &BTreeMap::new(),
        &ValidationOptions {
            allow_uninstalled: true,
            allowed_commands: Some(["git log".into()].into()),
            allowed_flags: None,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!r.accepted);
}

#[test]
fn synthesized_plan_must_preserve_explicit_literals() {
    let (_, i) = builtins();
    let r = plan::validate(
        &i,
        &draft(vec![step("git commit", &[], After::Start)]),
        &BTreeMap::new(),
        &ValidationOptions {
            allow_uninstalled: true,
            required_literals: ["release".into()].into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!r.accepted);
    assert!(r.errors.iter().any(|error| error.contains("release")));
}
#[test]
fn synthesized_plan_must_preserve_required_options() {
    let (_, i) = builtins();
    let r = plan::validate(
        &i,
        &draft(vec![step("docker compose up", &[], After::Start)]),
        &BTreeMap::new(),
        &ValidationOptions {
            allow_uninstalled: true,
            required_flags: [("docker compose up".into(), ["--detach".into()].into())].into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!r.accepted);
    assert!(r.errors.iter().any(|error| error.contains("--detach")));
}
#[test]
fn synthesized_plan_must_follow_retrieved_clause_order() {
    let (_, i) = builtins();
    let command_clauses = || {
        Some(
            [
                ("git add".into(), [0].into()),
                ("git commit".into(), [1].into()),
            ]
            .into(),
        )
    };
    let omitted = plan::validate(
        &i,
        &draft(vec![step("git add", &[], After::Start)]),
        &BTreeMap::new(),
        &ValidationOptions {
            allow_uninstalled: true,
            command_clauses: command_clauses(),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!omitted.accepted);
    assert!(omitted.errors.iter().any(|error| error.contains("omitted")));

    let ordered = plan::validate(
        &i,
        &draft(vec![
            step("git add", &[], After::Start),
            step("git commit", &[], After::Success),
        ]),
        &BTreeMap::new(),
        &ValidationOptions {
            allow_uninstalled: true,
            command_clauses: command_clauses(),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(ordered.accepted);

    let r = plan::validate(
        &i,
        &draft(vec![
            step("git commit", &[], After::Start),
            step("git add", &[], After::Success),
        ]),
        &BTreeMap::new(),
        &ValidationOptions {
            allow_uninstalled: true,
            command_clauses: command_clauses(),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!r.accepted);
    assert!(r.errors.iter().any(|error| error.contains("clause order")));
}
#[test]
fn jq_two_values_are_required() {
    assert!(!check(&draft(vec![step("jq", &["--arg", "name"], After::Start)])).accepted);
}
#[test]
fn jq_two_values_are_accepted() {
    assert!(
        check(&draft(vec![step(
            "jq",
            &["--arg", "name", "salt and pepper", "."],
            After::Start
        )]))
        .accepted
    );
}
