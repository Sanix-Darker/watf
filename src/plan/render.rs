use super::{After, Draft, RedirectMode, Status};
use crate::{Error, Result};

/// Quote one literal shell word. This never expands variables, globs, or subshells.
pub fn quote(word: &str) -> String {
    if !word.is_empty()
        && word
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&b))
    {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

pub fn render(plan: &Draft) -> Result<String> {
    if plan.status != Status::Ok || plan.steps.is_empty() || plan.steps.len() > super::MAX_STEPS {
        return Err(Error::message(
            "only a bounded nonempty ok plan can be rendered",
        ));
    }
    let mut groups: Vec<(After, Vec<String>)> = Vec::new();
    for (i, step) in plan.steps.iter().enumerate() {
        if (i == 0 && step.after != After::Start) || (i > 0 && step.after == After::Start) {
            return Err(Error::message("only the first step must use after=start"));
        }
        let mut command = step.command.split(' ').map(quote).collect::<Vec<_>>();
        command.extend(step.args.iter().map(|a| quote(a)));
        let mut command = command.join(" ");
        if let Some(output) = &step.stdout {
            command.push_str(match output.mode {
                RedirectMode::Truncate => " > ",
                RedirectMode::Append => " >> ",
            });
            command.push_str(&quote(&output.path));
        }
        if step.after == After::Pipe {
            if plan.steps[i - 1].stdout.is_some() {
                return Err(Error::message(
                    "pipeline producer cannot also redirect stdout",
                ));
            }
            groups
                .last_mut()
                .ok_or_else(|| Error::message("pipeline cannot start a plan"))?
                .1
                .push(command);
        } else {
            groups.push((step.after, vec![command]));
        }
    }
    let mut rendered = String::new();
    for (i, (after, pipeline)) in groups.into_iter().enumerate() {
        if i > 0 {
            rendered.push_str(match after {
                After::Success => " &&\n",
                After::Always => ";\n",
                _ => return Err(Error::message("invalid group relation")),
            });
        }
        if pipeline.len() > 1 {
            rendered.push_str("(set -o pipefail; ");
            rendered.push_str(&pipeline.join(" | "));
            rendered.push(')');
        } else {
            rendered.push_str(&pipeline[0]);
        }
    }
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_substitution() {
        assert_eq!(
            quote("$(touch /tmp/watf-test)"),
            "'$(touch /tmp/watf-test)'"
        );
    }
    #[test]
    fn apostrophe() {
        assert_eq!(quote("it's"), "'it'\\''s'");
    }
    #[test]
    fn empty_word() {
        assert_eq!(quote(""), "''");
    }
    #[test]
    fn safe_flag() {
        assert_eq!(quote("--file=a.txt"), "--file=a.txt");
    }
}
