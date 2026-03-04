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
    let start = usize::from(tokens[0] == "typst");
    if start >= tokens.len() {
        return Err("Missing subcommand. Available: compile, init, query, fonts, update".to_string());
    }

    let subcommand = &tokens[start];
    let args = &tokens[start + 1..];

    match subcommand.as_str() {
        "compile" | "c" => parse_compile(args),
        "init" => parse_init(args),
        "query" => parse_query(args),
        "fonts" => parse_fonts(args),
        "update" => parse_update(args),
        "watch" | "w" => parse_watch(args),
        "unwatch" => parse_unwatch(args),
        "watchlist" => Ok(TypstCommand::Watchlist),
        other => Err(format!(
            "Unknown subcommand '{}'. Available: compile, init, query, fonts, update, watch, unwatch, watchlist",
            other
        )),
    }
}

fn parse_compile(args: &[String]) -> Result<TypstCommand, String> {
    if args.is_empty() {
        return Err("Usage: typst compile <input.typ> [-o <output.pdf>] [--root <dir>]".to_string());
    }

    let mut input = None;
    let mut output = None;
    let mut root = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing value for -o/--output".to_string());
                }
                output = Some(args[i].clone());
            }
            "--root" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing value for --root".to_string());
                }
                root = Some(args[i].clone());
            }
            arg if arg.starts_with('-') => {
                // Skip unknown flags silently
            }
            _ => {
                if input.is_none() {
                    input = Some(args[i].clone());
                } else if output.is_none() {
                    // Second positional arg is output
                    output = Some(args[i].clone());
                }
            }
        }
        i += 1;
    }

    let input = input.ok_or("Missing input file. Usage: typst compile <input.typ>")?;
    Ok(TypstCommand::Compile { input, output, root })
}

fn parse_init(args: &[String]) -> Result<TypstCommand, String> {
    if args.is_empty() {
        return Err(
            "Usage: typst init <@preview/template:version> [directory]\nExample: typst init @preview/graceful-genetics:0.2.0"
                .to_string(),
        );
    }

    let template = args[0].clone();
    let directory = args.get(1).cloned();
    Ok(TypstCommand::Init { template, directory })
}

fn parse_query(args: &[String]) -> Result<TypstCommand, String> {
    if args.len() < 2 {
        return Err("Usage: typst query <input.typ> <selector> [--field <field>]".to_string());
    }

    let input = args[0].clone();
    let selector = args[1].clone();
    let mut field = None;
    let mut i = 2;

    while i < args.len() {
        if args[i] == "--field" {
            i += 1;
            if i < args.len() {
                field = Some(args[i].clone());
            }
        }
        i += 1;
    }

    Ok(TypstCommand::Query { input, selector, field })
}

fn parse_fonts(args: &[String]) -> Result<TypstCommand, String> {
    let variants = args.iter().any(|a| a == "--variants");
    Ok(TypstCommand::Fonts { variants })
}

fn parse_update(args: &[String]) -> Result<TypstCommand, String> {
    let package = args.first().cloned();
    Ok(TypstCommand::Update { package })
}

fn parse_watch(args: &[String]) -> Result<TypstCommand, String> {
    if args.is_empty() {
        return Err("Usage: typst watch <input.typ> [-o <output.pdf>]".to_string());
    }

    let mut input = None;
    let mut output = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing value for -o/--output".to_string());
                }
                output = Some(args[i].clone());
            }
            arg if arg.starts_with('-') => {}
            _ => {
                if input.is_none() {
                    input = Some(args[i].clone());
                } else if output.is_none() {
                    output = Some(args[i].clone());
                }
            }
        }
        i += 1;
    }

    let input = input.ok_or("Missing input file. Usage: typst watch <input.typ>")?;
    Ok(TypstCommand::Watch { input, output })
}

fn parse_unwatch(args: &[String]) -> Result<TypstCommand, String> {
    if args.is_empty() {
        return Err("Usage: typst unwatch <input.typ>".to_string());
    }
    Ok(TypstCommand::Unwatch { input: args[0].clone() })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_compile_basic() {
        let cmd = parse_command("typst compile doc.typ").unwrap();
        match cmd {
            TypstCommand::Compile { input, output, root } => {
                assert_eq!(input, "doc.typ");
                assert!(output.is_none());
                assert!(root.is_none());
            }
            _ => panic!("Expected Compile"),
        }
    }

    #[test]
    fn test_parse_compile_with_output() {
        let cmd = parse_command("typst compile doc.typ -o out.pdf").unwrap();
        match cmd {
            TypstCommand::Compile { input, output, .. } => {
                assert_eq!(input, "doc.typ");
                assert_eq!(output, Some("out.pdf".to_string()));
            }
            _ => panic!("Expected Compile"),
        }
    }

    #[test]
    fn test_parse_compile_without_prefix() {
        let cmd = parse_command("compile doc.typ").unwrap();
        match cmd {
            TypstCommand::Compile { input, .. } => assert_eq!(input, "doc.typ"),
            _ => panic!("Expected Compile"),
        }
    }

    #[test]
    fn test_parse_init() {
        let cmd = parse_command("typst init @preview/graceful-genetics:0.2.0").unwrap();
        match cmd {
            TypstCommand::Init { template, directory } => {
                assert_eq!(template, "@preview/graceful-genetics:0.2.0");
                assert!(directory.is_none());
            }
            _ => panic!("Expected Init"),
        }
    }

    #[test]
    fn test_parse_init_with_dir() {
        let cmd = parse_command("typst init @preview/graceful-genetics:0.2.0 my-poster").unwrap();
        match cmd {
            TypstCommand::Init { template, directory } => {
                assert_eq!(template, "@preview/graceful-genetics:0.2.0");
                assert_eq!(directory, Some("my-poster".to_string()));
            }
            _ => panic!("Expected Init"),
        }
    }

    #[test]
    fn test_parse_fonts() {
        let cmd = parse_command("typst fonts").unwrap();
        match cmd {
            TypstCommand::Fonts { variants } => assert!(!variants),
            _ => panic!("Expected Fonts"),
        }
    }

    #[test]
    fn test_parse_fonts_variants() {
        let cmd = parse_command("typst fonts --variants").unwrap();
        match cmd {
            TypstCommand::Fonts { variants } => assert!(variants),
            _ => panic!("Expected Fonts"),
        }
    }

    #[test]
    fn test_parse_watch() {
        let cmd = parse_command("typst watch doc.typ").unwrap();
        match cmd {
            TypstCommand::Watch { input, output } => {
                assert_eq!(input, "doc.typ");
                assert!(output.is_none());
            }
            _ => panic!("Expected Watch"),
        }
    }

    #[test]
    fn test_shell_split_quotes() {
        let tokens = shell_split("compile 'my file.typ' -o \"out put.pdf\"");
        assert_eq!(tokens, vec!["compile", "my file.typ", "-o", "out put.pdf"]);
    }

    #[test]
    fn test_parse_query() {
        let cmd = parse_command("typst query doc.typ '<heading>'").unwrap();
        match cmd {
            TypstCommand::Query { input, selector, field } => {
                assert_eq!(input, "doc.typ");
                assert_eq!(selector, "<heading>");
                assert!(field.is_none());
            }
            _ => panic!("Expected Query"),
        }
    }
}
