use std::env;
use std::io::{self, Read};
use std::process;
use std::time::Instant;

const PYTHON_MESSAGE: &str = "Use uv instead of bare Python or pip commands in this project. Replace the blocked command with 'uv run ...', 'uv add ...', 'uv add --dev ...', 'uv remove ...', or 'uv run --with ...' as appropriate.";
const UV_INIT_MESSAGE: &str = "Do not run 'uv init' in an existing project unless the user explicitly asks for project creation or conversion. Inspect the repo first and prefer 'uv run', 'uv add', 'uv sync', or 'uv run --with'. If project initialization is truly needed, use 'uv init --no-readme --no-workspace' to avoid overwriting existing files and git history.";

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
        Mode::Evaluate { input, claude_json } => {
            let raw = match input {
                InputMode::Command(text) => text,
                InputMode::StdinCommand => read_stdin()?,
                InputMode::ClaudeHookJson => extract_claude_command(&read_stdin()?)?,
            };

            match evaluate_command(raw.trim()) {
                Some(decision) if claude_json => {
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
        } => {
            if iterations == 0 {
                return Err("iterations must be greater than 0".to_string());
            }

            let start = Instant::now();
            let mut blocks = 0u64;

            for _ in 0..iterations {
                if evaluate_command(&command).is_some() {
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
    }
}

#[derive(Debug)]
struct Config {
    mode: Mode,
}

#[derive(Debug)]
enum Mode {
    Evaluate { input: InputMode, claude_json: bool },
    Benchmark { command: String, iterations: u64 },
}

#[derive(Debug)]
enum InputMode {
    Command(String),
    StdinCommand,
    ClaudeHookJson,
}

impl Config {
    fn parse<I>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = String>,
    {
        let mut input: Option<InputMode> = None;
        let mut claude_json = false;
        let mut benchmark_command: Option<String> = None;
        let mut iterations = 100_000u64;

        let mut iter = args.into_iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
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
                    input = Some(InputMode::ClaudeHookJson);
                    claude_json = true;
                }
                "--gemini-hook-json" => {
                    input = Some(InputMode::ClaudeHookJson);
                }
                "--claude-json" => {
                    claude_json = true;
                }
                "--benchmark-command" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| "missing value for --benchmark-command".to_string())?;
                    benchmark_command = Some(value);
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
                            claude_json: false,
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
                },
            });
        }

        let input = input.ok_or_else(|| {
            "expected one of --command, --stdin-command, or --claude-hook-json".to_string()
        })?;

        Ok(Self {
            mode: Mode::Evaluate { input, claude_json },
        })
    }
}

fn print_usage() {
    println!(
        "Usage:\n  enforce-uv-command --command \"python -m pytest\" [--claude-json]\n  enforce-uv-command --stdin-command [--claude-json]\n  enforce-uv-command --claude-hook-json\n  enforce-uv-command --gemini-hook-json\n  enforce-uv-command --benchmark-command \"python -m pytest\" [--iterations 1000000]"
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

fn evaluate_command(command: &str) -> Option<BlockDecision> {
    let bytes = command.as_bytes();
    let mut tokens = Vec::with_capacity(8);
    let mut raw = Vec::with_capacity(32);
    let mut value = Vec::with_capacity(32);
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut tracker = EvaluationTracker::default();
    let mut index = 0usize;

    while index < bytes.len() {
        let byte = bytes[index];

        if in_single_quote {
            raw.push(byte);
            if byte == b'\'' {
                in_single_quote = false;
            } else {
                value.push(byte);
            }
            index += 1;
            continue;
        }

        if in_double_quote {
            match byte {
                b'"' => {
                    raw.push(byte);
                    in_double_quote = false;
                }
                b'\\' => {
                    raw.push(byte);
                    if index + 1 < bytes.len() {
                        index += 1;
                        raw.push(bytes[index]);
                        value.push(bytes[index]);
                    } else {
                        value.push(b'\\');
                    }
                }
                _ => {
                    raw.push(byte);
                    value.push(byte);
                }
            }
            index += 1;
            continue;
        }

        match byte {
            b' ' | b'\n' | b'\r' | b'\t' => {
                if let Some(decision) =
                    finish_evaluation_token(&mut raw, &mut value, &mut tokens, &mut tracker)
                {
                    return Some(decision);
                }

                if tracker.state == EvaluationState::SkipSegment {
                    reset_evaluation_segment(&mut tokens, &mut tracker);
                    index = skip_segment_tail(
                        bytes,
                        index + 1,
                        &mut in_single_quote,
                        &mut in_double_quote,
                    );
                    continue;
                }
            }
            b'\'' => {
                raw.push(byte);
                in_single_quote = true;
            }
            b'"' => {
                raw.push(byte);
                in_double_quote = true;
            }
            b';' => {
                if let Some(decision) =
                    finish_evaluation_token(&mut raw, &mut value, &mut tokens, &mut tracker)
                {
                    return Some(decision);
                }

                if let Some(decision) = finalize_evaluation_segment(&mut tokens, &mut tracker) {
                    return Some(decision);
                }
            }
            b'|' | b'&' => {
                if let Some(decision) =
                    finish_evaluation_token(&mut raw, &mut value, &mut tokens, &mut tracker)
                {
                    return Some(decision);
                }

                if let Some(decision) = finalize_evaluation_segment(&mut tokens, &mut tracker) {
                    return Some(decision);
                }

                if index + 1 < bytes.len() && bytes[index + 1] == byte {
                    index += 1;
                }
            }
            b'\\' => {
                raw.push(byte);
                if index + 1 < bytes.len() {
                    index += 1;
                    raw.push(bytes[index]);
                    value.push(bytes[index]);
                } else {
                    value.push(b'\\');
                }
            }
            _ => {
                raw.push(byte);
                value.push(byte);
            }
        }

        index += 1;
    }

    if let Some(decision) = finish_evaluation_token(&mut raw, &mut value, &mut tokens, &mut tracker)
    {
        return Some(decision);
    }

    finalize_evaluation_segment(&mut tokens, &mut tracker)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ParsedToken {
    raw: String,
    value: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EvaluationState {
    SeekCommand,
    SeekUvDecision,
    PreserveSegment,
    SkipSegment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingDecisionKind {
    Python { command_index: usize },
    Pip { command_index: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EvaluationTracker {
    state: EvaluationState,
    wrapper: Option<WrapperKind>,
    skip_next_value: bool,
    pending_decision: Option<PendingDecisionKind>,
}

impl Default for EvaluationTracker {
    fn default() -> Self {
        Self {
            state: EvaluationState::SeekCommand,
            wrapper: None,
            skip_next_value: false,
            pending_decision: None,
        }
    }
}

fn finish_evaluation_token(
    raw: &mut Vec<u8>,
    value: &mut Vec<u8>,
    tokens: &mut Vec<ParsedToken>,
    tracker: &mut EvaluationTracker,
) -> Option<BlockDecision> {
    if raw.is_empty() {
        return None;
    }

    let token_index = tokens.len();
    if let Some(decision) = update_evaluation_tracker(value, tracker, token_index) {
        raw.clear();
        value.clear();
        return Some(decision);
    }

    tokens.push(ParsedToken {
        raw: String::from_utf8_lossy(raw).into_owned(),
        value: String::from_utf8_lossy(value).into_owned(),
    });
    raw.clear();
    value.clear();
    None
}

fn update_evaluation_tracker(
    value: &[u8],
    tracker: &mut EvaluationTracker,
    token_index: usize,
) -> Option<BlockDecision> {
    match tracker.state {
        EvaluationState::PreserveSegment | EvaluationState::SkipSegment => return None,
        EvaluationState::SeekCommand if tracker.skip_next_value => {
            tracker.skip_next_value = false;
            return None;
        }
        EvaluationState::SeekUvDecision if tracker.skip_next_value => {
            tracker.skip_next_value = false;
            return None;
        }
        _ => {}
    }

    match tracker.state {
        EvaluationState::SeekCommand => {
            if value.starts_with(b"-") {
                if let Some(wrapper_kind) = tracker.wrapper {
                    tracker.skip_next_value = wrapper_option_takes_value(wrapper_kind, value);
                }
                return None;
            }

            if is_shell_assignment(value) {
                return None;
            }

            match classify_token(value) {
                TokenKind::Wrapper => {
                    tracker.wrapper = wrapper_kind(value);
                }
                TokenKind::Uv => {
                    tracker.state = EvaluationState::SeekUvDecision;
                    tracker.wrapper = None;
                }
                TokenKind::Uvx | TokenKind::Other => {
                    tracker.state = EvaluationState::SkipSegment;
                }
                TokenKind::PythonLike => {
                    tracker.state = EvaluationState::PreserveSegment;
                    tracker.pending_decision = Some(PendingDecisionKind::Python {
                        command_index: token_index,
                    });
                }
                TokenKind::PipLike => {
                    tracker.state = EvaluationState::PreserveSegment;
                    tracker.pending_decision = Some(PendingDecisionKind::Pip {
                        command_index: token_index,
                    });
                }
            }
        }
        EvaluationState::SeekUvDecision => {
            if value.starts_with(b"-") {
                tracker.skip_next_value = uv_option_takes_value(value);
                return None;
            }

            if is_shell_assignment(value) {
                return None;
            }

            let name = normalized_program_name(value);
            if name == b"init" {
                return Some(BlockDecision::new(UV_INIT_MESSAGE));
            }

            tracker.state = EvaluationState::SkipSegment;
        }
        EvaluationState::PreserveSegment | EvaluationState::SkipSegment => {}
    }

    None
}

#[cold]
#[inline(never)]
fn skip_segment_tail(
    bytes: &[u8],
    mut index: usize,
    in_single_quote: &mut bool,
    in_double_quote: &mut bool,
) -> usize {
    let mut escape_next = false;

    while index < bytes.len() {
        let byte = bytes[index];

        if escape_next {
            escape_next = false;
            index += 1;
            continue;
        }

        if *in_single_quote {
            if byte == b'\'' {
                *in_single_quote = false;
            }
            index += 1;
            continue;
        }

        if *in_double_quote {
            match byte {
                b'"' => *in_double_quote = false,
                b'\\' => escape_next = true,
                _ => {}
            }
            index += 1;
            continue;
        }

        match byte {
            b'\'' => {
                *in_single_quote = true;
                index += 1;
            }
            b'"' => {
                *in_double_quote = true;
                index += 1;
            }
            b'\\' => {
                escape_next = true;
                index += 1;
            }
            b';' => {
                return index + 1;
            }
            b'|' | b'&' => {
                return if index + 1 < bytes.len() && bytes[index + 1] == byte {
                    index + 2
                } else {
                    index + 1
                };
            }
            _ => index += 1,
        }
    }

    index
}

fn finalize_evaluation_segment(
    tokens: &mut Vec<ParsedToken>,
    tracker: &mut EvaluationTracker,
) -> Option<BlockDecision> {
    let decision = match tracker.pending_decision.take() {
        Some(PendingDecisionKind::Python { command_index }) => {
            Some(build_python_decision(tokens, command_index))
        }
        Some(PendingDecisionKind::Pip { command_index }) => {
            Some(build_pip_decision(tokens, command_index))
        }
        None => None,
    };

    reset_evaluation_segment(tokens, tracker);
    decision
}

fn reset_evaluation_segment(tokens: &mut Vec<ParsedToken>, tracker: &mut EvaluationTracker) {
    tokens.clear();
    *tracker = EvaluationTracker::default();
}

fn build_python_decision(tokens: &[ParsedToken], command_index: usize) -> BlockDecision {
    let suggestion = insert_before_command(tokens, command_index, &["uv", "run"]);
    BlockDecision::new(format_exact_suggestion(PYTHON_MESSAGE, &suggestion))
}

fn build_pip_decision(tokens: &[ParsedToken], command_index: usize) -> BlockDecision {
    let pip_rewrite = replace_command(tokens, command_index, 1, &["uv", "pip"]);
    let Some(subcommand) = tokens
        .get(command_index + 1)
        .map(|token| token.value.as_str())
    else {
        return BlockDecision::new(format_exact_suggestion(PYTHON_MESSAGE, &pip_rewrite));
    };

    if subcommand.eq_ignore_ascii_case("install") {
        return build_pip_install_decision(tokens, command_index, pip_rewrite);
    }

    if subcommand.eq_ignore_ascii_case("uninstall") {
        return build_pip_uninstall_decision(tokens, command_index, pip_rewrite);
    }

    BlockDecision::new(format_exact_suggestion(PYTHON_MESSAGE, &pip_rewrite))
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

fn wrapper_kind(token: &[u8]) -> Option<WrapperKind> {
    match normalized_program_name(token) {
        b"sudo" => Some(WrapperKind::Sudo),
        b"env" => Some(WrapperKind::Env),
        b"command" => Some(WrapperKind::Command),
        b"time" => Some(WrapperKind::Time),
        b"nohup" => Some(WrapperKind::Nohup),
        b"builtin" => Some(WrapperKind::Builtin),
        _ => None,
    }
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

fn build_pip_install_decision(
    tokens: &[ParsedToken],
    command_index: usize,
    pip_rewrite: String,
) -> BlockDecision {
    let dependency_args = &tokens[command_index + 2..];
    if is_high_confidence_dependency_list(dependency_args) {
        let project_rewrite = replace_command(tokens, command_index, 2, &["uv", "add"]);
        return BlockDecision::new(format_alternative_suggestions(
            PYTHON_MESSAGE,
            &[project_rewrite, pip_rewrite],
            Some("Choose `uv add` for project dependencies; choose `uv pip` to keep pip-style behavior."),
        ));
    }

    BlockDecision::new(format_exact_suggestion_with_note(
        PYTHON_MESSAGE,
        &pip_rewrite,
        "Use `uv add ...` only when you intentionally want to modify project dependencies.",
    ))
}

fn build_pip_uninstall_decision(
    tokens: &[ParsedToken],
    command_index: usize,
    pip_rewrite: String,
) -> BlockDecision {
    let dependency_args = &tokens[command_index + 2..];
    if is_high_confidence_dependency_list(dependency_args) {
        let project_rewrite = replace_command(tokens, command_index, 2, &["uv", "remove"]);
        return BlockDecision::new(format_alternative_suggestions(
            PYTHON_MESSAGE,
            &[project_rewrite, pip_rewrite],
            Some("Choose `uv remove` when the package belongs in project metadata; choose `uv pip` for pip-style environment changes."),
        ));
    }

    BlockDecision::new(format_exact_suggestion_with_note(
        PYTHON_MESSAGE,
        &pip_rewrite,
        "Use `uv remove ...` only when you intentionally want to update project dependencies.",
    ))
}

fn insert_before_command(
    tokens: &[ParsedToken],
    command_index: usize,
    inserted: &[&str],
) -> String {
    rewrite_command(tokens, command_index, 0, inserted)
}

fn replace_command(
    tokens: &[ParsedToken],
    command_index: usize,
    consumed: usize,
    replacement: &[&str],
) -> String {
    rewrite_command(tokens, command_index, consumed, replacement)
}

fn rewrite_command(
    tokens: &[ParsedToken],
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
        push_command_part(&mut output, &token.raw, &mut needs_space);
    }
    for item in replacement {
        push_command_part(&mut output, item, &mut needs_space);
    }
    for token in &tokens[suffix_start..] {
        push_command_part(&mut output, &token.raw, &mut needs_space);
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

fn format_exact_suggestion_with_note(base: &str, suggestion: &str, note: &str) -> String {
    let mut message = format_exact_suggestion(base, suggestion);
    message.push('\n');
    message.push_str(note);
    message
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

fn is_high_confidence_dependency_list(tokens: &[ParsedToken]) -> bool {
    !tokens.is_empty()
        && tokens
            .iter()
            .all(|token| is_high_confidence_dependency_arg(&token.value))
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TokenKind {
    Wrapper,
    Uv,
    Uvx,
    PythonLike,
    PipLike,
    Other,
}

fn classify_token(token: &[u8]) -> TokenKind {
    let name = normalized_program_name(token);
    match name {
        b"uv" => TokenKind::Uv,
        b"uvx" => TokenKind::Uvx,
        b"sudo" | b"env" | b"command" | b"nohup" | b"time" | b"builtin" => TokenKind::Wrapper,
        _ if is_python_name(name) => TokenKind::PythonLike,
        _ if is_pip_name(name) => TokenKind::PipLike,
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

fn extract_claude_command(input: &str) -> Result<String, String> {
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

    fn decision_message(command: &str) -> String {
        evaluate_command(command).unwrap().message
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

        let sudo_with_value = decision_message("sudo -u root python script.py");
        assert!(sudo_with_value.contains("sudo -u root uv run python script.py"));
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

        let listing = decision_message("pip list --format=json");
        assert!(listing.contains("uv pip list --format=json"));
    }

    #[test]
    fn allows_uv_usage() {
        assert_eq!(evaluate_command("uv run pytest"), None);
        assert_eq!(evaluate_command("uv --directory repo run pytest"), None);
        assert_eq!(evaluate_command("uvx ruff check ."), None);
        assert_eq!(evaluate_command("sudo env FOO=1 uv run pytest"), None);
    }

    #[test]
    fn blocks_uv_init() {
        assert_eq!(decision_message("uv init"), UV_INIT_MESSAGE);
        assert_eq!(
            decision_message("uv --directory repo init"),
            UV_INIT_MESSAGE
        );
    }

    #[test]
    fn avoids_argument_false_positives() {
        assert_eq!(evaluate_command("echo python"), None);
        assert_eq!(evaluate_command("printf '%s' python"), None);
        assert_eq!(evaluate_command("uv run echo init"), None);
    }

    #[test]
    fn resumes_parsing_after_safe_segment_separator() {
        let and_then = decision_message("uv run pytest && python -m pytest");
        assert!(and_then.contains("uv run python -m pytest"));

        let sequence = decision_message("uv run pytest; python -m pytest");
        assert!(sequence.contains("uv run python -m pytest"));
    }

    #[test]
    fn ignores_quoted_separators_inside_safe_segment_arguments() {
        let message = decision_message("uv run \"foo && bar\" && python -m pytest");
        assert!(message.contains("uv run python -m pytest"));
        assert_eq!(evaluate_command("uv run \"foo && bar\""), None);
    }

    #[test]
    fn parses_claude_hook_json() {
        let input =
            r#"{"tool_name":"Bash","tool_input":{"command":"python -m pytest","cwd":"/tmp"}}"#;
        assert_eq!(
            extract_claude_command(input).unwrap(),
            "python -m pytest".to_string()
        );
    }

    #[test]
    fn parses_escaped_json_command() {
        let input = r#"{"tool_input":{"command":"python -c \"print(\\\"ok\\\")\"","cwd":"/tmp"}}"#;
        assert_eq!(
            extract_claude_command(input).unwrap(),
            "python -c \"print(\\\"ok\\\")\"".to_string()
        );
    }
}
