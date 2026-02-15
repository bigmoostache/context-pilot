/// Shared command classification types and utilities.
/// Used by both git and github modules.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandClass {
    ReadOnly,
    Mutating,
}

/// Parse a command string into arguments, respecting single and double quotes.
/// Strips the leading command name (e.g. "git" or "gh") and returns the rest.
pub fn parse_shell_args(command: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let chars = command.chars().peekable();

    for c in chars {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => {
                current.push(c);
            }
        }
    }

    if in_single {
        return Err("Unterminated single quote".to_string());
    }
    if in_double {
        return Err("Unterminated double quote".to_string());
    }
    if !current.is_empty() {
        args.push(current);
    }

    Ok(args)
}

/// Check for shell metacharacters outside of quoted strings.
pub fn check_shell_operators(command: &str) -> Result<(), String> {
    let mut in_single = false;
    let mut in_double = false;
    let chars: Vec<char> = command.chars().collect();
    let len = chars.len();

    for i in 0..len {
        let c = chars[i];
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            _ if in_single || in_double => {} // inside quotes, skip
            '|' | ';' | '`' | '>' | '<' => {
                return Err(format!("Shell operator '{}' is not allowed", c));
            }
            '$' if i + 1 < len && chars[i + 1] == '(' => {
                return Err("Shell operator '$(' is not allowed".to_string());
            }
            '&' if i + 1 < len && chars[i + 1] == '&' => {
                return Err("Shell operator '&&' is not allowed".to_string());
            }
            '\n' | '\r' => {
                return Err("Newlines are not allowed outside of quoted strings".to_string());
            }
            _ => {}
        }
    }
    Ok(())
}
