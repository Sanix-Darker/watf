//! Direct execution of validated plan argv. No shell evaluation.
use crate::{
    discover::Executable,
    plan::{After, RedirectMode, Report, Status, Step},
    Error, Result,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, VecDeque},
    fs::OpenOptions,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
pub struct Options {
    pub cwd: PathBuf,
    pub timeout_ms: u64,
    pub max_output_bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Output {
    pub text: String,
    pub bytes: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepResult {
    pub step: usize,
    pub argv: Vec<String>,
    pub exit_code: Option<i32>,
    pub skipped: bool,
    pub timed_out: bool,
    pub stdout: Output,
    pub stderr: Output,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
pub struct Execution {
    pub status: &'static str,
    pub steps: Vec<StepResult>,
    pub elapsed_ms: u128,
}

struct Captured {
    bytes: Vec<u8>,
    total: usize,
}

fn capture(mut reader: impl Read, limit: usize) -> Captured {
    let head_cap = limit.div_ceil(2);
    let tail_cap = limit / 2;
    let mut head = Vec::with_capacity(head_cap);
    let mut tail = VecDeque::with_capacity(tail_cap);
    let mut total = 0usize;
    let mut chunk = [0u8; 8192];
    while let Ok(n) = reader.read(&mut chunk) {
        if n == 0 {
            break;
        }
        total = total.saturating_add(n);
        let mut at = 0;
        if head.len() < head_cap {
            let take = (head_cap - head.len()).min(n);
            head.extend_from_slice(&chunk[..take]);
            at = take;
        }
        for &byte in &chunk[at..n] {
            if tail_cap == 0 {
                break;
            }
            if tail.len() == tail_cap {
                tail.pop_front();
            }
            tail.push_back(byte);
        }
    }
    if total <= limit {
        head.extend(tail);
        return Captured { bytes: head, total };
    }
    head.extend(tail);
    Captured { bytes: head, total }
}

fn output(captured: Captured) -> Output {
    Output {
        truncated: captured.total > captured.bytes.len(),
        bytes: captured.total,
        text: String::from_utf8_lossy(&captured.bytes).into_owned(),
    }
}

fn empty_output() -> Output {
    Output {
        text: String::new(),
        bytes: 0,
        truncated: false,
    }
}

fn argv(step: &Step) -> Vec<String> {
    step.command
        .split_whitespace()
        .chain(step.args.iter().map(String::as_str))
        .map(str::to_owned)
        .collect()
}

fn redirect(step: &Step, cwd: &Path) -> Result<Option<Stdio>> {
    let Some(output) = &step.stdout else {
        return Ok(None);
    };
    let path = Path::new(&output.path);
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        cwd.join(path)
    };
    let mut file = OpenOptions::new();
    file.create(true).write(true);
    match output.mode {
        RedirectMode::Truncate => {
            file.truncate(true);
        }
        RedirectMode::Append => {
            file.append(true);
        }
    }
    Ok(Some(Stdio::from(file.open(path)?)))
}

fn stop(children: &mut [Child]) {
    for child in children.iter_mut() {
        let _ = child.kill();
    }
    for child in children.iter_mut() {
        let _ = child.wait();
    }
}

fn run_group(
    steps: &[Step],
    first: usize,
    inventory: &BTreeMap<String, Executable>,
    options: &Options,
    deadline: Instant,
) -> Result<(Vec<StepResult>, bool, bool)> {
    let started = Instant::now();
    let mut children = Vec::with_capacity(steps.len());
    let mut stderr_threads = Vec::with_capacity(steps.len());
    let mut previous_stdout = None;
    let mut final_stdout = None;

    for (offset, step) in steps.iter().enumerate() {
        let words = argv(step);
        let root = words
            .first()
            .ok_or_else(|| Error::message("validated step has no executable"))?;
        let executable = inventory.get(root).ok_or_else(|| {
            Error::message(format!("{root} disappeared from the executable inventory"))
        })?;
        let mut command = Command::new(&executable.path);
        command
            .args(&words[1..])
            .current_dir(&options.cwd)
            .stdin(previous_stdout.take().map_or_else(Stdio::null, Stdio::from))
            .stderr(Stdio::piped());
        let last = offset + 1 == steps.len();
        if last {
            command.stdout(redirect(step, &options.cwd)?.unwrap_or_else(Stdio::piped));
        } else {
            command.stdout(Stdio::piped());
        }
        match command.spawn() {
            Ok(mut child) => {
                let stderr = child
                    .stderr
                    .take()
                    .ok_or_else(|| Error::message("failed to capture stderr"))?;
                let limit = options.max_output_bytes;
                stderr_threads.push(thread::spawn(move || capture(stderr, limit)));
                if !last {
                    previous_stdout = child.stdout.take();
                } else if step.stdout.is_none() {
                    let limit = options.max_output_bytes;
                    final_stdout = child
                        .stdout
                        .take()
                        .map(|stdout| thread::spawn(move || capture(stdout, limit)));
                }
                children.push(child);
            }
            Err(error) => {
                stop(&mut children);
                return Err(error.into());
            }
        }
    }

    let mut timed_out = false;
    loop {
        let mut done = true;
        for child in &mut children {
            if child.try_wait()?.is_none() {
                done = false;
            }
        }
        if done {
            break;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            stop(&mut children);
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }

    let mut statuses = Vec::with_capacity(children.len());
    for child in &mut children {
        statuses.push(child.try_wait()?.or_else(|| child.wait().ok()));
    }
    let mut stderrs = Vec::with_capacity(stderr_threads.len());
    for handle in stderr_threads {
        stderrs.push(output(
            handle
                .join()
                .map_err(|_| Error::message("stderr capture thread failed"))?,
        ));
    }
    let stdout = match final_stdout {
        Some(handle) => output(
            handle
                .join()
                .map_err(|_| Error::message("stdout capture thread failed"))?,
        ),
        None => empty_output(),
    };
    let success = !timed_out
        && statuses
            .iter()
            .all(|status| status.as_ref().is_some_and(|s| s.success()));
    let elapsed = started.elapsed().as_millis();
    let last = steps.len().saturating_sub(1);
    let results = steps
        .iter()
        .enumerate()
        .map(|(offset, step)| StepResult {
            step: first + offset,
            argv: argv(step),
            exit_code: statuses[offset].as_ref().and_then(|s| s.code()),
            skipped: false,
            timed_out,
            stdout: if offset == last {
                stdout.clone()
            } else {
                empty_output()
            },
            stderr: stderrs[offset].clone(),
            elapsed_ms: elapsed,
        })
        .collect();
    Ok((results, success, timed_out))
}

pub fn execute(
    report: &Report,
    inventory: &BTreeMap<String, Executable>,
    options: &Options,
) -> Result<Execution> {
    if !report.accepted || report.status != Status::Ok || report.steps.is_empty() {
        return Err(Error::message("only an accepted ok plan can execute"));
    }
    if !options.cwd.is_dir() {
        return Err(Error::message(
            "execution cwd must be an existing directory",
        ));
    }
    if !(1..=600_000).contains(&options.timeout_ms)
        || !(256..=1_048_576).contains(&options.max_output_bytes)
    {
        return Err(Error::message(
            "timeout-ms must be 1..600000 and max-output-bytes must be 256..1048576",
        ));
    }

    let started = Instant::now();
    let deadline = started + Duration::from_millis(options.timeout_ms);
    let mut results = Vec::with_capacity(report.steps.len());
    let mut previous_success = true;
    let mut timed_out = false;
    let mut first = 0;
    while first < report.steps.len() {
        let mut end = first + 1;
        while end < report.steps.len() && report.steps[end].after == After::Pipe {
            end += 1;
        }
        let relation = report.steps[first].after;
        let should_run = first == 0 || relation == After::Always || previous_success;
        if should_run && !timed_out {
            let (mut group, success, timeout) = run_group(
                &report.steps[first..end],
                first,
                inventory,
                options,
                deadline,
            )?;
            results.append(&mut group);
            previous_success = success;
            timed_out |= timeout;
        } else {
            results.extend(
                report.steps[first..end]
                    .iter()
                    .enumerate()
                    .map(|(offset, step)| StepResult {
                        step: first + offset,
                        argv: argv(step),
                        exit_code: None,
                        skipped: true,
                        timed_out: false,
                        stdout: empty_output(),
                        stderr: empty_output(),
                        elapsed_ms: 0,
                    }),
            );
        }
        first = end;
    }
    Ok(Execution {
        status: if timed_out {
            "timeout"
        } else if previous_success {
            "ok"
        } else {
            "failed"
        },
        steps: results,
        elapsed_ms: started.elapsed().as_millis(),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::plan::{Report, Step};
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn accepted(step: Step) -> Report {
        Report {
            schema_version: crate::SCHEMA_VERSION,
            status: Status::Ok,
            accepted: true,
            executed: false,
            approval_required: true,
            validation_scope: "test",
            steps: vec![step],
            evidence: vec![],
            errors: vec![],
            warnings: vec![],
            questions: vec![],
            shell: None,
            shell_dialect: "bash",
        }
    }

    #[test]
    fn direct_argv_and_bounded_streams() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("watf-exec-{}-{unique}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let program = dir.join("fixture");
        fs::write(
            &program,
            "#!/bin/sh\nprintf '%s' \"$1\"\nprintf '%s' \"$2\" >&2\n",
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        let mut inventory = BTreeMap::new();
        inventory.insert(
            "fixture".to_owned(),
            Executable {
                path: program,
                bytes: 0,
                modified_unix: None,
            },
        );
        let report = accepted(Step {
            command: "fixture".to_owned(),
            args: vec!["literal $(no-shell)".to_owned(), "diagnostic".to_owned()],
            after: After::Start,
            stdout: None,
        });
        let result = execute(
            &report,
            &inventory,
            &Options {
                cwd: dir.clone(),
                timeout_ms: 1000,
                max_output_bytes: 256,
            },
        )
        .unwrap();
        assert_eq!(result.status, "ok");
        assert_eq!(result.steps[0].stdout.text, "literal $(no-shell)");
        assert_eq!(result.steps[0].stderr.text, "diagnostic");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn capture_keeps_head_and_tail() {
        let captured = output(capture(&b"0123456789abcdef"[..], 8));
        assert!(captured.truncated);
        assert_eq!(captured.bytes, 16);
        assert_eq!(captured.text, "0123cdef");
    }
}
