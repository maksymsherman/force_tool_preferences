use std::env;
use std::io::{self, Read};
use std::process;
use std::time::Instant;

const PYTHON_MESSAGE: &str = "Use uv instead of bare Python or pip commands in this project. Replace the blocked command with 'uv run ...', 'uv add ...', 'uv add --dev ...', 'uv remove ...', or 'uv run --with ...' as appropriate.";
const UV_INIT_MESSAGE: &str = "Do not run 'uv init' in an existing project unless the user explicitly asks for project creation or conversion. Inspect the repo first and prefer 'uv run', 'uv add', 'uv sync', or 'uv run --with'.";

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
                Some(reason) if claude_json => {
                    println!(
                        "{{\"decision\":\"block\",\"reason\":\"{}\"}}",
                        escape_json(reason)
                    );
                    Ok(0)
                }
                Some(reason) => {
                    eprintln!("{reason}");
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
        "Usage:\n  enforce-uv-command --command \"python -m pytest\" [--claude-json]\n  enforce-uv-command --stdin-command [--claude-json]\n  enforce-uv-command --claude-hook-json\n  enforce-uv-command --benchmark-command \"python -m pytest\" [--iterations 1000000]"
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

fn evaluate_command(command: &str) -> Option<&'static str> {
    let bytes = command.as_bytes();
    let mut token = Vec::with_capacity(32);
    let mut state = SegmentState::SeekCommand;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escape_next = false;
    let mut index = 0usize;

    while index < bytes.len() {
        let byte = bytes[index];

        if escape_next {
            token.push(byte);
            escape_next = false;
            index += 1;
            continue;
        }

        if in_single_quote {
            if byte == b'\'' {
                in_single_quote = false;
            } else {
                token.push(byte);
            }
            index += 1;
            continue;
        }

        if in_double_quote {
            match byte {
                b'"' => in_double_quote = false,
                b'\\' => escape_next = true,
                _ => token.push(byte),
            }
            index += 1;
            continue;
        }

        match byte {
            b' ' | b'\n' | b'\r' | b'\t' => {
                if let Some(reason) = flush_token(&mut token, &mut state) {
                    return Some(reason);
                }
            }
            b'\'' => in_single_quote = true,
            b'"' => in_double_quote = true,
            b';' => {
                if let Some(reason) = flush_token(&mut token, &mut state) {
                    return Some(reason);
                }
                state = SegmentState::SeekCommand;
            }
            b'|' => {
                if let Some(reason) = flush_token(&mut token, &mut state) {
                    return Some(reason);
                }
                if index + 1 < bytes.len() && bytes[index + 1] == b'|' {
                    index += 1;
                }
                state = SegmentState::SeekCommand;
            }
            b'&' => {
                if let Some(reason) = flush_token(&mut token, &mut state) {
                    return Some(reason);
                }
                if index + 1 < bytes.len() && bytes[index + 1] == b'&' {
                    index += 1;
                }
                state = SegmentState::SeekCommand;
            }
            b'\\' => {
                if index + 1 < bytes.len() {
                    index += 1;
                    token.push(bytes[index]);
                } else {
                    token.push(b'\\');
                }
            }
            _ => token.push(byte),
        }

        index += 1;
    }

    flush_token(&mut token, &mut state)
}

fn flush_token(token: &mut Vec<u8>, state: &mut SegmentState) -> Option<&'static str> {
    if token.is_empty() {
        return None;
    }

    let reason = process_token(token, state);
    token.clear();
    reason
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SegmentState {
    SeekCommand,
    SeekUvDecision,
    IgnoreSegment,
}

fn process_token(token: &[u8], state: &mut SegmentState) -> Option<&'static str> {
    match state {
        SegmentState::SeekCommand => process_command_token(token, state),
        SegmentState::SeekUvDecision => process_uv_token(token, state),
        SegmentState::IgnoreSegment => None,
    }
}

fn process_command_token(token: &[u8], state: &mut SegmentState) -> Option<&'static str> {
    if token.starts_with(b"-") || is_shell_assignment(token) {
        return None;
    }

    match classify_token(token) {
        TokenKind::Wrapper => None,
        TokenKind::Uv => {
            *state = SegmentState::SeekUvDecision;
            None
        }
        TokenKind::Uvx => {
            *state = SegmentState::IgnoreSegment;
            None
        }
        TokenKind::PythonLike | TokenKind::PipLike => Some(PYTHON_MESSAGE),
        TokenKind::Other => {
            *state = SegmentState::IgnoreSegment;
            None
        }
    }
}

fn process_uv_token(token: &[u8], state: &mut SegmentState) -> Option<&'static str> {
    if token.starts_with(b"-") || is_shell_assignment(token) {
        return None;
    }

    let name = normalized_program_name(token);
    if name == b"init" {
        return Some(UV_INIT_MESSAGE);
    }

    if is_known_safe_uv_subcommand(name) {
        *state = SegmentState::IgnoreSegment;
    }

    None
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

fn is_known_safe_uv_subcommand(token: &[u8]) -> bool {
    matches!(
        token,
        b"add"
            | b"auth"
            | b"build"
            | b"cache"
            | b"export"
            | b"help"
            | b"lock"
            | b"pip"
            | b"publish"
            | b"python"
            | b"remove"
            | b"run"
            | b"sync"
            | b"tool"
            | b"tree"
            | b"venv"
            | b"version"
    )
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

    #[test]
    fn blocks_python_command() {
        assert_eq!(evaluate_command("python -m pytest"), Some(PYTHON_MESSAGE));
        assert_eq!(
            evaluate_command(".venv/bin/python script.py"),
            Some(PYTHON_MESSAGE)
        );
        assert_eq!(
            evaluate_command("pip install requests"),
            Some(PYTHON_MESSAGE)
        );
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
        assert_eq!(evaluate_command("uv init"), Some(UV_INIT_MESSAGE));
        assert_eq!(
            evaluate_command("uv --directory repo init"),
            Some(UV_INIT_MESSAGE)
        );
    }

    #[test]
    fn avoids_argument_false_positives() {
        assert_eq!(evaluate_command("echo python"), None);
        assert_eq!(evaluate_command("printf '%s' python"), None);
        assert_eq!(evaluate_command("uv run echo init"), None);
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
