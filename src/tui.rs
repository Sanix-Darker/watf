//! Minimal terminal UI with RAII cleanup. Enter retrieves; Ctrl+P plans explicitly.
use crate::{
    infer,
    packet::{Engine, Options},
    Error, Result,
};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute, queue,
    style::Print,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{self, IsTerminal, Write};

struct Screen;
impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), cursor::Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}
pub fn run(engine: &mut Engine, options: &Options, model: Option<&infer::Options>) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Error::message("tui needs an interactive terminal"));
    }
    terminal::enable_raw_mode()?;
    let _screen = Screen;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen, cursor::Hide)?;
    let mut input = String::new();
    let mut rows: Vec<String> = vec!["Enter an intent. No command name is required.".to_owned()];
    let mut offset = 0;
    loop {
        let (width, height) = terminal::size()?;
        let width = usize::from(width.saturating_sub(1));
        queue!(
            out,
            cursor::MoveTo(0, 0),
            Clear(ClearType::All),
            Print("watf: what tf ?  Enter: search  Ctrl+P: plan  Esc: quit")
        )?;
        queue!(
            out,
            cursor::MoveTo(0, 2),
            Print(crate::text::compact(&format!("> {input}"), width))
        )?;
        for (row, text) in rows
            .iter()
            .skip(offset)
            .take(usize::from(height.saturating_sub(5)))
            .enumerate()
        {
            queue!(
                out,
                cursor::MoveTo(0, row as u16 + 4),
                Print(crate::text::compact(text, width))
            )?;
        }
        out.flush()?;
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Esc => break,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    rows = if let Some(model) = model {
                        match crate::cli::plan_request(engine, &input, options, model, false) {
                            Ok(response) => {
                                let mut lines = response
                                    .report
                                    .shell
                                    .unwrap_or_default()
                                    .lines()
                                    .map(str::to_owned)
                                    .collect::<Vec<_>>();
                                lines.extend(response.report.errors);
                                lines.extend(response.report.questions);
                                lines.extend(
                                    response
                                        .report
                                        .warnings
                                        .into_iter()
                                        .map(|w| format!("Review: {w}")),
                                );
                                lines
                            }
                            Err(e) => vec![e.to_string()],
                        }
                    } else {
                        vec!["No model configured. Set WATF_MODEL to enable explicit local planning.".to_owned()]
                    };
                    offset = 0;
                }
                KeyCode::Char(ch)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    if input.len() + ch.len_utf8() <= crate::text::MAX_QUERY_BYTES {
                        input.push(ch);
                    }
                }
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Enter => {
                    rows = match engine.lookup(&input, options) {
                        Ok(packet) => {
                            let mut lines = vec![format!(
                                "{}: {} records, {} bytes",
                                packet.status,
                                packet.evidence.len(),
                                packet.json()?.len()
                            )];
                            for e in packet.evidence {
                                lines.push(format!("{} {}", e.command, e.name));
                                lines.push(format!("  {}", e.summary));
                            }
                            lines
                        }
                        Err(e) => vec![e.to_string()],
                    };
                    offset = 0;
                }
                KeyCode::Down | KeyCode::PageDown => {
                    offset = (offset + 1).min(rows.len().saturating_sub(1));
                }
                KeyCode::Up | KeyCode::PageUp => {
                    offset = offset.saturating_sub(1);
                }
                _ => {}
            },
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
    Ok(())
}
