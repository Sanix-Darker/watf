//! GBNF constrains syntax and command vocabulary, not task correctness.
use crate::{plan::Context, Error, Result};

pub fn for_context(context: &Context) -> Result<String> {
    if context.commands.is_empty() {
        return Err(Error::message("grammar needs command evidence"));
    }
    let mut terminals = Vec::new();
    for command in &context.commands {
        // The first quoting emits a JSON string; the second a GBNF terminal.
        terminals.push(serde_json::to_string(&serde_json::to_string(
            &command.command,
        )?)?);
    }
    let mut grammar = String::from(
        r#"root ::= "{" ws "\"status\":" ws status "," ws "\"steps\":" ws steps "," ws "\"questions\":" ws strings "}" ws
status ::= "\"ok\"" | "\"needs_input\"" | "\"unsupported\""
steps ::= "[" ws (step ("," ws step){0,7})? "]" ws
step ::= "{" ws "\"command\":" ws command "," ws "\"args\":" ws strings "," ws "\"after\":" ws after "," ws "\"stdout\":" ws redirect "}" ws
after ::= "\"start\"" | "\"success\"" | "\"always\"" | "\"pipe\""
redirect ::= "null" ws | "{" ws "\"mode\":" ws mode "," ws "\"path\":" ws string "}" ws
mode ::= "\"truncate\"" | "\"append\""
strings ::= "[" ws (string ("," ws string){0,63})? "]" ws
string ::= "\"" char* "\"" ws
char ::= [^"\\\x00-\x1F] | "\\" (["\\/bfnrt] | "u" [0-9a-fA-F]{4})
ws ::= [ \t\n\r]{0,4}
"#,
    );
    grammar.push_str("command ::= ");
    grammar.push_str(&terminals.join(" | "));
    grammar.push('\n');
    Ok(grammar)
}
