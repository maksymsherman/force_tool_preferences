use std::borrow::Cow;
use std::env;
use std::fs;
use std::io::{self, Read};
use std::process;
use std::time::Instant;

use serde_json::{json, Map, Value};
use smallvec::SmallVec;

const BINARY_NAME: &str = "enforce-tool-preferences-command";
const GREP_MESSAGE: &str = "Use rg (ripgrep) instead of grep in this project. Replace blocked grep commands with the least invasive exact rg rewrite when the flag mapping is clear. If a flag does not have a guaranteed direct rg translation, translate it manually instead of guessing.";
const PYTHON_MESSAGE: &str = "Use uv instead of bare Python or pip commands in this project. Replace the blocked command with 'uv run ...', 'uv add ...', 'uv add --dev ...', 'uv remove ...', or 'uv run --with ...' as appropriate.";
const UV_INIT_MESSAGE: &str = "Do not run 'uv init' in an existing project unless the user explicitly asks for project creation or conversion. Inspect the repo first and prefer 'uv run', 'uv add', 'uv sync', or 'uv run --with'. If project initialization is truly needed, use 'uv init --no-readme --no-workspace' to avoid overwriting existing files and git history.";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuleId {
    Ripgrep = 0,
    Uv = 1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuleSpec {
    name: &'static str,
    preferred_tool: &'static str,
    guidance: &'static str,
}

const RULE_SPECS: [RuleSpec; 2] = [
    RuleSpec {
        name: "grep-family",
        preferred_tool: "rg",
        guidance: GREP_MESSAGE,
    },
    RuleSpec {
        name: "python-family",
        preferred_tool: "uv",
        guidance: PYTHON_MESSAGE,
    },
];

fn rule_spec(rule: RuleId) -> &'static RuleSpec {
    &RULE_SPECS[rule as usize]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuleSet(u8);

impl RuleSet {
    const RIPGREP: u8 = 1 << 0;
    const UV: u8 = 1 << 1;
    const ALL: u8 = Self::RIPGREP | Self::UV;

    fn all() -> Self {
        Self(Self::ALL)
    }

    #[cfg(test)]
    fn only(rule: RuleId) -> Self {
        Self(rule_mask(rule))
    }

    fn contains(self, rule: RuleId) -> bool {
        self.0 & rule_mask(rule) != 0
    }

    fn parse(value: &str) -> Result<Self, String> {
        let mut mask = 0u8;

        for item in value.split(',') {
            let name = item.trim();
            if name.is_empty() {
                continue;
            }

            match name {
                "rg" | "ripgrep" => mask |= Self::RIPGREP,
                "uv" => mask |= Self::UV,
                _ => {
                    return Err(format!(
                    "unknown rule set '{name}'. Expected a comma-separated list using rg and/or uv"
                ))
                }
            }
        }

        if mask == 0 {
            return Err("at least one rule must be enabled; use rg, uv, or rg,uv".to_string());
        }

        Ok(Self(mask))
    }

    fn cli_value(self) -> &'static str {
        match self.0 {
            Self::RIPGREP => "rg",
            Self::UV => "uv",
            Self::ALL => "rg,uv",
            _ => "rg,uv",
        }
    }
}

fn rule_mask(rule: RuleId) -> u8 {
    match rule {
        RuleId::Ripgrep => RuleSet::RIPGREP,
        RuleId::Uv => RuleSet::UV,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BlockDecision {
    message: String,
}

impl BlockDecision {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

fn main() {
    let exit_code = match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            1
        }
    };

    process::exit(exit_code);
}

fn run() -> Result<i32, String> {
    let config = Config::parse(env::args().skip(1))?;

    match config.mode {
        Mode::Evaluate {
            input,
            json_block_output,
            rules,
        } => {
            let raw = match input {
                InputMode::Command(text) => text,
                InputMode::StdinCommand => read_stdin()?,
                InputMode::HookJson => extract_tool_input_command(&read_stdin()?)?,
            };

            match evaluate_command(raw.trim(), rules) {
                Some(decision) if json_block_output => {
                    println!(
                        "{{\"decision\":\"block\",\"reason\":\"{}\"}}",
                        escape_json(&decision.message)
                    );
                    Ok(0)
                }
                Some(decision) => {
                    eprintln!("{}", decision.message);
                    Ok(2)
                }
                None => Ok(0),
            }
        }
        Mode::Benchmark {
            command,
            iterations,
            rules,
        } => {
            if iterations == 0 {
                return Err("iterations must be greater than 0".to_string());
            }

            let start = Instant::now();
            let mut blocks = 0u64;

            for _ in 0..iterations {
                if evaluate_command(&command, rules).is_some() {
                    blocks += 1;
                }
            }

            let elapsed = start.elapsed();
            let total_ns = elapsed.as_nanos();
            let avg_ns = total_ns as f64 / iterations as f64;
            let avg_us = avg_ns / 1_000.0;

            println!("iterations={iterations}");
            println!("blocked={blocks}");
            println!("total_ns={total_ns}");
            println!("avg_ns={avg_ns:.2}");
            println!("avg_us={avg_us:.4}");
            Ok(0)
        }
        Mode::ConfigureClaudeHook {
            settings_path,
            binary_name,
            rules,
        } => {
            configure_claude_hook(&settings_path, &binary_name, rules)?;
            Ok(0)
        }
        Mode::ConfigureGeminiHook {
            settings_path,
            binary_name,
            rules,
        } => {
            configure_gemini_hook(&settings_path, &binary_name, rules)?;
            Ok(0)
        }
        Mode::ConfigureCodexHook {
            settings_path,
            binary_name,
            rules,
        } => {
            configure_codex_hook(&settings_path, &binary_name, rules)?;
            Ok(0)
        }
    }
}

#[derive(Debug)]
struct Config {
    mode: Mode,
}

#[derive(Debug)]
enum Mode {
    Evaluate {
        input: InputMode,
        json_block_output: bool,
        rules: RuleSet,
    },
    Benchmark {
        command: String,
        iterations: u64,
        rules: RuleSet,
    },
    ConfigureClaudeHook {
        settings_path: String,
        binary_name: String,
        rules: RuleSet,
    },
    ConfigureGeminiHook {
        settings_path: String,
        binary_name: String,
        rules: RuleSet,
    },
    ConfigureCodexHook {
        settings_path: String,
        binary_name: String,
        rules: RuleSet,
    },
}

#[derive(Debug)]
enum InputMode {
    Command(String),
    StdinCommand,
    HookJson,
}

impl Config {
    fn parse<I>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = String>,
    {
        let mut input: Option<InputMode> = None;
        let mut json_block_output = false;
        let mut benchmark_command: Option<String> = None;
        let mut configure_claude_hook: Option<(String, String)> = None;
        let mut configure_gemini_hook: Option<(String, String)> = None;
        let mut configure_codex_hook: Option<(String, String)> = None;
        let mut iterations = 100_000u64;
        let mut rules = RuleSet::all();

        let mut iter = args.into_iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--rules" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| "missing value for --rules".to_string())?;
                    rules = RuleSet::parse(&value)?;
                }
                "--command" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| "missing value for --command".to_string())?;
                    input = Some(InputMode::Command(value));
                }
                "--stdin-command" => {
                    input = Some(InputMode::StdinCommand);
                }
                "--claude-hook-json" => {
                    input = Some(InputMode::HookJson);
                    json_block_output = true;
                }
                "--codex-hook-json" => {
                    input = Some(InputMode::HookJson);
                    json_block_output = true;
                }
                "--gemini-hook-json" => {
                    input = Some(InputMode::HookJson);
                }
                "--claude-json" => {
                    json_block_output = true;
                }
                "--benchmark-command" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| "missing value for --benchmark-command".to_string())?;
                    benchmark_command = Some(value);
                }
                "--configure-claude-hook" => {
                    let settings_path = iter.next().ok_or_else(|| {
                        "missing settings path for --configure-claude-hook".to_string()
                    })?;
                    let binary_name = iter.next().ok_or_else(|| {
                        "missing binary name for --configure-claude-hook".to_string()
                    })?;
                    configure_claude_hook = Some((settings_path, binary_name));
                }
                "--configure-gemini-hook" => {
                    let settings_path = iter.next().ok_or_else(|| {
                        "missing settings path for --configure-gemini-hook".to_string()
                    })?;
                    let binary_name = iter.next().ok_or_else(|| {
                        "missing binary name for --configure-gemini-hook".to_string()
                    })?;
                    configure_gemini_hook = Some((settings_path, binary_name));
                }
                "--configure-codex-hook" => {
                    let settings_path = iter.next().ok_or_else(|| {
                        "missing settings path for --configure-codex-hook".to_string()
                    })?;
                    let binary_name = iter.next().ok_or_else(|| {
                        "missing binary name for --configure-codex-hook".to_string()
                    })?;
                    configure_codex_hook = Some((settings_path, binary_name));
                }
                "--iterations" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| "missing value for --iterations".to_string())?;
                    iterations = value
                        .parse::<u64>()
                        .map_err(|_| "iterations must be an integer".to_string())?;
                }
                "--help" | "-h" => {
                    print_usage();
                    return Ok(Self {
                        mode: Mode::Evaluate {
                            input: InputMode::Command(String::new()),
                            json_block_output: false,
                            rules,
                        },
                    });
                }
                _ => {
                    return Err(format!("unknown argument: {arg}"));
                }
            }
        }

        if let Some(command) = benchmark_command {
            return Ok(Self {
                mode: Mode::Benchmark {
                    command,
                    iterations,
                    rules,
                },
            });
        }

        if let Some((settings_path, binary_name)) = configure_claude_hook {
            return Ok(Self {
                mode: Mode::ConfigureClaudeHook {
                    settings_path,
                    binary_name,
                    rules,
                },
            });
        }

        if let Some((settings_path, binary_name)) = configure_gemini_hook {
            return Ok(Self {
                mode: Mode::ConfigureGeminiHook {
                    settings_path,
                    binary_name,
                    rules,
                },
            });
        }

        if let Some((settings_path, binary_name)) = configure_codex_hook {
            return Ok(Self {
                mode: Mode::ConfigureCodexHook {
                    settings_path,
                    binary_name,
                    rules,
                },
            });
        }

        let input = input.ok_or_else(|| {
            "expected one of --command, --stdin-command, --claude-hook-json, --codex-hook-json, or --gemini-hook-json".to_string()
        })?;

        Ok(Self {
            mode: Mode::Evaluate {
                input,
                json_block_output,
                rules,
            },
        })
    }
}

fn print_usage() {
    println!(
        "Usage:\n  {0} --command \"grep -rn pattern .\" [--claude-json] [--rules rg,uv]\n  {0} --stdin-command [--claude-json] [--rules rg,uv]\n  {0} --claude-hook-json [--rules rg,uv]\n  {0} --codex-hook-json [--rules rg,uv]\n  {0} --gemini-hook-json [--rules rg,uv]\n  {0} --benchmark-command \"grep -rn pattern .\" [--iterations 1000000] [--rules rg,uv]\n  {0} --configure-claude-hook <settings-path> <binary-name> [--rules rg,uv]\n  {0} --configure-gemini-hook <settings-path> <binary-name> [--rules rg,uv]\n  {0} --configure-codex-hook <hooks-path> <binary-name> [--rules rg,uv]",
        BINARY_NAME
    );
}

fn read_stdin() -> Result<String, String> {
    let mut buffer = String::new();
    io::stdin()
        .read_to_string(&mut buffer)
        .map_err(|error| format!("failed to read stdin: {error}"))?;
    Ok(buffer)
}

fn escape_json(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 8);
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn evaluate_command(command: &str, rules: RuleSet) -> Option<BlockDecision> {
    if is_simple_command(command) {
        return evaluate_simple_command(command, rules);
    }

    let bytes = command.as_bytes();
    let mut tokens = TokenBuffer::new();
    let mut token_start = None;
    let mut value: Option<Vec<u8>> = None;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut index = 0usize;

    while index < bytes.len() {
        let byte = bytes[index];

        if in_single_quote {
            if byte == b'\'' {
                in_single_quote = false;
            } else {
                value
                    .as_mut()
                    .expect("quoted tokens must use an owned value buffer")
                    .push(byte);
            }
            index += 1;
            continue;
        }

        if in_double_quote {
            match byte {
                b'"' => in_double_quote = false,
                b'\\' => {
                    if index + 1 < bytes.len() {
                        index += 1;
                        value
                            .as_mut()
                            .expect("quoted tokens must use an owned value buffer")
                            .push(bytes[index]);
                    } else {
                        value
                            .as_mut()
                            .expect("quoted tokens must use an owned value buffer")
                            .push(b'\\');
                    }
                }
                _ => value
                    .as_mut()
                    .expect("quoted tokens must use an owned value buffer")
                    .push(byte),
            }
            index += 1;
            continue;
        }

        match byte {
            b' ' | b'\n' | b'\r' | b'\t' => {
                flush_parsed_token(command, index, &mut token_start, &mut value, &mut tokens)
            }
            b'\'' => {
                let start = token_start.get_or_insert(index);
                ensure_owned_value(command, *start, index, &mut value);
                in_single_quote = true;
            }
            b'"' => {
                let start = token_start.get_or_insert(index);
                ensure_owned_value(command, *start, index, &mut value);
                in_double_quote = true;
            }
            b';' => {
                flush_parsed_token(command, index, &mut token_start, &mut value, &mut tokens);
                if let Some(decision) = evaluate_parsed_segment(&mut tokens, rules) {
                    return Some(decision);
                }
            }
            b'|' | b'&' => {
                flush_parsed_token(command, index, &mut token_start, &mut value, &mut tokens);
                if let Some(decision) = evaluate_parsed_segment(&mut tokens, rules) {
                    return Some(decision);
                }
                if index + 1 < bytes.len() && bytes[index + 1] == byte {
                    index += 1;
                }
            }
            b'\\' => {
                let start = token_start.get_or_insert(index);
                let value = ensure_owned_value(command, *start, index, &mut value);
                if index + 1 < bytes.len() {
                    index += 1;
                    value.push(bytes[index]);
                } else {
                    value.push(b'\\');
                }
            }
            _ => {
                token_start.get_or_insert(index);
                if let Some(value) = value.as_mut() {
                    value.push(byte);
                }
            }
        }

        index += 1;
    }

    flush_parsed_token(
        command,
        bytes.len(),
        &mut token_start,
        &mut value,
        &mut tokens,
    );
    evaluate_parsed_segment(&mut tokens, rules)
}

fn is_simple_command(command: &str) -> bool {
    !command.as_bytes().iter().any(|byte| {
        matches!(
            byte,
            b'\'' | b'"' | b'\\' | b';' | b'|' | b'&' | b'\n' | b'\r' | b'\t'
        )
    })
}

fn evaluate_simple_command(command: &str, rules: RuleSet) -> Option<BlockDecision> {
    let mut tokens = TokenBuffer::new();

    for raw in command.split_ascii_whitespace() {
        tokens.push(ParsedToken {
            raw,
            value: Cow::Borrowed(raw),
        });
    }

    evaluate_segment(&tokens, rules)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ParsedToken<'a> {
    raw: &'a str,
    value: Cow<'a, str>,
}

type TokenBuffer<'a> = SmallVec<[ParsedToken<'a>; 8]>;

fn ensure_owned_value<'a, 'b>(
    command: &'a str,
    token_start: usize,
    prefix_end: usize,
    value: &'b mut Option<Vec<u8>>,
) -> &'b mut Vec<u8> {
    value.get_or_insert_with(|| command[token_start..prefix_end].as_bytes().to_vec())
}

fn flush_parsed_token<'a>(
    command: &'a str,
    token_end: usize,
    token_start: &mut Option<usize>,
    value: &mut Option<Vec<u8>>,
    tokens: &mut TokenBuffer<'a>,
) {
    let Some(start) = token_start.take() else {
        return;
    };

    let raw = &command[start..token_end];
    let value = match value.take() {
        Some(value) => {
            Cow::Owned(String::from_utf8(value).expect("parser only collects valid UTF-8 bytes"))
        }
        None => Cow::Borrowed(raw),
    };

    tokens.push(ParsedToken { raw, value });
}

fn evaluate_parsed_segment(tokens: &mut TokenBuffer<'_>, rules: RuleSet) -> Option<BlockDecision> {
    if tokens.is_empty() {
        return None;
    }

    let decision = evaluate_segment(tokens, rules);
    tokens.clear();
    decision
}

fn evaluate_segment(tokens: &[ParsedToken<'_>], rules: RuleSet) -> Option<BlockDecision> {
    let mut wrapper = None;
    let mut skip_next_value = false;

    for (index, token) in tokens.iter().enumerate() {
        let value = token.value.as_bytes();

        if skip_next_value {
            skip_next_value = false;
            continue;
        }

        if value.starts_with(b"-") {
            if let Some(wrapper_kind) = wrapper {
                skip_next_value = wrapper_option_takes_value(wrapper_kind, value);
            }
            continue;
        }

        if is_shell_assignment(value) {
            continue;
        }

        match classify_token(value) {
            TokenKind::Wrapper(kind) => {
                wrapper = Some(kind);
            }
            TokenKind::Allowed(AllowedCommand::Rg) | TokenKind::Allowed(AllowedCommand::Uvx) => {
                return None;
            }
            TokenKind::Allowed(AllowedCommand::Uv) => {
                if !rules.contains(RuleId::Uv) {
                    return None;
                }
                return evaluate_uv_command(tokens, index);
            }
            TokenKind::Blocked(BlockedCommand::Grep(kind)) => {
                if !rules.contains(RuleId::Ripgrep) {
                    return None;
                }
                return Some(build_grep_decision(tokens, index, kind));
            }
            TokenKind::Blocked(BlockedCommand::Python) => {
                if !rules.contains(RuleId::Uv) {
                    return None;
                }
                return Some(build_python_decision(tokens, index));
            }
            TokenKind::Blocked(BlockedCommand::Pip) => {
                if !rules.contains(RuleId::Uv) {
                    return None;
                }
                return Some(build_pip_decision(tokens, index));
            }
            TokenKind::Other => return None,
        }
    }

    None
}

fn build_python_decision(tokens: &[ParsedToken<'_>], command_index: usize) -> BlockDecision {
    let suggestion = insert_before_command(tokens, command_index, &["uv", "run"]);
    BlockDecision::new(format_exact_suggestion(
        rule_spec(RuleId::Uv).guidance,
        &suggestion,
    ))
}

fn build_pip_decision(tokens: &[ParsedToken<'_>], command_index: usize) -> BlockDecision {
    BlockDecision::new(render_pip_decision(tokens, command_index))
}

fn evaluate_uv_command(tokens: &[ParsedToken<'_>], command_index: usize) -> Option<BlockDecision> {
    let mut skip_next_value = false;

    for token in &tokens[command_index + 1..] {
        let value = token.value.as_bytes();

        if skip_next_value {
            skip_next_value = false;
            continue;
        }

        if value == b"--" {
            continue;
        }

        if value.starts_with(b"-") {
            skip_next_value = uv_option_takes_value(value);
            continue;
        }

        if is_shell_assignment(value) {
            continue;
        }

        if normalized_program_name(value) == b"init" {
            return Some(BlockDecision::new(UV_INIT_MESSAGE));
        }

        return None;
    }

    None
}

fn render_pip_decision(tokens: &[ParsedToken<'_>], command_index: usize) -> String {
    let pip_rewrite = replace_command(tokens, command_index, 1, &["uv", "pip"]);
    let Some(subcommand) = tokens
        .get(command_index + 1)
        .map(|token| token.value.as_ref())
    else {
        return format_exact_suggestion(rule_spec(RuleId::Uv).guidance, &pip_rewrite);
    };

    if subcommand.eq_ignore_ascii_case("install") {
        return render_pip_install_decision(tokens, command_index, pip_rewrite);
    }

    if subcommand.eq_ignore_ascii_case("uninstall") {
        return render_pip_uninstall_decision(tokens, command_index, pip_rewrite);
    }

    format_exact_suggestion(rule_spec(RuleId::Uv).guidance, &pip_rewrite)
}

fn render_pip_install_decision(
    tokens: &[ParsedToken<'_>],
    command_index: usize,
    pip_rewrite: String,
) -> String {
    let dependency_args = &tokens[command_index + 2..];
    if is_high_confidence_dependency_list(dependency_args) {
        let project_rewrite = replace_command(tokens, command_index, 2, &["uv", "add"]);
        return format_alternative_suggestions(
            rule_spec(RuleId::Uv).guidance,
            &[project_rewrite, pip_rewrite],
            Some("Choose `uv add` for project dependencies; choose `uv pip` to keep pip-style behavior."),
        );
    }

    let mut message = format_exact_suggestion(rule_spec(RuleId::Uv).guidance, &pip_rewrite);
    message.push('\n');
    message.push_str(
        "Use `uv add ...` only when you intentionally want to modify project dependencies.",
    );
    message
}

fn render_pip_uninstall_decision(
    tokens: &[ParsedToken<'_>],
    command_index: usize,
    pip_rewrite: String,
) -> String {
    let dependency_args = &tokens[command_index + 2..];
    if is_high_confidence_dependency_list(dependency_args) {
        let project_rewrite = replace_command(tokens, command_index, 2, &["uv", "remove"]);
        return format_alternative_suggestions(
            rule_spec(RuleId::Uv).guidance,
            &[project_rewrite, pip_rewrite],
            Some("Choose `uv remove` when the package belongs in project metadata; choose `uv pip` for pip-style environment changes."),
        );
    }

    let mut message = format_exact_suggestion(rule_spec(RuleId::Uv).guidance, &pip_rewrite);
    message.push('\n');
    message.push_str(
        "Use `uv remove ...` only when you intentionally want to update project dependencies.",
    );
    message
}

fn insert_before_command(
    tokens: &[ParsedToken<'_>],
    command_index: usize,
    inserted: &[&str],
) -> String {
    rewrite_command(tokens, command_index, 0, inserted)
}

fn replace_command(
    tokens: &[ParsedToken<'_>],
    command_index: usize,
    consumed: usize,
    replacement: &[&str],
) -> String {
    rewrite_command(tokens, command_index, consumed, replacement)
}

fn rewrite_command(
    tokens: &[ParsedToken<'_>],
    command_index: usize,
    consumed: usize,
    replacement: &[&str],
) -> String {
    let suffix_start = command_index + consumed;
    let part_count = command_index + replacement.len() + tokens.len().saturating_sub(suffix_start);
    let total_len = tokens[..command_index]
        .iter()
        .map(|token| token.raw.len())
        .sum::<usize>()
        + replacement.iter().map(|item| item.len()).sum::<usize>()
        + tokens[suffix_start..]
            .iter()
            .map(|token| token.raw.len())
            .sum::<usize>()
        + part_count.saturating_sub(1);
    let mut output = String::with_capacity(total_len);
    let mut needs_space = false;

    for token in &tokens[..command_index] {
        push_command_part(&mut output, token.raw, &mut needs_space);
    }
    for item in replacement {
        push_command_part(&mut output, item, &mut needs_space);
    }
    for token in &tokens[suffix_start..] {
        push_command_part(&mut output, token.raw, &mut needs_space);
    }

    output
}

fn push_command_part(output: &mut String, part: &str, needs_space: &mut bool) {
    if *needs_space {
        output.push(' ');
    }
    output.push_str(part);
    *needs_space = true;
}

fn format_exact_suggestion(base: &str, suggestion: &str) -> String {
    format!("{base}\nSuggested replacement:\n  {suggestion}")
}

fn format_alternative_suggestions(
    base: &str,
    suggestions: &[String],
    note: Option<&str>,
) -> String {
    let mut message = String::from(base);

    if suggestions.len() == 1 {
        message.push_str("\nSuggested replacement:\n  ");
        message.push_str(&suggestions[0]);
    } else if !suggestions.is_empty() {
        message.push_str("\nLikely alternatives:");
        for suggestion in suggestions {
            message.push_str("\n  ");
            message.push_str(suggestion);
        }
    }

    if let Some(note) = note {
        message.push('\n');
        message.push_str(note);
    }

    message
}

fn is_high_confidence_dependency_list(tokens: &[ParsedToken<'_>]) -> bool {
    !tokens.is_empty()
        && tokens
            .iter()
            .all(|token| is_high_confidence_dependency_arg(token.value.as_ref()))
}

fn is_high_confidence_dependency_arg(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value.bytes().all(|byte| {
            matches!(
                byte,
                b'a'..=b'z'
                    | b'A'..=b'Z'
                    | b'0'..=b'9'
                    | b'.'
                    | b'_'
                    | b'-'
                    | b'['
                    | b']'
                    | b','
                    | b'='
                    | b'<'
                    | b'>'
                    | b'!'
                    | b'~'
            )
        })
}

fn build_grep_decision(
    tokens: &[ParsedToken<'_>],
    command_index: usize,
    grep_kind: GrepKind,
) -> BlockDecision {
    match rewrite_grep_to_rg(tokens, command_index, grep_kind) {
        GrepRewrite::Exact(suggestion) => BlockDecision::new(format_exact_suggestion(
            rule_spec(RuleId::Ripgrep).guidance,
            &suggestion,
        )),
        GrepRewrite::NeedsManualTranslation { flags } => BlockDecision::new(
            format_manual_translation_message(rule_spec(RuleId::Ripgrep).guidance, &flags),
        ),
    }
}

enum GrepRewrite {
    Exact(String),
    NeedsManualTranslation { flags: Vec<String> },
}

fn rewrite_grep_to_rg(
    tokens: &[ParsedToken<'_>],
    command_index: usize,
    grep_kind: GrepKind,
) -> GrepRewrite {
    let is_fgrep = matches!(grep_kind, GrepKind::Fgrep);
    let estimated_len = tokens
        .iter()
        .map(|token| token.raw.len() + 1)
        .sum::<usize>()
        + 4;
    let mut suggestion = String::with_capacity(estimated_len);
    let mut uncertain_flags = Vec::new();
    let mut has_parts = false;

    for token in &tokens[..command_index] {
        push_suggestion_part(&mut suggestion, token.raw, &mut has_parts);
    }

    push_suggestion_part(&mut suggestion, "rg", &mut has_parts);
    let fixed_strings_insert_at = suggestion.len();

    if is_fgrep {
        push_suggestion_part(&mut suggestion, "-F", &mut has_parts);
    }

    let mut need_fixed_strings = false;
    let mut end_of_options = false;

    for token in &tokens[command_index + 1..] {
        let val = token.value.as_ref();

        if end_of_options || !val.starts_with('-') || val == "-" {
            push_suggestion_part(&mut suggestion, token.raw, &mut has_parts);
            continue;
        }

        if val == "--" {
            push_suggestion_part(&mut suggestion, token.raw, &mut has_parts);
            end_of_options = true;
            continue;
        }

        if val.starts_with("--") {
            match classify_long_grep_flag(val) {
                LongFlagResult::Drop => continue,
                LongFlagResult::Keep(flag) => {
                    push_suggestion_part(&mut suggestion, &flag, &mut has_parts);
                    continue;
                }
                LongFlagResult::NeedFixedStrings => {
                    need_fixed_strings = true;
                    continue;
                }
                LongFlagResult::Uncertain => {
                    uncertain_flags.push(token.raw.to_string());
                    continue;
                }
            }
        }

        match classify_short_grep_flag(val) {
            ShortFlagResult::Drop => continue,
            ShortFlagResult::Keep(flags) => {
                push_suggestion_part(&mut suggestion, &flags, &mut has_parts);
                continue;
            }
            ShortFlagResult::NeedFixedStrings(remaining) => {
                need_fixed_strings = true;
                if let Some(flags) = remaining {
                    push_suggestion_part(&mut suggestion, &flags, &mut has_parts);
                }
                continue;
            }
            ShortFlagResult::Uncertain => {
                uncertain_flags.push(token.raw.to_string());
                continue;
            }
        }
    }

    if !uncertain_flags.is_empty() {
        return GrepRewrite::NeedsManualTranslation {
            flags: uncertain_flags,
        };
    }

    if need_fixed_strings && !is_fgrep {
        suggestion.insert_str(fixed_strings_insert_at, " -F");
    }

    GrepRewrite::Exact(suggestion)
}

fn push_suggestion_part(output: &mut String, part: &str, has_parts: &mut bool) {
    if *has_parts {
        output.push(' ');
    } else {
        *has_parts = true;
    }
    output.push_str(part);
}

enum LongFlagResult {
    Drop,
    Keep(String),
    NeedFixedStrings,
    Uncertain,
}

enum ShortFlagResult {
    Drop,
    Keep(String),
    NeedFixedStrings(Option<String>),
    Uncertain,
}

fn classify_long_grep_flag(flag: &str) -> LongFlagResult {
    match flag {
        "--recursive" | "--line-number" | "--extended-regexp" => LongFlagResult::Drop,
        "--fixed-strings" => LongFlagResult::NeedFixedStrings,
        "--colour" => LongFlagResult::Keep("--color".to_string()),
        "--ignore-case"
        | "--invert-match"
        | "--word-regexp"
        | "--line-regexp"
        | "--files-with-matches"
        | "--files-without-match"
        | "--count"
        | "--only-matching"
        | "--quiet"
        | "--regexp"
        | "--file"
        | "--after-context"
        | "--before-context"
        | "--context"
        | "--max-count"
        | "--color"
        | "--with-filename"
        | "--no-filename" => LongFlagResult::Keep(flag.to_string()),
        _ if flag.starts_with("--regexp=")
            || flag.starts_with("--file=")
            || flag.starts_with("--after-context=")
            || flag.starts_with("--before-context=")
            || flag.starts_with("--context=")
            || flag.starts_with("--max-count=") =>
        {
            LongFlagResult::Keep(flag.to_string())
        }
        _ if matches!(flag, "--color=auto" | "--color=always" | "--color=never") => {
            LongFlagResult::Keep(flag.to_string())
        }
        _ if matches!(flag, "--colour=auto" | "--colour=always" | "--colour=never") => {
            LongFlagResult::Keep(flag.replacen("--colour", "--color", 1))
        }
        _ => LongFlagResult::Uncertain,
    }
}

fn classify_short_grep_flag(flag: &str) -> ShortFlagResult {
    let bytes = flag.as_bytes();

    if bytes.len() == 2 {
        return match bytes[1] {
            b'r' | b'n' | b'E' => ShortFlagResult::Drop,
            b'F' => ShortFlagResult::NeedFixedStrings(None),
            byte if is_safe_no_value_short_grep_flag(byte)
                || is_safe_value_short_grep_flag(byte) =>
            {
                ShortFlagResult::Keep(flag.to_string())
            }
            _ => ShortFlagResult::Uncertain,
        };
    }

    if is_safe_attached_numeric_short_flag(bytes) {
        return ShortFlagResult::Keep(flag.to_string());
    }

    let mut kept = None::<String>;
    let mut had_fixed = false;

    for &byte in &bytes[1..] {
        match byte {
            b'r' | b'n' | b'E' => {}
            b'F' => had_fixed = true,
            byte if is_safe_no_value_short_grep_flag(byte) => {
                let kept = kept.get_or_insert_with(|| {
                    let mut value = String::with_capacity(bytes.len());
                    value.push('-');
                    value
                });
                kept.push(byte as char);
            }
            _ => return ShortFlagResult::Uncertain,
        }
    }

    if had_fixed {
        return ShortFlagResult::NeedFixedStrings(kept);
    }

    if let Some(kept) = kept {
        return ShortFlagResult::Keep(kept);
    }

    if bytes.len() > 2 {
        return ShortFlagResult::Drop;
    }

    ShortFlagResult::Uncertain
}

fn is_safe_no_value_short_grep_flag(byte: u8) -> bool {
    matches!(
        byte,
        b'a' | b'c' | b'H' | b'i' | b'I' | b'l' | b'o' | b'q' | b'v' | b'w' | b'x'
    )
}

fn is_safe_value_short_grep_flag(byte: u8) -> bool {
    matches!(byte, b'A' | b'B' | b'C' | b'e' | b'f' | b'm')
}

fn is_safe_attached_numeric_short_flag(flag: &[u8]) -> bool {
    matches!(flag, [b'-', b'A' | b'B' | b'C' | b'm', rest @ ..] if !rest.is_empty()
        && rest.iter().all(|byte| byte.is_ascii_digit()))
}

fn format_manual_translation_message(base: &str, flags: &[String]) -> String {
    let mut message = String::from(base);
    message.push_str("\nFlags requiring manual translation before switching to rg:");
    for flag in flags {
        message.push_str("\n  ");
        message.push_str(flag);
    }
    message.push_str(
        "\nTranslate those flags manually after checking `rg --help` instead of assuming they behave the same.",
    );
    message
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WrapperKind {
    Sudo,
    Env,
    Command,
    Time,
    Nohup,
    Builtin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GrepKind {
    Grep,
    Egrep,
    Fgrep,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AllowedCommand {
    Rg,
    Uv,
    Uvx,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BlockedCommand {
    Grep(GrepKind),
    Python,
    Pip,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TokenKind {
    Wrapper(WrapperKind),
    Allowed(AllowedCommand),
    Blocked(BlockedCommand),
    Other,
}

fn wrapper_option_takes_value(kind: WrapperKind, token: &[u8]) -> bool {
    match kind {
        WrapperKind::Sudo => matches!(
            token,
            b"-u"
                | b"--user"
                | b"-g"
                | b"--group"
                | b"-h"
                | b"--host"
                | b"-p"
                | b"--prompt"
                | b"-R"
                | b"--chroot"
                | b"-D"
                | b"--chdir"
                | b"-C"
                | b"--close-from"
                | b"-T"
                | b"--command-timeout"
        ),
        WrapperKind::Env => matches!(
            token,
            b"-u" | b"--unset" | b"-C" | b"--chdir" | b"-S" | b"--split-string" | b"--argv0"
        ),
        WrapperKind::Command | WrapperKind::Time | WrapperKind::Nohup | WrapperKind::Builtin => {
            false
        }
    }
}

fn uv_option_takes_value(token: &[u8]) -> bool {
    matches!(
        token,
        b"--directory"
            | b"--project"
            | b"--config-file"
            | b"--cache-dir"
            | b"--python"
            | b"--index"
            | b"--default-index"
            | b"--extra-index-url"
            | b"--find-links"
            | b"--index-url"
    )
}

fn classify_token(token: &[u8]) -> TokenKind {
    match normalized_program_name(token) {
        b"rg" | b"ripgrep" => TokenKind::Allowed(AllowedCommand::Rg),
        b"uv" => TokenKind::Allowed(AllowedCommand::Uv),
        b"uvx" => TokenKind::Allowed(AllowedCommand::Uvx),
        b"sudo" => TokenKind::Wrapper(WrapperKind::Sudo),
        b"env" => TokenKind::Wrapper(WrapperKind::Env),
        b"command" => TokenKind::Wrapper(WrapperKind::Command),
        b"nohup" => TokenKind::Wrapper(WrapperKind::Nohup),
        b"time" => TokenKind::Wrapper(WrapperKind::Time),
        b"builtin" => TokenKind::Wrapper(WrapperKind::Builtin),
        b"grep" => TokenKind::Blocked(BlockedCommand::Grep(GrepKind::Grep)),
        b"egrep" => TokenKind::Blocked(BlockedCommand::Grep(GrepKind::Egrep)),
        b"fgrep" => TokenKind::Blocked(BlockedCommand::Grep(GrepKind::Fgrep)),
        name if is_python_name(name) => TokenKind::Blocked(BlockedCommand::Python),
        name if is_pip_name(name) => TokenKind::Blocked(BlockedCommand::Pip),
        _ => TokenKind::Other,
    }
}

fn normalized_program_name(token: &[u8]) -> &[u8] {
    let mut start = 0usize;
    for (index, byte) in token.iter().enumerate() {
        if *byte == b'/' || *byte == b'\\' {
            start = index + 1;
        }
    }

    let name = &token[start..];
    strip_exe_suffix(name)
}

fn strip_exe_suffix(token: &[u8]) -> &[u8] {
    if token.len() > 4 && token[token.len() - 4..].eq_ignore_ascii_case(b".exe") {
        &token[..token.len() - 4]
    } else {
        token
    }
}

fn is_python_name(name: &[u8]) -> bool {
    if name == b"python" {
        return true;
    }

    if let Some(rest) = name.strip_prefix(b"python") {
        return !rest.is_empty()
            && rest
                .iter()
                .all(|byte| byte.is_ascii_digit() || *byte == b'.');
    }

    false
}

fn is_pip_name(name: &[u8]) -> bool {
    if name == b"pip" {
        return true;
    }

    if let Some(rest) = name.strip_prefix(b"pip") {
        return !rest.is_empty()
            && rest
                .iter()
                .all(|byte| byte.is_ascii_digit() || *byte == b'.');
    }

    false
}

fn is_shell_assignment(token: &[u8]) -> bool {
    let Some(index) = token.iter().position(|byte| *byte == b'=') else {
        return false;
    };
    let head = &token[..index];

    if head.is_empty() {
        return false;
    }

    if !(head[0].is_ascii_alphabetic() || head[0] == b'_') {
        return false;
    }

    head[1..]
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

fn configure_claude_hook(
    settings_path: &str,
    binary_name: &str,
    rules: RuleSet,
) -> Result<(), String> {
    configure_agent_hook(
        settings_path,
        "PreToolUse",
        "Bash",
        binary_name,
        "--claude-hook-json",
        rules,
    )
}

fn configure_gemini_hook(
    settings_path: &str,
    binary_name: &str,
    rules: RuleSet,
) -> Result<(), String> {
    configure_agent_hook(
        settings_path,
        "BeforeTool",
        "run_shell_command",
        binary_name,
        "--gemini-hook-json",
        rules,
    )
}

fn configure_codex_hook(
    settings_path: &str,
    binary_name: &str,
    rules: RuleSet,
) -> Result<(), String> {
    configure_agent_hook(
        settings_path,
        "PreToolUse",
        "Bash",
        binary_name,
        "--codex-hook-json",
        rules,
    )
}

fn configure_agent_hook(
    settings_path: &str,
    phase: &str,
    matcher: &str,
    binary_name: &str,
    hook_flag: &str,
    rules: RuleSet,
) -> Result<(), String> {
    let hook_command = build_hook_command(binary_name, hook_flag, rules);
    let input = fs::read_to_string(settings_path)
        .map_err(|error| format!("failed to read {settings_path}: {error}"))?;
    let mut settings: Value = serde_json::from_str(&input)
        .map_err(|error| format!("failed to parse {settings_path} as JSON: {error}"))?;

    update_hook_settings(
        &mut settings,
        phase,
        matcher,
        binary_name,
        hook_flag,
        &hook_command,
    )?;

    let mut serialized = serde_json::to_string_pretty(&settings)
        .map_err(|error| format!("failed to serialize updated settings: {error}"))?;
    serialized.push('\n');
    fs::write(settings_path, serialized)
        .map_err(|error| format!("failed to write {settings_path}: {error}"))?;
    Ok(())
}

fn build_hook_command(binary_name: &str, hook_flag: &str, rules: RuleSet) -> String {
    format!("{binary_name} {hook_flag} --rules {}", rules.cli_value())
}

fn update_hook_settings(
    settings: &mut Value,
    phase: &str,
    matcher: &str,
    binary_name: &str,
    hook_flag: &str,
    hook_command: &str,
) -> Result<(), String> {
    let settings_obj = ensure_object(settings, "settings root")?;
    let hooks = settings_obj
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks_obj = ensure_object(hooks, "`hooks`")?;
    let phase_list = hooks_obj
        .entry(phase.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let entries = ensure_array(phase_list, phase)?;

    let mut matched_entry_index = None;
    for (index, entry) in entries.iter().enumerate() {
        if entry
            .as_object()
            .and_then(|value| value.get("matcher"))
            .and_then(Value::as_str)
            == Some(matcher)
        {
            matched_entry_index = Some(index);
            break;
        }
    }

    if matched_entry_index.is_none() {
        entries.push(json!({
            "matcher": matcher,
            "hooks": [],
        }));
        matched_entry_index = Some(entries.len() - 1);
    }

    let entry = entries
        .get_mut(matched_entry_index.unwrap())
        .ok_or_else(|| "failed to select hook entry".to_string())?;
    let entry_obj = ensure_object(entry, "hook entry")?;
    let hook_list = entry_obj
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let hooks_array = ensure_array(hook_list, "hook list")?;

    let mut matched_index = None;
    let mut duplicate_indexes = Vec::new();

    for (index, hook) in hooks_array.iter().enumerate() {
        let Some(command) = hook
            .as_object()
            .and_then(|value| value.get("command"))
            .and_then(Value::as_str)
        else {
            continue;
        };

        if !hook_command_matches_existing(command, binary_name, hook_flag) {
            continue;
        }

        if matched_index.is_none() {
            matched_index = Some(index);
        } else {
            duplicate_indexes.push(index);
        }
    }

    if let Some(index) = matched_index {
        hooks_array[index] = json!({
            "type": "command",
            "command": hook_command,
        });

        for index in duplicate_indexes.into_iter().rev() {
            hooks_array.remove(index);
        }

        return Ok(());
    }

    hooks_array.push(json!({
        "type": "command",
        "command": hook_command,
    }));

    Ok(())
}

fn hook_command_matches_existing(command: &str, binary_name: &str, hook_flag: &str) -> bool {
    let binary_basename = binary_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(binary_name);

    command.contains(hook_flag)
        && (command.contains(binary_name) || command.contains(binary_basename))
}

fn ensure_object<'a>(
    value: &'a mut Value,
    context: &str,
) -> Result<&'a mut Map<String, Value>, String> {
    value
        .as_object_mut()
        .ok_or_else(|| format!("{context} must be a JSON object"))
}

fn ensure_array<'a>(value: &'a mut Value, context: &str) -> Result<&'a mut Vec<Value>, String> {
    value
        .as_array_mut()
        .ok_or_else(|| format!("{context} must be a JSON array"))
}

fn extract_tool_input_command(input: &str) -> Result<String, String> {
    let mut parser = JsonParser::new(input);
    let command = parser
        .parse_root_for_tool_input_command()?
        .ok_or_else(|| "stdin JSON did not contain tool_input.command".to_string())?;
    parser.skip_whitespace();
    if !parser.is_eof() {
        return Err("unexpected trailing JSON input".to_string());
    }
    Ok(command)
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> JsonParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            bytes: input.as_bytes(),
            index: 0,
        }
    }

    fn parse_root_for_tool_input_command(&mut self) -> Result<Option<String>, String> {
        self.skip_whitespace();
        self.expect_byte(b'{')?;

        loop {
            self.skip_whitespace();
            if self.consume_byte(b'}') {
                return Ok(None);
            }

            let key = self.parse_string()?;
            self.skip_whitespace();
            self.expect_byte(b':')?;

            if key == "tool_input" {
                let command = self.parse_tool_input_object()?;
                self.skip_object_tail()?;
                return Ok(command);
            }

            self.skip_value()?;
            self.skip_whitespace();
            if self.consume_byte(b',') {
                continue;
            }
            if self.consume_byte(b'}') {
                return Ok(None);
            }
            return Err("expected ',' or '}' in root object".to_string());
        }
    }

    fn parse_tool_input_object(&mut self) -> Result<Option<String>, String> {
        self.skip_whitespace();
        self.expect_byte(b'{')?;

        loop {
            self.skip_whitespace();
            if self.consume_byte(b'}') {
                return Ok(None);
            }

            let key = self.parse_string()?;
            self.skip_whitespace();
            self.expect_byte(b':')?;

            if key == "command" {
                let command = self.parse_string()?;
                self.skip_object_tail()?;
                return Ok(Some(command));
            }

            self.skip_value()?;
            self.skip_whitespace();
            if self.consume_byte(b',') {
                continue;
            }
            if self.consume_byte(b'}') {
                return Ok(None);
            }
            return Err("expected ',' or '}' in tool_input object".to_string());
        }
    }

    fn skip_object_tail(&mut self) -> Result<(), String> {
        self.skip_whitespace();
        while !self.consume_byte(b'}') {
            self.expect_byte(b',')?;
            self.skip_whitespace();
            let _ = self.parse_string()?;
            self.skip_whitespace();
            self.expect_byte(b':')?;
            self.skip_value()?;
            self.skip_whitespace();
        }
        Ok(())
    }

    fn skip_value(&mut self) -> Result<(), String> {
        self.skip_whitespace();
        match self.peek_byte() {
            Some(b'{') => self.skip_object(),
            Some(b'[') => self.skip_array(),
            Some(b'"') => {
                let _ = self.parse_string()?;
                Ok(())
            }
            Some(b'-' | b'0'..=b'9') => self.skip_number(),
            Some(b't') => self.expect_bytes(b"true"),
            Some(b'f') => self.expect_bytes(b"false"),
            Some(b'n') => self.expect_bytes(b"null"),
            _ => Err("unexpected JSON value".to_string()),
        }
    }

    fn skip_object(&mut self) -> Result<(), String> {
        self.expect_byte(b'{')?;
        loop {
            self.skip_whitespace();
            if self.consume_byte(b'}') {
                return Ok(());
            }
            let _ = self.parse_string()?;
            self.skip_whitespace();
            self.expect_byte(b':')?;
            self.skip_value()?;
            self.skip_whitespace();
            if self.consume_byte(b',') {
                continue;
            }
            if self.consume_byte(b'}') {
                return Ok(());
            }
            return Err("expected ',' or '}' in object".to_string());
        }
    }

    fn skip_array(&mut self) -> Result<(), String> {
        self.expect_byte(b'[')?;
        loop {
            self.skip_whitespace();
            if self.consume_byte(b']') {
                return Ok(());
            }
            self.skip_value()?;
            self.skip_whitespace();
            if self.consume_byte(b',') {
                continue;
            }
            if self.consume_byte(b']') {
                return Ok(());
            }
            return Err("expected ',' or ']' in array".to_string());
        }
    }

    fn skip_number(&mut self) -> Result<(), String> {
        if self.consume_byte(b'-') {}

        match self.peek_byte() {
            Some(b'0') => {
                self.index += 1;
            }
            Some(b'1'..=b'9') => {
                self.index += 1;
                while matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                    self.index += 1;
                }
            }
            _ => return Err("invalid number".to_string()),
        }

        if self.consume_byte(b'.') {
            if !matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                return Err("invalid fractional number".to_string());
            }
            while matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
        }

        if matches!(self.peek_byte(), Some(b'e' | b'E')) {
            self.index += 1;
            if matches!(self.peek_byte(), Some(b'+' | b'-')) {
                self.index += 1;
            }
            if !matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                return Err("invalid exponent".to_string());
            }
            while matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
        }

        Ok(())
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.expect_byte(b'"')?;
        let mut output = String::new();

        loop {
            let byte = self
                .next_byte()
                .ok_or_else(|| "unterminated JSON string".to_string())?;

            match byte {
                b'"' => return Ok(output),
                b'\\' => {
                    let escaped = self
                        .next_byte()
                        .ok_or_else(|| "unterminated JSON escape".to_string())?;
                    match escaped {
                        b'"' => output.push('"'),
                        b'\\' => output.push('\\'),
                        b'/' => output.push('/'),
                        b'b' => output.push('\u{0008}'),
                        b'f' => output.push('\u{000C}'),
                        b'n' => output.push('\n'),
                        b'r' => output.push('\r'),
                        b't' => output.push('\t'),
                        b'u' => output.push(self.parse_unicode_escape()?),
                        _ => return Err("invalid JSON escape".to_string()),
                    }
                }
                byte if byte < 0x20 => return Err("control character in JSON string".to_string()),
                _ => output.push(byte as char),
            }
        }
    }

    fn parse_unicode_escape(&mut self) -> Result<char, String> {
        let first = self.parse_hex_u16()?;
        if !(0xD800..=0xDBFF).contains(&first) {
            return char::from_u32(first as u32)
                .ok_or_else(|| "invalid unicode scalar".to_string());
        }

        self.expect_byte(b'\\')?;
        self.expect_byte(b'u')?;
        let second = self.parse_hex_u16()?;
        if !(0xDC00..=0xDFFF).contains(&second) {
            return Err("invalid unicode surrogate pair".to_string());
        }

        let scalar = 0x10000 + (((first as u32 - 0xD800) << 10) | (second as u32 - 0xDC00));
        char::from_u32(scalar).ok_or_else(|| "invalid unicode scalar".to_string())
    }

    fn parse_hex_u16(&mut self) -> Result<u16, String> {
        let mut value = 0u16;
        for _ in 0..4 {
            let digit = self
                .next_byte()
                .ok_or_else(|| "incomplete unicode escape".to_string())?;
            value = (value << 4)
                | match digit {
                    b'0'..=b'9' => (digit - b'0') as u16,
                    b'a'..=b'f' => (digit - b'a' + 10) as u16,
                    b'A'..=b'F' => (digit - b'A' + 10) as u16,
                    _ => return Err("invalid unicode escape".to_string()),
                };
        }
        Ok(value)
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek_byte(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.index += 1;
        }
    }

    fn consume_byte(&mut self, expected: u8) -> bool {
        if self.peek_byte() == Some(expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn expect_byte(&mut self, expected: u8) -> Result<(), String> {
        match self.next_byte() {
            Some(actual) if actual == expected => Ok(()),
            Some(actual) => Err(format!(
                "expected '{}', found '{}'",
                expected as char, actual as char
            )),
            None => Err(format!(
                "expected '{}', found end of input",
                expected as char
            )),
        }
    }

    fn expect_bytes(&mut self, expected: &[u8]) -> Result<(), String> {
        for byte in expected {
            self.expect_byte(*byte)?;
        }
        Ok(())
    }

    fn next_byte(&mut self) -> Option<u8> {
        let byte = self.peek_byte()?;
        self.index += 1;
        Some(byte)
    }

    fn peek_byte(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn is_eof(&self) -> bool {
        self.index >= self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn decision_message(command: &str) -> String {
        decision_message_with_rules(command, RuleSet::all())
    }

    fn decision_message_with_rules(command: &str, rules: RuleSet) -> String {
        evaluate_command(command, rules).unwrap().message
    }

    #[test]
    fn blocks_grep() {
        assert_eq!(
            decision_message("grep -rn pattern ."),
            format_exact_suggestion(GREP_MESSAGE, "rg pattern .")
        );
    }

    #[test]
    fn suggests_exact_rg_rewrites() {
        assert!(decision_message("grep pattern file.txt").contains("rg pattern file.txt"));
        assert!(decision_message("grep -E 'foo|bar' .").contains("rg 'foo|bar' ."));
        assert!(decision_message("grep -rni pattern .").contains("rg -i pattern ."));
        assert!(decision_message("grep -A 3 pattern file.txt").contains("rg -A 3 pattern file.txt"));
        assert!(decision_message("grep --color=auto pattern file.txt")
            .contains("rg --color=auto pattern file.txt"));
        assert!(decision_message("grep -rFiv 'literal' file.txt")
            .contains("rg -F -iv 'literal' file.txt"));
    }

    #[test]
    fn requires_manual_translation_for_uncertain_grep_flags() {
        let message = decision_message("grep -s pattern file.txt");
        assert!(message.contains("Flags requiring manual translation"));
        assert!(message.contains("\n  -s"));
        assert!(!message.contains("Suggested replacement"));
    }

    #[test]
    fn handles_wrapped_and_chained_grep_commands() {
        assert!(decision_message("sudo -u root grep pattern /etc/hosts")
            .contains("sudo -u root rg pattern /etc/hosts"));
        assert!(decision_message("env FOO=1 grep pattern file.txt")
            .contains("env FOO=1 rg pattern file.txt"));
        assert!(decision_message("cd /tmp && grep -rn TODO .").contains("rg TODO ."));
        assert!(decision_message("cat file.txt | grep pattern").contains("rg pattern"));
    }

    #[test]
    fn preserves_quoted_and_escaped_grep_tokens() {
        assert!(decision_message("grep \"two words\" 'file name.txt'")
            .contains("rg \"two words\" 'file name.txt'"));
        assert!(decision_message("grep foo\\ bar file.txt").contains("rg foo\\ bar file.txt"));
        assert!(decision_message("grep -- -foo file.txt").contains("rg -- -foo file.txt"));
    }

    #[test]
    fn suggests_exact_python_rewrites() {
        let message = decision_message("python -m pytest");
        assert!(message.contains(PYTHON_MESSAGE));
        assert!(message.contains("uv run python -m pytest"));

        let quoted = decision_message(r#"python -c "print(\"ok\")""#);
        assert!(quoted.contains(r#"uv run python -c "print(\"ok\")""#));

        let wrapped = decision_message("sudo env FOO=1 python script.py");
        assert!(wrapped.contains("sudo env FOO=1 uv run python script.py"));
    }

    #[test]
    fn suggests_confidence_graded_pip_rewrites() {
        let install = decision_message("pip install requests");
        assert!(install.contains("Likely alternatives:"));
        assert!(install.contains("uv add requests"));
        assert!(install.contains("uv pip install requests"));

        let install_requirements = decision_message("pip install -r requirements.txt");
        assert!(install_requirements.contains("uv pip install -r requirements.txt"));
        assert!(!install_requirements.contains("uv add requirements.txt"));

        let uninstall = decision_message("pip uninstall black");
        assert!(uninstall.contains("uv remove black"));
        assert!(uninstall.contains("uv pip uninstall black"));
    }

    #[test]
    fn allows_uv_and_rg_usage() {
        assert_eq!(evaluate_command("uv run pytest", RuleSet::all()), None);
        assert_eq!(
            evaluate_command("uv --directory repo run pytest", RuleSet::all()),
            None
        );
        assert_eq!(evaluate_command("uvx ruff check .", RuleSet::all()), None);
        assert_eq!(evaluate_command("rg pattern .", RuleSet::all()), None);
        assert_eq!(evaluate_command("ripgrep pattern .", RuleSet::all()), None);
    }

    #[test]
    fn blocks_uv_init() {
        assert_eq!(decision_message("uv init"), UV_INIT_MESSAGE);
        assert_eq!(
            decision_message("uv --directory repo init"),
            UV_INIT_MESSAGE
        );
        assert_eq!(
            decision_message("env FOO=1 uv --project repo init"),
            UV_INIT_MESSAGE
        );
    }

    #[test]
    fn avoids_argument_false_positives() {
        assert_eq!(evaluate_command("echo python", RuleSet::all()), None);
        assert_eq!(evaluate_command("printf '%s' grep", RuleSet::all()), None);
        assert_eq!(evaluate_command("uv run echo init", RuleSet::all()), None);
    }

    #[test]
    fn resumes_parsing_after_segment_separators() {
        let and_then = decision_message("uv run pytest && python -m pytest");
        assert!(and_then.contains("uv run python -m pytest"));

        let sequence = decision_message("uv run pytest; python -m pytest");
        assert!(sequence.contains("uv run python -m pytest"));

        let quoted = decision_message("uv run \"foo && bar\" && python -m pytest");
        assert!(quoted.contains("uv run python -m pytest"));
    }

    #[test]
    fn parses_tool_hook_json() {
        let input =
            r#"{"tool_name":"Bash","tool_input":{"command":"python -m pytest","cwd":"/tmp"}}"#;
        assert_eq!(
            extract_tool_input_command(input).unwrap(),
            "python -m pytest".to_string()
        );
    }

    #[test]
    fn parses_escaped_json_command() {
        let input = r#"{"tool_input":{"command":"grep -rn \"pattern\" .","cwd":"/tmp"}}"#;
        assert_eq!(
            extract_tool_input_command(input).unwrap(),
            "grep -rn \"pattern\" .".to_string()
        );
    }

    #[test]
    fn parses_codex_hook_flag() {
        let config = Config::parse(["--codex-hook-json".to_string()]).unwrap();

        match config.mode {
            Mode::Evaluate {
                input: InputMode::HookJson,
                json_block_output: true,
                rules: _,
            } => {}
            mode => panic!("unexpected mode: {mode:?}"),
        }
    }

    #[test]
    fn parses_rules_flag() {
        let config = Config::parse([
            "--command".to_string(),
            "python -m pytest".to_string(),
            "--rules".to_string(),
            "uv".to_string(),
        ])
        .unwrap();

        match config.mode {
            Mode::Evaluate {
                input: InputMode::Command(command),
                json_block_output: false,
                rules,
            } => {
                assert_eq!(command, "python -m pytest");
                assert_eq!(rules, RuleSet::only(RuleId::Uv));
            }
            mode => panic!("unexpected mode: {mode:?}"),
        }
    }

    #[test]
    fn parses_codex_hook_configuration_flag() {
        let config = Config::parse([
            "--configure-codex-hook".to_string(),
            "/tmp/hooks.json".to_string(),
            format!("/tmp/{BINARY_NAME}"),
            "--rules".to_string(),
            "rg".to_string(),
        ])
        .unwrap();

        match config.mode {
            Mode::ConfigureCodexHook {
                settings_path,
                binary_name,
                rules,
            } => {
                assert_eq!(settings_path, "/tmp/hooks.json");
                assert_eq!(binary_name, format!("/tmp/{BINARY_NAME}"));
                assert_eq!(rules, RuleSet::only(RuleId::Ripgrep));
            }
            mode => panic!("unexpected mode: {mode:?}"),
        }
    }

    #[test]
    fn supports_selective_rule_sets() {
        assert_eq!(
            evaluate_command("grep -rn pattern .", RuleSet::only(RuleId::Uv)),
            None
        );
        assert_eq!(
            evaluate_command("python -m pytest", RuleSet::only(RuleId::Ripgrep)),
            None
        );
        assert_eq!(
            evaluate_command("uv init", RuleSet::only(RuleId::Ripgrep)),
            None
        );

        let rg_only =
            decision_message_with_rules("grep -rn pattern .", RuleSet::only(RuleId::Ripgrep));
        assert!(rg_only.contains("rg pattern ."));

        let uv_only = decision_message_with_rules("python -m pytest", RuleSet::only(RuleId::Uv));
        assert!(uv_only.contains("uv run python -m pytest"));
    }

    #[test]
    fn updates_hook_settings_without_duplicates() {
        let mut settings = json!({});
        let hook_command = build_hook_command(BINARY_NAME, "--claude-hook-json", RuleSet::all());

        update_hook_settings(
            &mut settings,
            "PreToolUse",
            "Bash",
            BINARY_NAME,
            "--claude-hook-json",
            &hook_command,
        )
        .unwrap();
        update_hook_settings(
            &mut settings,
            "PreToolUse",
            "Bash",
            BINARY_NAME,
            "--claude-hook-json",
            &hook_command,
        )
        .unwrap();

        assert_eq!(
            settings,
            json!({
              "hooks": {
                "PreToolUse": [{
                  "matcher": "Bash",
                  "hooks": [{
                    "type": "command",
                    "command": hook_command
                  }]
                }]
              }
            })
        );
    }

    #[test]
    fn updates_codex_hook_settings_without_duplicates() {
        let mut settings = json!({});
        let hook_command = build_hook_command(
            &format!("/tmp/{BINARY_NAME}"),
            "--codex-hook-json",
            RuleSet::all(),
        );

        update_hook_settings(
            &mut settings,
            "PreToolUse",
            "Bash",
            &format!("/tmp/{BINARY_NAME}"),
            "--codex-hook-json",
            &hook_command,
        )
        .unwrap();
        update_hook_settings(
            &mut settings,
            "PreToolUse",
            "Bash",
            &format!("/tmp/{BINARY_NAME}"),
            "--codex-hook-json",
            &hook_command,
        )
        .unwrap();

        assert_eq!(
            settings,
            json!({
              "hooks": {
                "PreToolUse": [{
                  "matcher": "Bash",
                  "hooks": [{
                    "type": "command",
                    "command": hook_command
                  }]
                }]
              }
            })
        );
    }

    #[test]
    fn rewrites_existing_hook_command_when_rule_set_changes() {
        let mut settings = json!({
          "hooks": {
            "PreToolUse": [{
              "matcher": "Bash",
              "hooks": [{
                "type": "command",
                "command": format!("/tmp/{BINARY_NAME} --codex-hook-json")
              }]
            }]
          }
        });

        let new_hook_command = build_hook_command(
            &format!("/tmp/{BINARY_NAME}"),
            "--codex-hook-json",
            RuleSet::only(RuleId::Ripgrep),
        );

        update_hook_settings(
            &mut settings,
            "PreToolUse",
            "Bash",
            &format!("/tmp/{BINARY_NAME}"),
            "--codex-hook-json",
            &new_hook_command,
        )
        .unwrap();

        assert_eq!(
            settings,
            json!({
              "hooks": {
                "PreToolUse": [{
                  "matcher": "Bash",
                  "hooks": [{
                    "type": "command",
                    "command": new_hook_command
                  }]
                }]
              }
            })
        );
    }
}
