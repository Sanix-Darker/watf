//! Direct execution of validated plan argv. No shell evaluation.
use crate::{
    discover::Executable,
    plan::{After, RedirectMode, Report, Status, Step},
    Error, Result,
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

static RAW_CAPTURE_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct Options {
    pub cwd: PathBuf,
    pub timeout_ms: u64,
    pub max_output_bytes: usize,
    pub raw_output_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Output {
    pub text: String,
    pub bytes: usize,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collapsed_lines: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filtered_lines: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_path: Option<PathBuf>,
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
    split: Option<usize>,
    priority: Vec<u8>,
    raw_path: Option<PathBuf>,
}

type CaptureHandle = thread::JoinHandle<Result<Captured>>;

#[cfg(test)]
fn capture(mut reader: impl Read, limit: usize) -> Captured {
    capture_with_recovery(&mut reader, limit, None, false)
        .expect("bounded capture without recovery")
}

fn raw_file(dir: &Path, label: &str) -> Result<(std::fs::File, PathBuf)> {
    let path = dir.join(format!("{label}.raw"));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok((options.open(&path)?, path))
}

fn reserve_raw_namespace(dir: &Path) -> Result<PathBuf> {
    for _ in 0..32 {
        let id = RAW_CAPTURE_ID.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("watf-{}-{id}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(Error::message("could not allocate raw output namespace"))
}

struct RawNamespace {
    path: PathBuf,
}

impl RawNamespace {
    fn reserve(dir: &Path) -> Result<Self> {
        Ok(Self {
            path: reserve_raw_namespace(dir)?,
        })
    }

    fn cleanup_error(self, error: Error) -> Error {
        let token = self.token();
        match fs::remove_dir_all(&self.path) {
            Ok(()) => error,
            Err(cleanup) => Error::message(format!(
                "{token}; cleanup_error={cleanup}; original_error={error}"
            )),
        }
    }

    fn token(&self) -> String {
        let basename = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown");
        format!("raw_namespace={basename}")
    }

    fn remove(self) -> Result<()> {
        fs::remove_dir_all(&self.path).map_err(|error| {
            Error::message(format!(
                "failed to remove raw output namespace {}: {error}",
                self.path.display()
            ))
        })
    }
}

fn capture_with_recovery(
    mut reader: impl Read,
    limit: usize,
    recovery: Option<(PathBuf, String)>,
    keep_diagnostics: bool,
) -> Result<Captured> {
    let head_cap = limit.div_ceil(2);
    let tail_cap = limit / 2;
    let mut head = Vec::with_capacity(head_cap);
    let mut tail = Vec::with_capacity(tail_cap);
    let mut raw = None;
    let mut raw_path = None;
    let mut priority = Vec::new();
    let mut pending = Vec::new();
    let mut total = 0usize;
    let mut chunk = [0u8; 8192];
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        };
        if raw.is_none() && total.saturating_add(n) > limit {
            if let Some((dir, label)) = &recovery {
                let (mut file, path) = raw_file(dir, label)?;
                file.write_all(&head)?;
                file.write_all(&tail)?;
                raw_path = Some(path);
                raw = Some(file);
            }
        }
        if let Some(file) = raw.as_mut() {
            file.write_all(&chunk[..n])?;
        }
        if keep_diagnostics {
            collect_diagnostics(&chunk[..n], &mut pending, &mut priority, limit / 2);
        }
        total = total.saturating_add(n);
        let mut at = 0;
        if head.len() < head_cap {
            let take = (head_cap - head.len()).min(n);
            head.extend_from_slice(&chunk[..take]);
            at = take;
        }
        let rest = &chunk[at..n];
        if tail_cap == 0 || rest.is_empty() {
            continue;
        }
        if rest.len() >= tail_cap {
            tail.clear();
            tail.extend_from_slice(&rest[rest.len() - tail_cap..]);
        } else {
            let overflow = tail
                .len()
                .saturating_add(rest.len())
                .saturating_sub(tail_cap);
            if overflow > 0 {
                tail.copy_within(overflow.., 0);
                tail.truncate(tail.len() - overflow);
            }
            tail.extend_from_slice(rest);
        }
    }
    if total <= limit {
        head.extend(tail);
        return Ok(Captured {
            bytes: head,
            total,
            split: None,
            priority,
            raw_path: None,
        });
    }
    let split = head.len();
    head.extend(tail);
    Ok(Captured {
        bytes: head,
        total,
        split: Some(split),
        priority,
        raw_path,
    })
}

fn collect_diagnostics(chunk: &[u8], pending: &mut Vec<u8>, out: &mut Vec<u8>, cap: usize) {
    if cap == 0 || out.len() >= cap {
        return;
    }
    pending.extend_from_slice(chunk);
    let mut consumed = 0usize;
    while let Some(end) = pending[consumed..].iter().position(|byte| *byte == b'\n') {
        let end = consumed + end + 1;
        let line = &pending[consumed..end];
        let trimmed = line
            .iter()
            .position(|byte| !byte.is_ascii_whitespace())
            .map_or(line, |start| &line[start..]);
        if (trimmed.starts_with(b"warning:") || trimmed.starts_with(b"error:"))
            && out.len().saturating_add(line.len()) <= cap
        {
            out.extend_from_slice(line);
        }
        consumed = end;
    }
    if consumed > 0 {
        pending.drain(..consumed);
    }
    if pending.len() > 8192 {
        pending.clear();
    }
}

fn collapse_repetition(text: String) -> (String, Option<usize>) {
    let mut out = String::with_capacity(text.len());
    let mut lines = text.split_inclusive('\n').peekable();
    let mut collapsed = 0usize;
    while let Some(line) = lines.next() {
        let mut count = 1usize;
        while lines.peek().is_some_and(|next| *next == line) {
            lines.next();
            count += 1;
        }
        out.push_str(line);
        if count > 1 {
            let marker = format!("[watf: previous line repeated {} more times]\n", count - 1);
            if marker.len() < line.len().saturating_mul(count - 1) {
                out.push_str(&marker);
                collapsed += count - 1;
            } else {
                for _ in 1..count {
                    out.push_str(line);
                }
            }
        }
    }
    (out, (collapsed > 0).then_some(collapsed))
}

fn filter_success(command: &str, text: String) -> (String, Option<usize>) {
    if command != "cargo" && !command.starts_with("cargo ") {
        return (text, None);
    }
    let mut out = String::with_capacity(text.len());
    let mut filtered = 0usize;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("Compiling ")
            || trimmed.starts_with("Checking ")
            || trimmed.starts_with("Downloading ")
            || trimmed.starts_with("Downloaded ")
        {
            filtered += 1;
        } else {
            out.push_str(line);
        }
    }
    if filtered == 0 {
        return (text, None);
    }
    let marker = format!("[watf: {filtered} routine cargo progress lines omitted]\n");
    if marker.len() >= text.len().saturating_sub(out.len()) {
        return (text, None);
    }
    out.insert_str(0, &marker);
    (out, Some(filtered))
}

fn restore_diagnostics(
    command: &str,
    diagnostics: &[u8],
    mut text: String,
    budget: usize,
) -> String {
    if diagnostics.is_empty() || (command != "cargo" && !command.starts_with("cargo ")) {
        return text;
    }
    let diagnostics = String::from_utf8_lossy(diagnostics);
    let mut missing = String::new();
    for line in diagnostics.split_inclusive('\n') {
        if !text.contains(line) {
            missing.push_str(line);
        }
    }
    if missing.is_empty() {
        return text;
    }
    let marker = "[watf: retained cargo diagnostics]\n";
    if text
        .len()
        .saturating_add(marker.len())
        .saturating_add(missing.len())
        <= budget
    {
        text.insert_str(0, &format!("{marker}{missing}"));
    }
    text
}

fn output(captured: Captured, reduce: bool, command: &str) -> Output {
    let raw = String::from_utf8_lossy(&captured.bytes).into_owned();
    let (text, collapsed_lines) = match (reduce, captured.split) {
        (true, None) => collapse_repetition(raw),
        (true, Some(split)) => {
            let split = split.min(captured.bytes.len());
            let head = String::from_utf8_lossy(&captured.bytes[..split]).into_owned();
            let tail = String::from_utf8_lossy(&captured.bytes[split..]).into_owned();
            let (head, head_lines) = collapse_repetition(head);
            let (tail, tail_lines) = collapse_repetition(tail);
            let omitted = captured.total.saturating_sub(captured.bytes.len());
            let marker = format!("[watf: {omitted} bytes omitted]\n");
            let reduced = format!("{head}{marker}{tail}");
            if reduced.len() < raw.len() {
                (
                    reduced,
                    Some(head_lines.unwrap_or(0) + tail_lines.unwrap_or(0)),
                )
            } else {
                (raw, None)
            }
        }
        (false, _) => (raw, None),
    };
    let (text, filtered_lines) = if reduce {
        filter_success(command, text)
    } else {
        (text, None)
    };
    let text = if reduce && captured.total > captured.bytes.len() {
        restore_diagnostics(command, &captured.priority, text, captured.bytes.len())
    } else {
        text
    };
    Output {
        truncated: captured.total > captured.bytes.len(),
        bytes: captured.total,
        text,
        collapsed_lines,
        filtered_lines,
        raw_path: captured.raw_path,
    }
}

fn empty_output() -> Output {
    Output {
        text: String::new(),
        bytes: 0,
        truncated: false,
        collapsed_lines: None,
        filtered_lines: None,
        raw_path: None,
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

#[cfg(unix)]
fn spawn_direct(command: &mut Command) -> std::io::Result<Child> {
    command.process_group(0).spawn()
}

#[cfg(not(unix))]
fn spawn_direct(command: &mut Command) -> std::io::Result<Child> {
    command.spawn()
}

#[cfg(unix)]
fn stop_groups(process_groups: &[libc::pid_t], children: &mut [Child]) {
    for group in process_groups {
        unsafe {
            libc::kill(-*group, libc::SIGKILL);
        }
    }
    stop(children);
}

#[cfg(not(unix))]
fn stop_groups(_: &[()], children: &mut [Child]) {
    stop(children);
}

fn spawn_capture(
    name: &str,
    f: impl FnOnce() -> Result<Captured> + Send + 'static,
) -> Result<CaptureHandle> {
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(f)
        .map_err(|error| Error::message(format!("{name} capture thread: {error}")))
}

#[cfg(unix)]
fn abort_group(
    error: Error,
    process_groups: &[libc::pid_t],
    children: &mut [Child],
    stderr_threads: Vec<CaptureHandle>,
    final_stdout: Option<CaptureHandle>,
) -> Error {
    stop_groups(process_groups, children);
    let _ = drain_captures(stderr_threads, final_stdout);
    error
}

#[cfg(not(unix))]
fn abort_group(
    error: Error,
    process_groups: &[()],
    children: &mut [Child],
    stderr_threads: Vec<CaptureHandle>,
    final_stdout: Option<CaptureHandle>,
) -> Error {
    stop_groups(process_groups, children);
    let _ = drain_captures(stderr_threads, final_stdout);
    error
}

fn join_capture(
    handle: CaptureHandle,
    name: &str,
    first_error: &mut Option<Error>,
) -> Option<Captured> {
    match handle.join() {
        Ok(Ok(captured)) => Some(captured),
        Ok(Err(error)) => {
            if first_error.is_none() {
                *first_error = Some(error);
            }
            None
        }
        Err(_) => {
            if first_error.is_none() {
                *first_error = Some(Error::message(format!("{name} capture thread failed")));
            }
            None
        }
    }
}

fn drain_captures(
    stderr_threads: Vec<CaptureHandle>,
    final_stdout: Option<CaptureHandle>,
) -> Result<(Vec<Captured>, Option<Captured>)> {
    let mut first_error = None;
    let mut stderrs = Vec::with_capacity(stderr_threads.len());
    for handle in stderr_threads {
        if let Some(captured) = join_capture(handle, "stderr", &mut first_error) {
            stderrs.push(captured);
        }
    }
    let stdout = final_stdout.and_then(|handle| join_capture(handle, "stdout", &mut first_error));
    match first_error {
        Some(error) => Err(error),
        None => Ok((stderrs, stdout)),
    }
}

fn run_group(
    steps: &[Step],
    first: usize,
    inventory: &BTreeMap<String, Executable>,
    options: &Options,
    raw_output_dir: Option<&Path>,
    deadline: Instant,
) -> Result<(Vec<StepResult>, bool, bool)> {
    let started = Instant::now();
    let mut children = Vec::with_capacity(steps.len());
    let mut stderr_threads = Vec::with_capacity(steps.len());
    let mut previous_stdout = None;
    let mut final_stdout = None;
    #[cfg(unix)]
    let mut process_groups = Vec::with_capacity(steps.len());
    #[cfg(not(unix))]
    let process_groups: [(); 0] = [];
    let mut prepared = Vec::with_capacity(steps.len());

    for (offset, step) in steps.iter().enumerate() {
        let words = argv(step);
        let root = words
            .first()
            .ok_or_else(|| Error::message("validated step has no executable"))?;
        let executable = inventory.get(root).ok_or_else(|| {
            Error::message(format!("{root} disappeared from the executable inventory"))
        })?;
        let last = offset + 1 == steps.len();
        let stdout = if last {
            redirect(step, &options.cwd)?
        } else {
            None
        };
        prepared.push((words, executable.path.clone(), stdout));
    }

    for (offset, ((words, path, stdout), step)) in prepared.into_iter().zip(steps).enumerate() {
        let mut command = Command::new(path);
        command
            .args(&words[1..])
            .current_dir(&options.cwd)
            .stdin(previous_stdout.take().map_or_else(Stdio::null, Stdio::from))
            .stderr(Stdio::piped());
        let last = offset + 1 == steps.len();
        if last {
            command.stdout(stdout.unwrap_or_else(Stdio::piped));
        } else {
            command.stdout(Stdio::piped());
        }
        match spawn_direct(&mut command) {
            Ok(child) => {
                #[cfg(unix)]
                if let Ok(group) = libc::pid_t::try_from(child.id()) {
                    process_groups.push(group);
                }
                children.push(child);
                let child = children.last_mut().unwrap();
                let stderr = match child.stderr.take() {
                    Some(stderr) => stderr,
                    None => {
                        return Err(abort_group(
                            Error::message("failed to capture stderr"),
                            &process_groups,
                            &mut children,
                            stderr_threads,
                            final_stdout,
                        ));
                    }
                };
                let limit = options.max_output_bytes;
                let recovery = raw_output_dir
                    .map(|dir| (dir.to_path_buf(), format!("step-{}-stderr", first + offset)));
                let keep_diagnostics =
                    step.command == "cargo" || step.command.starts_with("cargo ");
                let stderr_handle = match spawn_capture("stderr", move || {
                    capture_with_recovery(stderr, limit, recovery, keep_diagnostics)
                }) {
                    Ok(handle) => handle,
                    Err(error) => {
                        return Err(abort_group(
                            error,
                            &process_groups,
                            &mut children,
                            stderr_threads,
                            final_stdout,
                        ));
                    }
                };
                stderr_threads.push(stderr_handle);
                if !last {
                    previous_stdout = match child.stdout.take() {
                        Some(stdout) => Some(stdout),
                        None => {
                            return Err(abort_group(
                                Error::message("failed to capture stdout"),
                                &process_groups,
                                &mut children,
                                stderr_threads,
                                final_stdout,
                            ));
                        }
                    };
                } else if step.stdout.is_none() {
                    let limit = options.max_output_bytes;
                    let recovery = raw_output_dir
                        .map(|dir| (dir.to_path_buf(), format!("step-{}-stdout", first + offset)));
                    let keep_diagnostics =
                        step.command == "cargo" || step.command.starts_with("cargo ");
                    let stdout = match child.stdout.take() {
                        Some(stdout) => stdout,
                        None => {
                            return Err(abort_group(
                                Error::message("failed to capture stdout"),
                                &process_groups,
                                &mut children,
                                stderr_threads,
                                final_stdout,
                            ));
                        }
                    };
                    let stdout_handle = match spawn_capture("stdout", move || {
                        capture_with_recovery(stdout, limit, recovery, keep_diagnostics)
                    }) {
                        Ok(handle) => handle,
                        Err(error) => {
                            return Err(abort_group(
                                error,
                                &process_groups,
                                &mut children,
                                stderr_threads,
                                final_stdout,
                            ));
                        }
                    };
                    final_stdout = Some(stdout_handle);
                }
            }
            Err(error) => {
                return Err(abort_group(
                    error.into(),
                    &process_groups,
                    &mut children,
                    stderr_threads,
                    final_stdout,
                ));
            }
        }
    }

    let mut timed_out = false;
    loop {
        let mut done = true;
        for child in &mut children {
            match child.try_wait() {
                Ok(None) => done = false,
                Ok(Some(_)) => {}
                Err(error) => {
                    return Err(abort_group(
                        error.into(),
                        &process_groups,
                        &mut children,
                        stderr_threads,
                        final_stdout,
                    ));
                }
            }
        }
        let captures_done = stderr_threads.iter().all(|handle| handle.is_finished())
            && final_stdout
                .as_ref()
                .is_none_or(|handle| handle.is_finished());
        if done && captures_done {
            break;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            stop_groups(&process_groups, &mut children);
            break;
        }
        let elapsed = started.elapsed();
        if elapsed < Duration::from_micros(250) {
            thread::yield_now();
        } else if elapsed < Duration::from_millis(2) {
            thread::sleep(Duration::from_micros(50));
        } else {
            thread::sleep(Duration::from_millis(1));
        }
    }

    let mut statuses = Vec::with_capacity(children.len());
    for child in &mut children {
        match child.try_wait() {
            Ok(status) => statuses.push(status.or_else(|| child.wait().ok())),
            Err(error) => {
                return Err(abort_group(
                    error.into(),
                    &process_groups,
                    &mut children,
                    stderr_threads,
                    final_stdout,
                ));
            }
        }
    }
    let (stderrs, stdout) = drain_captures(stderr_threads, final_stdout)?;
    let success = !timed_out
        && statuses
            .iter()
            .all(|status| status.as_ref().is_some_and(|s| s.success()));
    let stderrs: Vec<_> = stderrs
        .into_iter()
        .zip(steps)
        .map(|(captured, step)| output(captured, success, &step.command))
        .collect();
    let stdout = stdout.map_or_else(empty_output, |captured| {
        output(captured, success, &steps.last().unwrap().command)
    });
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
    if options
        .raw_output_dir
        .as_ref()
        .is_some_and(|path| !path.is_dir())
    {
        return Err(Error::message(
            "raw-output-dir must be an existing directory",
        ));
    }
    let raw_namespace = options
        .raw_output_dir
        .as_deref()
        .map(RawNamespace::reserve)
        .transpose()?;
    let raw_namespace_path = raw_namespace
        .as_ref()
        .map(|namespace| namespace.path.as_path());

    let result = (|| {
        let started = Instant::now();
        let deadline = started + Duration::from_millis(options.timeout_ms);
        let mut results = Vec::with_capacity(report.steps.len());
        let mut previous_success = true;
        let mut failed = false;
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
                    raw_namespace_path,
                    deadline,
                )?;
                results.append(&mut group);
                previous_success = success;
                failed |= !success && !timeout;
                timed_out |= timeout;
            } else {
                results.extend(report.steps[first..end].iter().enumerate().map(
                    |(offset, step)| StepResult {
                        step: first + offset,
                        argv: argv(step),
                        exit_code: None,
                        skipped: true,
                        timed_out: false,
                        stdout: empty_output(),
                        stderr: empty_output(),
                        elapsed_ms: 0,
                    },
                ));
            }
            first = end;
        }
        Ok(Execution {
            status: if timed_out {
                "timeout"
            } else if failed {
                "failed"
            } else {
                "ok"
            },
            steps: results,
            elapsed_ms: started.elapsed().as_millis(),
        })
    })();

    match result {
        Ok(execution) => {
            if let Some(namespace) = raw_namespace {
                if !execution
                    .steps
                    .iter()
                    .any(|step| step.stdout.raw_path.is_some() || step.stderr.raw_path.is_some())
                {
                    namespace.remove()?;
                }
            }
            Ok(execution)
        }
        Err(error) => Err(match raw_namespace {
            Some(namespace) => namespace.cleanup_error(error),
            None => error,
        }),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::plan::{Report, Step};
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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
                raw_output_dir: None,
            },
        )
        .unwrap();
        assert_eq!(result.status, "ok");
        assert_eq!(result.steps[0].stdout.text, "literal $(no-shell)");
        assert_eq!(result.steps[0].stderr.text, "diagnostic");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn execution_timeout_is_enforced() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("watf-timeout-{}-{unique}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let program = dir.join("fixture");
        fs::write(&program, "#!/bin/sh\nsleep 1 &\n").unwrap();
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
            args: vec![],
            after: After::Start,
            stdout: None,
        });
        let started = Instant::now();
        let result = execute(
            &report,
            &inventory,
            &Options {
                cwd: dir.clone(),
                timeout_ms: 10,
                max_output_bytes: 256,
                raw_output_dir: None,
            },
        )
        .unwrap();
        assert_eq!(result.status, "timeout");
        assert!(result.steps[0].timed_out);
        assert!(started.elapsed() < Duration::from_millis(500));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn abort_group_kills_child_and_drains_captures() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("watf-abort-{}-{unique}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let program = dir.join("fixture");
        fs::write(&program, "#!/bin/sh\nsleep 1\n").unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        let mut command = Command::new(&program);
        command
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = spawn_direct(&mut command).unwrap();
        let groups = vec![libc::pid_t::try_from(child.id()).unwrap()];
        let stderr = child.stderr.take().unwrap();
        let handle = spawn_capture("stderr", move || {
            capture_with_recovery(stderr, 256, None, false)
        })
        .unwrap();
        let mut children = vec![child];
        let error = abort_group(
            Error::message("original failure"),
            &groups,
            &mut children,
            vec![handle],
            None,
        );
        assert_eq!(error.to_string(), "original failure");
        assert!(children[0].try_wait().unwrap().is_some());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cleanup_error_compaction_keeps_namespace_token() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("watf-cleanup-{}-{unique}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("watf-123-456");
        fs::write(&path, "not a directory").unwrap();
        let error = RawNamespace { path: path.clone() }
            .cleanup_error(Error::message("original ".repeat(80)))
            .to_string();
        let compact = crate::text::compact_bytes(&error, 96);
        assert!(compact.starts_with("raw_namespace=watf-123-456;"));
        assert!(compact.contains("raw_namespace=watf-123-456"));
        fs::remove_file(path).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pipeline_preflight_failure_does_not_spawn_or_leave_namespace() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("watf-pipe-{}-{unique}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let raw_dir = dir.join("raw");
        fs::create_dir(&raw_dir).unwrap();
        let marker = dir.join("ran");
        let program = dir.join("fixture");
        fs::write(
            &program,
            format!("#!/bin/sh\nprintf ran > '{}'\n", marker.display()),
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
        let mut report = accepted(Step {
            command: "fixture".to_owned(),
            args: vec![],
            after: After::Start,
            stdout: None,
        });
        report.steps.push(Step {
            command: "missing".to_owned(),
            args: vec![],
            after: After::Pipe,
            stdout: None,
        });
        let error = execute(
            &report,
            &inventory,
            &Options {
                cwd: dir.clone(),
                timeout_ms: 1000,
                max_output_bytes: 256,
                raw_output_dir: Some(raw_dir.clone()),
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("disappeared"));
        assert!(!marker.exists());
        assert!(fs::read_dir(raw_dir).unwrap().next().is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn partial_pipeline_spawn_failure_cleans_namespace() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("watf-spawn-{}-{unique}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let raw_dir = dir.join("raw");
        fs::create_dir(&raw_dir).unwrap();
        let program = dir.join("fixture");
        fs::write(
            &program,
            "#!/bin/sh\ni=0\nwhile [ $i -lt 100 ]; do printf '0123456789abcdef0123456789abcdef\\n' >&2; i=$((i+1)); done\nsleep 1\n",
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
        inventory.insert(
            "bad".to_owned(),
            Executable {
                path: dir.join("missing-executable"),
                bytes: 0,
                modified_unix: None,
            },
        );
        let mut report = accepted(Step {
            command: "fixture".to_owned(),
            args: vec![],
            after: After::Start,
            stdout: None,
        });
        report.steps.push(Step {
            command: "bad".to_owned(),
            args: vec![],
            after: After::Pipe,
            stdout: None,
        });
        assert!(execute(
            &report,
            &inventory,
            &Options {
                cwd: dir.clone(),
                timeout_ms: 1000,
                max_output_bytes: 256,
                raw_output_dir: Some(raw_dir.clone()),
            },
        )
        .is_err());
        assert!(fs::read_dir(raw_dir).unwrap().next().is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn capture_keeps_head_and_tail() {
        let captured = output(capture(&b"0123456789abcdef"[..], 8), true, "fixture");
        assert!(captured.truncated);
        assert_eq!(captured.bytes, 16);
        assert_eq!(captured.text, "0123cdef");
        assert_eq!(captured.collapsed_lines, None);
    }

    #[test]
    fn capture_retries_interrupted_and_returns_read_errors() {
        struct Reader {
            reads: Vec<std::io::Result<&'static [u8]>>,
        }
        impl Read for Reader {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                match self.reads.remove(0) {
                    Ok(bytes) => {
                        out[..bytes.len()].copy_from_slice(bytes);
                        Ok(bytes.len())
                    }
                    Err(error) => Err(error),
                }
            }
        }

        let captured = capture_with_recovery(
            Reader {
                reads: vec![
                    Err(std::io::ErrorKind::Interrupted.into()),
                    Ok(b"ok"),
                    Ok(b""),
                ],
            },
            256,
            None,
            false,
        )
        .unwrap();
        assert_eq!(output(captured, true, "fixture").text, "ok");

        let error = match capture_with_recovery(
            Reader {
                reads: vec![Err(std::io::Error::other("boom"))],
            },
            256,
            None,
            false,
        ) {
            Ok(_) => panic!("read error should propagate"),
            Err(error) => error,
        };
        assert_eq!(error.to_string(), "boom");
    }

    #[test]
    fn successful_repetition_is_collapsed_loss_aware() {
        let line = "same diagnostic line with enough bytes to make reduction useful\n";
        let raw = line.repeat(20);
        let captured = output(capture(raw.as_bytes(), 4096), true, "fixture");
        assert!(!captured.truncated);
        assert_eq!(captured.bytes, raw.len());
        assert_eq!(captured.collapsed_lines, Some(19));
        assert!(captured.text.starts_with(line));
        assert!(captured
            .text
            .contains("previous line repeated 19 more times"));

        let failed = output(capture(raw.as_bytes(), 4096), false, "fixture");
        assert_eq!(failed.text, raw);
        assert_eq!(failed.collapsed_lines, None);
    }

    #[test]
    fn successful_cargo_progress_is_filtered_but_diagnostics_survive() {
        let raw = concat!(
            "   Compiling alpha v0.1.0\n",
            "    Checking beta v0.1.0\n",
            "warning: useful diagnostic\n",
            "    Finished `dev` profile in 1.00s\n",
        );
        let reduced = output(capture(raw.as_bytes(), 4096), true, "cargo check");
        assert_eq!(reduced.filtered_lines, Some(2));
        assert!(reduced
            .text
            .contains("2 routine cargo progress lines omitted"));
        assert!(reduced.text.contains("warning: useful diagnostic"));
        assert!(reduced.text.contains("Finished `dev` profile"));
        assert!(!reduced.text.contains("Compiling alpha"));

        let failed = output(capture(raw.as_bytes(), 4096), false, "cargo check");
        assert_eq!(failed.filtered_lines, None);
        assert_eq!(failed.text, raw);
    }

    #[test]
    fn truncated_cargo_progress_keeps_truncation_and_diagnostics() {
        let progress = "   Compiling alpha v0.1.0 with enough detail to occupy space\n";
        let raw = format!(
            "{}warning: middle diagnostic\n{}error: tail diagnostic\n",
            progress.repeat(20),
            progress.repeat(20),
        );
        let captured = capture_with_recovery(raw.as_bytes(), 1024, None, true).unwrap();
        let reduced = output(captured, true, "cargo check");
        assert!(reduced.truncated);
        assert_eq!(reduced.bytes, raw.len());
        assert!(reduced.text.contains("warning: middle diagnostic"));
        assert!(reduced.text.contains("error: tail diagnostic"));
        assert!(reduced.filtered_lines.unwrap_or(0) > 0);
    }

    #[test]
    fn successful_truncated_repetition_collapses_only_captured_sides() {
        let line = "same successful line with enough bytes to make reduction useful\n";
        let raw = line.repeat(200);
        let captured = output(capture(raw.as_bytes(), 1024), true, "fixture");
        assert!(captured.truncated);
        assert_eq!(captured.bytes, raw.len());
        assert!(captured.collapsed_lines.unwrap_or(0) > 0);
        assert!(captured.text.contains("bytes omitted"));
        assert!(captured.text.len() < 1024);

        let failed = output(capture(raw.as_bytes(), 1024), false, "fixture");
        assert!(failed.truncated);
        assert_eq!(failed.text.len(), 1024);
        assert_eq!(failed.collapsed_lines, None);
    }

    #[test]
    fn truncated_output_can_be_recovered_exactly_when_requested() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("watf-raw-{}-{unique}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let raw = "0123456789abcdef\n".repeat(64);
        let captured = capture_with_recovery(
            raw.as_bytes(),
            256,
            Some((dir.clone(), "stdout".to_owned())),
            false,
        )
        .unwrap();
        let output = output(captured, true, "fixture");
        let path = output.raw_path.unwrap();
        assert!(output.truncated);
        assert_eq!(fs::read(&path).unwrap(), raw.as_bytes());

        let short = capture_with_recovery(
            b"small output".as_slice(),
            256,
            Some((dir.clone(), "short".to_owned())),
            false,
        )
        .unwrap();
        assert!(short.raw_path.is_none());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn later_always_success_does_not_hide_failure() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("watf-exec-{}-{unique}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let program = dir.join("fixture");
        fs::write(&program, "#!/bin/sh\n[ \"$1\" = ok ]\n").unwrap();
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
        let mut report = accepted(Step {
            command: "fixture".to_owned(),
            args: vec!["fail".to_owned()],
            after: After::Start,
            stdout: None,
        });
        report.steps.push(Step {
            command: "fixture".to_owned(),
            args: vec!["ok".to_owned()],
            after: After::Always,
            stdout: None,
        });
        let result = execute(
            &report,
            &inventory,
            &Options {
                cwd: dir.clone(),
                timeout_ms: 1000,
                max_output_bytes: 256,
                raw_output_dir: None,
            },
        )
        .unwrap();
        assert_eq!(result.status, "failed");
        assert_eq!(result.steps[0].exit_code, Some(1));
        assert_eq!(result.steps[1].exit_code, Some(0));
        fs::remove_dir_all(dir).unwrap();
    }
}
