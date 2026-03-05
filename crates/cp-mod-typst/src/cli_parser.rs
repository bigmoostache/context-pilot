//! Parse typst command strings for the typst_execute tool.
//!
//! Converts "typst compile file.typ -o out.pdf" into structured commands.

/// A parsed typst CLI command.
#[derive(Debug)]
pub enum TypstCommand {
    /// `typst compile <input> [-o <output>] [--root <root>]`
    Compile {
        /// Source `.typ` file path.
        input: String,
        /// Output file path (defaults to input with `.pdf`).
        output: Option<String>,
        /// Project root directory override.
        root: Option<String>,
    },
    /// `typst init <template> [<directory>]`
    Init {
        /// Template identifier (e.g., `@preview/graceful-genetics:0.2.0`).
        template: String,
        /// Target directory (defaults to template name).
        directory: Option<String>,
    },
    /// `typst query <input> <selector> [--field <field>]`
    Query {
        /// Source `.typ` file to query.
        input: String,
        /// Content selector (e.g., `<heading>`).
        selector: String,
        /// Optional field to extract from results.
        field: Option<String>,
    },
    /// `typst fonts [--variants]`
    Fonts {
        /// Whether to show font variants.
        variants: bool,
    },
    /// `typst update [<package>]`
    Update {
        /// Optional specific package to re-download.
        package: Option<String>,
    },
    /// `typst watch <input> [-o <output>]` — add document to auto-compile watchlist
    Watch {
        /// Source `.typ` file to watch.
        input: String,
        /// Output file path override.
        output: Option<String>,
    },
    /// `typst unwatch <input>` — remove document from watchlist
    Unwatch {
        /// Source `.typ` file to stop watching.
        input: String,
    },
    /// `typst watchlist` — list all watched documents
    Watchlist,
}

/// Parse a typst command string into a structured `TypstCommand`.
///
/// Accepts commands with or without the "typst" prefix:
/// - "typst compile doc.typ"
/// - "compile doc.typ"
///
/// # Errors
///
/// Returns `Err` if the command is empty, has an unknown subcommand, or
/// the subcommand arguments are invalid.
pub fn parse_command(command: &str) -> Result<TypstCommand, String> {
    let tokens = shell_split(command);
    if tokens.is_empty() {
        return Err("Empty command".to_string());
    }

    // Skip leading "typst" if present
    let Some(first) = tokens.first() else {
        return Err("Empty command".to_string());
    };
    let start = usize::from(first == "typst");
    if start >= tokens.len() {
        return Err("Missing subcommand. Available: compile, init, query, fonts, update".to_string());
    }

    let Some(subcommand) = tokens.get(start) else {
        return Err("Missing subcommand. Available: compile, init, query, fonts, update".to_string());
    };
    let rest_start = start.saturating_add(1);
    let args = tokens.get(rest_start..).unwrap_or_default();

    match subcommand.as_str() {
        "compile" | "c" => parse_compile(args),
        "init" => parse_init(args),
        "query" => parse_query(args),
        "fonts" => Ok(parse_fonts(args)),
        "update" => Ok(parse_update(args)),
        "watch" | "w" => parse_watch(args),
        "unwatch" => parse_unwatch(args),
        "watchlist" => Ok(TypstCommand::Watchlist),
        other => Err(format!(
            "Unknown subcommand '{other}'. Available: compile, init, query, fonts, update, watch, unwatch, watchlist"
        )),
    }
}

/// Parse `compile` subcommand arguments into a `TypstCommand::Compile`.
fn parse_compile(args: &[String]) -> Result<TypstCommand, String> {
    if args.is_empty() {
        return Err("Usage: typst compile <input.typ> [-o <output.pdf>] [--root <dir>]".to_string());
    }

    let mut input = None;
    let mut output = None;
    let mut root = None;
    let mut i = 0;

    while i < args.len() {
        let Some(arg) = args.get(i) else {
            break;
        };
        match arg.as_str() {
            "-o" | "--output" => {
                i = i.saturating_add(1);
                let Some(val) = args.get(i) else {
                    return Err("Missing value for -o/--output".to_string());
                };
                output = Some(val.clone());
            }
            "--root" => {
                i = i.saturating_add(1);
                let Some(val) = args.get(i) else {
                    return Err("Missing value for --root".to_string());
                };
                root = Some(val.clone());
            }
            a if a.starts_with('-') => {
                // Skip unknown flags silently
            }
            _ => {
                let Some(val) = args.get(i) else {
                    break;
                };
                if input.is_none() {
                    input = Some(val.clone());
                } else if output.is_none() {
                    // Second positional arg is output
                    output = Some(val.clone());
                }
            }
        }
        i = i.saturating_add(1);
    }

    let input = input.ok_or("Missing input file. Usage: typst compile <input.typ>")?;
    Ok(TypstCommand::Compile { input, output, root })
}

/// Parse `init` subcommand arguments into a `TypstCommand::Init`.
fn parse_init(args: &[String]) -> Result<TypstCommand, String> {
    if args.is_empty() {
        return Err(
            "Usage: typst init <@preview/template:version> [directory]\nExample: typst init @preview/graceful-genetics:0.2.0"
                .to_string(),
        );
    }

    let Some(template) = args.first() else {
        return Err("Usage: typst init <@preview/template:version> [directory]".to_string());
    };
    let template = template.clone();
    let directory = args.get(1).cloned();
    Ok(TypstCommand::Init { template, directory })
}

/// Parse `query` subcommand arguments into a `TypstCommand::Query`.
fn parse_query(args: &[String]) -> Result<TypstCommand, String> {
    if args.len() < 2 {
        return Err("Usage: typst query <input.typ> <selector> [--field <field>]".to_string());
    }

    // Length already checked above (args.len() >= 2)
    let (Some(input), Some(selector)) = (args.first().cloned(), args.get(1).cloned()) else {
        return Err("Usage: typst query <input.typ> <selector> [--field <field>]".to_string());
    };
    let mut field = None;
    let mut i = 2;

    while i < args.len() {
        let Some(arg) = args.get(i) else {
            break;
        };
        if arg == "--field" {
            i = i.saturating_add(1);
            if let Some(val) = args.get(i) {
                field = Some(val.clone());
            }
        }
        i = i.saturating_add(1);
    }

    Ok(TypstCommand::Query { input, selector, field })
}

/// Parse `fonts` subcommand arguments into a `TypstCommand::Fonts`.
fn parse_fonts(args: &[String]) -> TypstCommand {
    let variants = args.iter().any(|a| a == "--variants");
    TypstCommand::Fonts { variants }
}

/// Parse `update` subcommand arguments into a `TypstCommand::Update`.
fn parse_update(args: &[String]) -> TypstCommand {
    let package = args.first().cloned();
    TypstCommand::Update { package }
}

/// Parse `watch` subcommand arguments into a `TypstCommand::Watch`.
fn parse_watch(args: &[String]) -> Result<TypstCommand, String> {
    if args.is_empty() {
        return Err("Usage: typst watch <input.typ> [-o <output.pdf>]".to_string());
    }

    let mut input = None;
    let mut output = None;
    let mut i = 0;

    while i < args.len() {
        let Some(arg) = args.get(i) else {
            break;
        };
        match arg.as_str() {
            "-o" | "--output" => {
                i = i.saturating_add(1);
                let Some(val) = args.get(i) else {
                    return Err("Missing value for -o/--output".to_string());
                };
                output = Some(val.clone());
            }
            a if a.starts_with('-') => {}
            _ => {
                let Some(val) = args.get(i) else {
                    break;
                };
                if input.is_none() {
                    input = Some(val.clone());
                } else if output.is_none() {
                    output = Some(val.clone());
                }
            }
        }
        i = i.saturating_add(1);
    }

    let input = input.ok_or("Missing input file. Usage: typst watch <input.typ>")?;
    Ok(TypstCommand::Watch { input, output })
}

/// Parse `unwatch` subcommand arguments into a `TypstCommand::Unwatch`.
fn parse_unwatch(args: &[String]) -> Result<TypstCommand, String> {
    if args.is_empty() {
        return Err("Usage: typst unwatch <input.typ>".to_string());
    }
    let Some(input) = args.first() else {
        return Err("Usage: typst unwatch <input.typ>".to_string());
    };
    Ok(TypstCommand::Unwatch { input: input.clone() })
}

/// Basic shell-like string splitting that respects quotes.
///
/// `"typst compile 'my file.typ'"` → `["typst", "compile", "my file.typ"]`
fn shell_split(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for ch in input.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' if !in_single => escaped = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ' ' | '\t' if !in_single && !in_double => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}
