use std::borrow::Cow;
use std::env;
use std::fs;
use std::io::{self, Read};
use std::process;
use std::sync::OnceLock;
use std::time::Instant;

use serde_json::{json, Map, Value};
use smallvec::SmallVec;

const BINARY_NAME: &str = "enforce-tool-preferences-command";
const GREP_MESSAGE: &str = "Use rg (ripgrep) instead of grep in this project. Replace blocked grep commands with the least invasive exact rg rewrite when the flag mapping is clear. If a flag does not have a guaranteed direct rg translation, translate it manually instead of guessing.";
const PYTHON_MESSAGE: &str = "Use uv instead of bare Python or pip commands in this project. Replace the blocked command with 'uv run ...', 'uv add ...', 'uv add --dev ...', 'uv remove ...', or 'uv run --with ...' as appropriate.";
const UV_INIT_MESSAGE: &str = "Do not run 'uv init' in an existing project unless the user explicitly asks for project creation or conversion. Inspect the repo first and prefer 'uv run', 'uv add', 'uv sync', or 'uv run --with'. If project initialization is truly needed, use 'uv init --no-readme --no-workspace' to avoid overwriting existing files and git history.";
const BUN_MESSAGE: &str = "Use bun instead of npm or npx in this project. Replace blocked commands with 'bun install', 'bun add', 'bun remove', 'bun run', 'bunx', 'bun create', 'bun publish', 'bun update', or 'bun outdated' when the mapping is exact. If an npm or npx flag does not have a guaranteed Bun equivalent, translate it manually instead of guessing.";
const INSTALL_SH_SOURCE: &str = include_str!("../install.sh");
const SHARED_RULE_CATALOG_BEGIN: &str = "# BEGIN_SHARED_RULE_CATALOG";
const SHARED_RULE_CATALOG_END: &str = "# END_SHARED_RULE_CATALOG";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuleId {
    Ripgrep = 0,
    Uv = 1,
    Bun = 2,
}

const RULE_IDS: [RuleId; 3] = [RuleId::Ripgrep, RuleId::Uv, RuleId::Bun];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuleSpec {
    manifest_id: &'static str,
    guidance: &'static str,
}

const RULE_SPECS: [RuleSpec; 3] = [
    RuleSpec {
        manifest_id: "rg",
        guidance: GREP_MESSAGE,
    },
    RuleSpec {
        manifest_id: "uv",
        guidance: PYTHON_MESSAGE,
    },
    RuleSpec {
        manifest_id: "bun",
        guidance: BUN_MESSAGE,
    },
];

fn rule_spec(rule: RuleId) -> &'static RuleSpec {
    &RULE_SPECS[rule as usize]
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RuleCatalogEntry {
    cli_name: &'static str,
    aliases: SmallVec<[&'static str; 4]>,
    description: &'static str,
    prerequisites: SmallVec<[&'static str; 4]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuleSet(u32);

impl RuleSet {
    const RIPGREP: u32 = 1 << 0;
    const UV: u32 = 1 << 1;
    const BUN: u32 = 1 << 2;
    const ALL: u32 = Self::RIPGREP | Self::UV | Self::BUN;

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
        let mut mask = 0u32;

        for item in value.split(',') {
            let name = item.trim();
            if name.is_empty() {
                continue;
            }

            let Some(rule) = rule_id_for_cli_name(name) else {
                return Err(format!(
                    "unknown rule '{name}'. Expected --rules <rule[,rule...]> using supported rule ids: {}",
                    supported_rule_names()
                ));
            };

            mask |= rule_mask(rule);
        }

        if mask == 0 {
            return Err(format!(
                "at least one rule must be enabled; use --rules <rule[,rule...]> with one or more of: {}",
                supported_rule_names()
            ));
        }

        Ok(Self(mask))
    }

    fn cli_value(self) -> String {
        let mut names = Vec::new();

        for rule in RULE_IDS {
            if self.contains(rule) {
                names.push(rule_catalog_entry(rule).cli_name);
            }
        }

        names.join(",")
    }
}

fn rule_mask(rule: RuleId) -> u32 {
    match rule {
        RuleId::Ripgrep => RuleSet::RIPGREP,
        RuleId::Uv => RuleSet::UV,
        RuleId::Bun => RuleSet::BUN,
    }
}

fn shared_rule_catalog() -> &'static [RuleCatalogEntry] {
    static RULE_CATALOG: OnceLock<Vec<RuleCatalogEntry>> = OnceLock::new();
    RULE_CATALOG
        .get_or_init(parse_shared_rule_catalog)
        .as_slice()
}

fn parse_shared_rule_catalog() -> Vec<RuleCatalogEntry> {
    extract_shared_rule_catalog(INSTALL_SH_SOURCE)
        .lines()
        .filter_map(parse_rule_catalog_entry)
        .collect()
}

fn extract_shared_rule_catalog(source: &'static str) -> &'static str {
    let start = source
        .find(SHARED_RULE_CATALOG_BEGIN)
        .unwrap_or_else(|| panic!("missing {SHARED_RULE_CATALOG_BEGIN} in install.sh"));
    let after_start = &source[start + SHARED_RULE_CATALOG_BEGIN.len()..];
    let after_start = after_start
        .strip_prefix('\n')
        .unwrap_or_else(|| panic!("expected newline after {SHARED_RULE_CATALOG_BEGIN}"));
    let end = after_start
        .find(SHARED_RULE_CATALOG_END)
        .unwrap_or_else(|| panic!("missing {SHARED_RULE_CATALOG_END} in install.sh"));
    after_start[..end].trim()
}

fn parse_rule_catalog_entry(line: &'static str) -> Option<RuleCatalogEntry> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    let mut fields = line.split('\t');
    let cli_name = fields
        .next()
        .unwrap_or_else(|| panic!("invalid shared rule catalog row: '{line}'"));
    let aliases = fields
        .next()
        .unwrap_or_else(|| panic!("missing aliases field in shared rule catalog row: '{line}'"));
    let description = fields.next().unwrap_or_else(|| {
        panic!("missing description field in shared rule catalog row: '{line}'")
    });
    let prerequisites = fields.next().unwrap_or_else(|| {
        panic!("missing prerequisites field in shared rule catalog row: '{line}'")
    });

    if fields.next().is_some() {
        panic!("too many fields in shared rule catalog row: '{line}'");
    }

    Some(RuleCatalogEntry {
        cli_name,
        aliases: parse_catalog_list(aliases),
        description,
        prerequisites: parse_catalog_list(prerequisites),
    })
}

fn parse_catalog_list(field: &'static str) -> SmallVec<[&'static str; 4]> {
    if field.is_empty() || field == "-" {
        return SmallVec::new();
    }

    field
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect()
}

fn rule_catalog_entry(rule: RuleId) -> &'static RuleCatalogEntry {
    let manifest_id = rule_spec(rule).manifest_id;
    shared_rule_catalog()
        .iter()
        .find(|entry| entry.cli_name == manifest_id)
        .unwrap_or_else(|| panic!("missing shared rule catalog entry for {manifest_id}"))
}

fn rule_id_for_cli_name(name: &str) -> Option<RuleId> {
    RULE_IDS.iter().copied().find(|rule| {
        let entry = rule_catalog_entry(*rule);
        entry.cli_name == name || entry.aliases.iter().any(|alias| *alias == name)
    })
}

fn supported_rule_names() -> String {
    shared_rule_catalog()
        .iter()
        .map(|entry| entry.cli_name)
        .collect::<Vec<_>>()
        .join(", ")
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
        Mode::ListRules => {
            print_rule_catalog();
            Ok(0)
        }
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
    ListRules,
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
        let mut list_rules = false;
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
                "--list-rules" => {
                    list_rules = true;
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

        if list_rules {
            if input.is_some()
                || benchmark_command.is_some()
                || configure_claude_hook.is_some()
                || configure_gemini_hook.is_some()
                || configure_codex_hook.is_some()
            {
                return Err("--list-rules cannot be combined with another mode".to_string());
            }

            return Ok(Self {
                mode: Mode::ListRules,
            });
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
    let rule_csv_example = RuleSet::all().cli_value();

    println!(
        "Usage:\n  {0} --list-rules\n  {0} --command \"grep -rn pattern .\" [--claude-json] [--rules <rule[,rule...]>]\n  {0} --stdin-command [--claude-json] [--rules <rule[,rule...]>]\n  {0} --claude-hook-json [--rules <rule[,rule...]>]\n  {0} --codex-hook-json [--rules <rule[,rule...]>]\n  {0} --gemini-hook-json [--rules <rule[,rule...]>]\n  {0} --benchmark-command \"grep -rn pattern .\" [--iterations 1000000] [--rules <rule[,rule...]>]\n  {0} --configure-claude-hook <settings-path> <binary-name> [--rules <rule[,rule...]>]\n  {0} --configure-gemini-hook <settings-path> <binary-name> [--rules <rule[,rule...]>]\n  {0} --configure-codex-hook <hooks-path> <binary-name> [--rules <rule[,rule...]>]\n\nSupported rule ids: {1}\nExample exact set: --rules {2}",
        BINARY_NAME,
        supported_rule_names(),
        rule_csv_example,
    );
}

fn print_rule_catalog() {
    println!("Supported rule ids:");

    for entry in shared_rule_catalog() {
        println!("  {}", entry.cli_name);
        println!("    Description: {}", entry.description);
        println!("    Aliases: {}", format_catalog_values(&entry.aliases));
        println!(
            "    Requires: {}",
            format_catalog_values(&entry.prerequisites)
        );
    }
}

fn format_catalog_values(values: &[&str]) -> String {
    if values.is_empty() {
        return "<none>".to_string();
    }

    values.join(", ")
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
    if let Ok(decision) = try_evaluate_simple_command(command, rules) {
        return decision;
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

fn try_evaluate_simple_command(command: &str, rules: RuleSet) -> Result<Option<BlockDecision>, ()> {
    let mut tokens = TokenBuffer::new();
    let bytes = command.as_bytes();
    let mut token_start = None;

    for (index, byte) in bytes.iter().copied().enumerate() {
        match byte {
            b' ' => {
                if let Some(start) = token_start.take() {
                    let raw = &command[start..index];
                    tokens.push(ParsedToken {
                        raw,
                        value: Cow::Borrowed(raw),
                    });
                }
            }
            b'\'' | b'"' | b'\\' | b';' | b'|' | b'&' | b'\n' | b'\r' | b'\t' => {
                return Err(());
            }
            _ => {
                token_start.get_or_insert(index);
            }
        }
    }

    if let Some(start) = token_start {
        let raw = &command[start..];
        tokens.push(ParsedToken {
            raw,
            value: Cow::Borrowed(raw),
        });
    }

    Ok(evaluate_segment(&tokens, rules))
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
            TokenKind::Allowed(AllowedCommand::Rg)
            | TokenKind::Allowed(AllowedCommand::Uvx)
            | TokenKind::Allowed(AllowedCommand::Bun)
            | TokenKind::Allowed(AllowedCommand::Bunx) => {
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
            TokenKind::Blocked(BlockedCommand::Npm) => {
                if !rules.contains(RuleId::Bun) {
                    return None;
                }
                return Some(build_npm_decision(tokens, index));
            }
            TokenKind::Blocked(BlockedCommand::Npx) => {
                if !rules.contains(RuleId::Bun) {
                    return None;
                }
                return Some(build_npx_decision(tokens, index));
            }
            TokenKind::Other => return None,
        }
    }

    None
}

fn build_python_decision(tokens: &[ParsedToken<'_>], command_index: usize) -> BlockDecision {
    let suggestion = insert_before_command(tokens, command_index, &["uv", "run"]);
    BlockDecision::new(into_exact_suggestion_message(
        rule_spec(RuleId::Uv).guidance,
        suggestion,
    ))
}

fn build_pip_decision(tokens: &[ParsedToken<'_>], command_index: usize) -> BlockDecision {
    BlockDecision::new(render_pip_decision(tokens, command_index))
}

fn build_npm_decision(tokens: &[ParsedToken<'_>], command_index: usize) -> BlockDecision {
    BlockDecision::new(render_npm_decision(tokens, command_index))
}

fn build_npx_decision(tokens: &[ParsedToken<'_>], command_index: usize) -> BlockDecision {
    match rewrite_npx_to_bunx(tokens, command_index) {
        BunRewrite::Exact(suggestion) => BlockDecision::new(into_exact_suggestion_message(
            rule_spec(RuleId::Bun).guidance,
            suggestion,
        )),
        BunRewrite::NeedsManualTranslation { items, note } => BlockDecision::new(
            format_bun_manual_translation_message(rule_spec(RuleId::Bun).guidance, &items, note),
        ),
    }
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

fn render_npm_decision(tokens: &[ParsedToken<'_>], command_index: usize) -> String {
    let Some(subcommand_token) = tokens.get(command_index + 1) else {
        return format_bun_manual_translation_message(
            rule_spec(RuleId::Bun).guidance,
            &[],
            "Choose the Bun subcommand manually after checking `bun --help` instead of guessing.",
        );
    };

    let subcommand = subcommand_token.value.as_ref();
    if subcommand.starts_with('-') {
        return format_bun_manual_translation_message(
            rule_spec(RuleId::Bun).guidance,
            &[subcommand_token.raw.to_string()],
            "Translate npm flags manually after checking Bun's CLI docs instead of assuming they map one-to-one.",
        );
    }

    if subcommand.eq_ignore_ascii_case("install") || subcommand.eq_ignore_ascii_case("i") {
        return match rewrite_npm_install_to_bun(tokens, command_index) {
            BunRewrite::Exact(suggestion) => {
                into_exact_suggestion_message(rule_spec(RuleId::Bun).guidance, suggestion)
            }
            BunRewrite::NeedsManualTranslation { items, note } => {
                format_bun_manual_translation_message(rule_spec(RuleId::Bun).guidance, &items, note)
            }
        };
    }

    if matches_npm_remove_subcommand(subcommand) {
        return match rewrite_npm_remove_to_bun(tokens, command_index) {
            BunRewrite::Exact(suggestion) => {
                into_exact_suggestion_message(rule_spec(RuleId::Bun).guidance, suggestion)
            }
            BunRewrite::NeedsManualTranslation { items, note } => {
                format_bun_manual_translation_message(rule_spec(RuleId::Bun).guidance, &items, note)
            }
        };
    }

    if subcommand.eq_ignore_ascii_case("run") || subcommand.eq_ignore_ascii_case("run-script") {
        return match rewrite_npm_run_to_bun(tokens, command_index) {
            BunRewrite::Exact(suggestion) => {
                into_exact_suggestion_message(rule_spec(RuleId::Bun).guidance, suggestion)
            }
            BunRewrite::NeedsManualTranslation { items, note } => {
                format_bun_manual_translation_message(rule_spec(RuleId::Bun).guidance, &items, note)
            }
        };
    }

    if subcommand.eq_ignore_ascii_case("exec") {
        return match rewrite_npm_exec_to_bun(tokens, command_index) {
            BunRewrite::Exact(suggestion) => {
                into_exact_suggestion_message(rule_spec(RuleId::Bun).guidance, suggestion)
            }
            BunRewrite::NeedsManualTranslation { items, note } => {
                format_bun_manual_translation_message(rule_spec(RuleId::Bun).guidance, &items, note)
            }
        };
    }

    if subcommand.eq_ignore_ascii_case("create") {
        return match rewrite_npm_create_to_bun(tokens, command_index) {
            BunRewrite::Exact(suggestion) => {
                into_exact_suggestion_message(rule_spec(RuleId::Bun).guidance, suggestion)
            }
            BunRewrite::NeedsManualTranslation { items, note } => {
                format_bun_manual_translation_message(rule_spec(RuleId::Bun).guidance, &items, note)
            }
        };
    }

    if subcommand.eq_ignore_ascii_case("publish") {
        return match rewrite_npm_publish_to_bun(tokens, command_index) {
            BunRewrite::Exact(suggestion) => {
                into_exact_suggestion_message(rule_spec(RuleId::Bun).guidance, suggestion)
            }
            BunRewrite::NeedsManualTranslation { items, note } => {
                format_bun_manual_translation_message(rule_spec(RuleId::Bun).guidance, &items, note)
            }
        };
    }

    if subcommand.eq_ignore_ascii_case("update") || subcommand.eq_ignore_ascii_case("upgrade") {
        return match rewrite_npm_update_to_bun(tokens, command_index) {
            BunRewrite::Exact(suggestion) => {
                into_exact_suggestion_message(rule_spec(RuleId::Bun).guidance, suggestion)
            }
            BunRewrite::NeedsManualTranslation { items, note } => {
                format_bun_manual_translation_message(rule_spec(RuleId::Bun).guidance, &items, note)
            }
        };
    }

    if subcommand.eq_ignore_ascii_case("outdated") {
        return match rewrite_npm_outdated_to_bun(tokens, command_index) {
            BunRewrite::Exact(suggestion) => {
                into_exact_suggestion_message(rule_spec(RuleId::Bun).guidance, suggestion)
            }
            BunRewrite::NeedsManualTranslation { items, note } => {
                format_bun_manual_translation_message(rule_spec(RuleId::Bun).guidance, &items, note)
            }
        };
    }

    if subcommand.eq_ignore_ascii_case("pack") {
        return match rewrite_npm_pack_to_bun(tokens, command_index) {
            BunRewrite::Exact(suggestion) => {
                into_exact_suggestion_message(rule_spec(RuleId::Bun).guidance, suggestion)
            }
            BunRewrite::NeedsManualTranslation { items, note } => {
                format_bun_manual_translation_message(rule_spec(RuleId::Bun).guidance, &items, note)
            }
        };
    }

    if subcommand.eq_ignore_ascii_case("ci") {
        return match rewrite_npm_ci_to_bun(tokens, command_index) {
            BunRewrite::Exact(suggestion) => {
                into_exact_suggestion_message(rule_spec(RuleId::Bun).guidance, suggestion)
            }
            BunRewrite::NeedsManualTranslation { items, note } => {
                format_bun_manual_translation_message(rule_spec(RuleId::Bun).guidance, &items, note)
            }
        };
    }

    if matches_npm_lifecycle_subcommand(subcommand) {
        let script = normalize_npm_lifecycle_name(subcommand);
        return match rewrite_npm_lifecycle_to_bun(tokens, command_index, script) {
            BunRewrite::Exact(suggestion) => {
                into_exact_suggestion_message(rule_spec(RuleId::Bun).guidance, suggestion)
            }
            BunRewrite::NeedsManualTranslation { items, note } => {
                format_bun_manual_translation_message(rule_spec(RuleId::Bun).guidance, &items, note)
            }
        };
    }

    if subcommand.eq_ignore_ascii_case("init") {
        return match rewrite_npm_init_to_bun(tokens, command_index) {
            BunRewrite::Exact(suggestion) => {
                into_exact_suggestion_message(rule_spec(RuleId::Bun).guidance, suggestion)
            }
            BunRewrite::NeedsManualTranslation { items, note } => {
                format_bun_manual_translation_message(rule_spec(RuleId::Bun).guidance, &items, note)
            }
        };
    }

    if subcommand.eq_ignore_ascii_case("link") || subcommand.eq_ignore_ascii_case("ln") {
        return match rewrite_npm_link_to_bun(tokens, command_index) {
            BunRewrite::Exact(suggestion) => {
                into_exact_suggestion_message(rule_spec(RuleId::Bun).guidance, suggestion)
            }
            BunRewrite::NeedsManualTranslation { items, note } => {
                format_bun_manual_translation_message(rule_spec(RuleId::Bun).guidance, &items, note)
            }
        };
    }

    format_bun_manual_translation_message(
        rule_spec(RuleId::Bun).guidance,
        &[format!("subcommand: {}", subcommand_token.raw)],
        "Translate this npm workflow manually after checking Bun's docs instead of assuming the CLIs behave the same.",
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NpmInstallMode {
    Default,
    Dev,
    Optional,
    Peer,
}

enum BunRewrite {
    Exact(String),
    NeedsManualTranslation {
        items: Vec<String>,
        note: &'static str,
    },
}

fn matches_npm_remove_subcommand(subcommand: &str) -> bool {
    matches!(
        subcommand,
        value
            if value.eq_ignore_ascii_case("uninstall")
                || value.eq_ignore_ascii_case("remove")
                || value.eq_ignore_ascii_case("rm")
                || value.eq_ignore_ascii_case("r")
                || value.eq_ignore_ascii_case("un")
    )
}

fn rewrite_npx_to_bunx(tokens: &[ParsedToken<'_>], command_index: usize) -> BunRewrite {
    let args = &tokens[command_index + 1..];
    let mut saw_target = false;
    let mut uncertain_items = Vec::new();

    for token in args {
        let value = token.value.as_ref();
        if saw_target {
            continue;
        }

        if value.starts_with('-') || value == "--" {
            uncertain_items.push(token.raw.to_string());
            continue;
        }

        saw_target = true;
    }

    if !uncertain_items.is_empty() {
        return BunRewrite::NeedsManualTranslation {
            items: uncertain_items,
            note: "Translate npx-only flags manually after checking `bunx --help` instead of assuming they map directly.",
        };
    }

    if !saw_target {
        return BunRewrite::NeedsManualTranslation {
            items: Vec::new(),
            note: "Provide the Bun package or executable manually after checking `bunx --help`.",
        };
    }

    BunRewrite::Exact(replace_command(tokens, command_index, 1, &["bunx"]))
}

fn rewrite_npm_install_to_bun(tokens: &[ParsedToken<'_>], command_index: usize) -> BunRewrite {
    let args = &tokens[command_index + 2..];
    let mut install_mode = NpmInstallMode::Default;
    let mut has_mode_flag = false;
    let mut exact = false;
    let mut global = false;
    let mut package_args = Vec::new();
    let mut passthrough_flags = Vec::new();
    let mut uncertain_items = Vec::new();

    for token in args {
        let value = token.value.as_ref();
        if value.starts_with('-') {
            match value {
                "-D" | "--save-dev" => {
                    if has_mode_flag && install_mode != NpmInstallMode::Dev {
                        uncertain_items.push(token.raw.to_string());
                    } else {
                        install_mode = NpmInstallMode::Dev;
                        has_mode_flag = true;
                    }
                }
                "-O" | "--save-optional" => {
                    if has_mode_flag && install_mode != NpmInstallMode::Optional {
                        uncertain_items.push(token.raw.to_string());
                    } else {
                        install_mode = NpmInstallMode::Optional;
                        has_mode_flag = true;
                    }
                }
                "--save-peer" => {
                    if has_mode_flag && install_mode != NpmInstallMode::Peer {
                        uncertain_items.push(token.raw.to_string());
                    } else {
                        install_mode = NpmInstallMode::Peer;
                        has_mode_flag = true;
                    }
                }
                "-E" | "--save-exact" => {
                    exact = true;
                }
                "-g" | "--global" => {
                    global = true;
                }
                "-S" | "--save" | "--save-prod" => {}
                "--production" | "--frozen-lockfile" | "--dry-run" => {
                    passthrough_flags.push(token.raw.to_string());
                }
                _ => uncertain_items.push(token.raw.to_string()),
            }
            continue;
        }

        package_args.push(token.raw);
    }

    if !uncertain_items.is_empty() {
        return BunRewrite::NeedsManualTranslation {
            items: uncertain_items,
            note: "Translate npm install flags manually after checking Bun's package-manager docs instead of guessing.",
        };
    }

    if package_args.is_empty() {
        if global || install_mode != NpmInstallMode::Default || exact {
            let mut items = Vec::new();
            if global {
                items.push("--global without package arguments".to_string());
            }
            if install_mode != NpmInstallMode::Default {
                items.push("dependency save-mode flag without package arguments".to_string());
            }
            if exact {
                items.push("--save-exact without package arguments".to_string());
            }
            return BunRewrite::NeedsManualTranslation {
                items,
                note: "Choose the Bun install form manually after checking whether you mean to add packages or install the existing lockfile.",
            };
        }

        return BunRewrite::Exact(replace_command(
            tokens,
            command_index,
            2,
            &["bun", "install"],
        ));
    }

    if global {
        if install_mode != NpmInstallMode::Default || exact {
            let mut items = Vec::new();
            if install_mode != NpmInstallMode::Default {
                items.push("dependency save-mode flag combined with --global".to_string());
            }
            if exact {
                items.push("--save-exact combined with --global".to_string());
            }
            return BunRewrite::NeedsManualTranslation {
                items,
                note: "Translate this global install manually after checking Bun's global install docs instead of assuming package metadata flags still apply.",
            };
        }

        return BunRewrite::Exact(replace_command(
            tokens,
            command_index,
            2,
            &["bun", "install"],
        ));
    }

    let mut suggestion = String::new();
    let mut needs_space = false;
    for token in &tokens[..command_index] {
        push_command_part(&mut suggestion, token.raw, &mut needs_space);
    }
    push_command_part(&mut suggestion, "bun", &mut needs_space);
    push_command_part(&mut suggestion, "add", &mut needs_space);

    match install_mode {
        NpmInstallMode::Default => {}
        NpmInstallMode::Dev => push_command_part(&mut suggestion, "-d", &mut needs_space),
        NpmInstallMode::Optional => {
            push_command_part(&mut suggestion, "--optional", &mut needs_space)
        }
        NpmInstallMode::Peer => push_command_part(&mut suggestion, "--peer", &mut needs_space),
    }

    if exact {
        push_command_part(&mut suggestion, "-E", &mut needs_space);
    }

    for flag in &passthrough_flags {
        push_command_part(&mut suggestion, flag, &mut needs_space);
    }

    for token in package_args {
        push_command_part(&mut suggestion, token, &mut needs_space);
    }

    BunRewrite::Exact(suggestion)
}

fn rewrite_npm_remove_to_bun(tokens: &[ParsedToken<'_>], command_index: usize) -> BunRewrite {
    let args = &tokens[command_index + 2..];
    let mut package_count = 0usize;
    let mut uncertain_items = Vec::new();

    for token in args {
        let value = token.value.as_ref();
        if value.starts_with('-') {
            if !matches!(value, "-g" | "--global") {
                uncertain_items.push(token.raw.to_string());
            }
            continue;
        }

        package_count += 1;
    }

    if !uncertain_items.is_empty() {
        return BunRewrite::NeedsManualTranslation {
            items: uncertain_items,
            note: "Translate npm remove flags manually after checking Bun's remove docs instead of guessing.",
        };
    }

    if package_count == 0 {
        return BunRewrite::NeedsManualTranslation {
            items: Vec::new(),
            note: "Choose the Bun remove target manually instead of running a package-removal command without arguments.",
        };
    }

    BunRewrite::Exact(replace_command(
        tokens,
        command_index,
        2,
        &["bun", "remove"],
    ))
}

fn rewrite_npm_run_to_bun(tokens: &[ParsedToken<'_>], command_index: usize) -> BunRewrite {
    let args = &tokens[command_index + 2..];
    let mut saw_script = false;
    let mut uncertain_items = Vec::new();

    for token in args {
        let value = token.value.as_ref();
        if saw_script {
            continue;
        }

        if value == "--" || value.starts_with('-') {
            uncertain_items.push(token.raw.to_string());
            continue;
        }

        saw_script = true;
    }

    if !uncertain_items.is_empty() {
        return BunRewrite::NeedsManualTranslation {
            items: uncertain_items,
            note: "Translate npm run flags manually after checking Bun's script runner docs instead of assuming the same CLI shape.",
        };
    }

    BunRewrite::Exact(replace_command(tokens, command_index, 2, &["bun", "run"]))
}

fn rewrite_npm_exec_to_bun(tokens: &[ParsedToken<'_>], command_index: usize) -> BunRewrite {
    let args = &tokens[command_index + 2..];
    let mut target = None::<&str>;
    let mut uncertain_items = Vec::new();

    for token in args {
        let value = token.value.as_ref();
        if target.is_some() {
            continue;
        }

        if value == "--" || value.starts_with('-') {
            uncertain_items.push(token.raw.to_string());
            continue;
        }

        if !is_simple_bun_exec_target(value) {
            uncertain_items.push(token.raw.to_string());
            continue;
        }

        target = Some(token.raw);
    }

    if !uncertain_items.is_empty() {
        return BunRewrite::NeedsManualTranslation {
            items: uncertain_items,
            note: "Translate npm exec options manually after checking whether the Bun equivalent should be `bun` or `bunx`.",
        };
    }

    if target.is_none() {
        return BunRewrite::NeedsManualTranslation {
            items: Vec::new(),
            note: "Choose the Bun executable manually after checking whether you mean a local binary, package script, or one-off package execution.",
        };
    }

    BunRewrite::Exact(replace_command(tokens, command_index, 2, &["bun"]))
}

fn rewrite_npm_create_to_bun(tokens: &[ParsedToken<'_>], command_index: usize) -> BunRewrite {
    let args = &tokens[command_index + 2..];
    let mut uncertain_items = Vec::new();

    for token in args {
        let value = token.value.as_ref();
        if value == "--" || value.starts_with('-') {
            uncertain_items.push(token.raw.to_string());
        } else {
            break;
        }
    }

    if !uncertain_items.is_empty() {
        return BunRewrite::NeedsManualTranslation {
            items: uncertain_items,
            note: "Translate npm create flags manually after checking `bun create --help` instead of assuming the scaffolder flags line up exactly.",
        };
    }

    BunRewrite::Exact(replace_command(
        tokens,
        command_index,
        2,
        &["bun", "create"],
    ))
}

fn rewrite_npm_publish_to_bun(tokens: &[ParsedToken<'_>], command_index: usize) -> BunRewrite {
    let args = &tokens[command_index + 2..];
    let uncertain_items = args
        .iter()
        .filter(|token| token.value.starts_with('-'))
        .map(|token| token.raw.to_string())
        .collect::<Vec<_>>();

    if !uncertain_items.is_empty() {
        return BunRewrite::NeedsManualTranslation {
            items: uncertain_items,
            note: "Translate npm publish flags manually after checking Bun's publish docs instead of assuming they behave the same.",
        };
    }

    BunRewrite::Exact(replace_command(
        tokens,
        command_index,
        2,
        &["bun", "publish"],
    ))
}

fn rewrite_npm_update_to_bun(tokens: &[ParsedToken<'_>], command_index: usize) -> BunRewrite {
    let args = &tokens[command_index + 2..];
    let uncertain_items = args
        .iter()
        .filter(|token| token.value.starts_with('-') && !matches!(token.value.as_ref(), "--latest"))
        .map(|token| token.raw.to_string())
        .collect::<Vec<_>>();

    if !uncertain_items.is_empty() {
        return BunRewrite::NeedsManualTranslation {
            items: uncertain_items,
            note: "Translate npm update flags manually after checking Bun's update docs instead of guessing.",
        };
    }

    BunRewrite::Exact(replace_command(
        tokens,
        command_index,
        2,
        &["bun", "update"],
    ))
}

fn rewrite_npm_outdated_to_bun(tokens: &[ParsedToken<'_>], command_index: usize) -> BunRewrite {
    if tokens.len() > command_index + 2 {
        let items = tokens[command_index + 2..]
            .iter()
            .map(|token| token.raw.to_string())
            .collect::<Vec<_>>();
        return BunRewrite::NeedsManualTranslation {
            items,
            note: "Translate npm outdated arguments manually after checking Bun's outdated command instead of assuming it accepts the same filters.",
        };
    }

    BunRewrite::Exact(replace_command(
        tokens,
        command_index,
        2,
        &["bun", "outdated"],
    ))
}

fn rewrite_npm_pack_to_bun(tokens: &[ParsedToken<'_>], command_index: usize) -> BunRewrite {
    if tokens.len() > command_index + 2 {
        let items = tokens[command_index + 2..]
            .iter()
            .map(|token| token.raw.to_string())
            .collect::<Vec<_>>();
        return BunRewrite::NeedsManualTranslation {
            items,
            note: "Translate npm pack arguments manually after checking `bun pm pack` instead of assuming it accepts the same inputs.",
        };
    }

    BunRewrite::Exact(replace_command(
        tokens,
        command_index,
        2,
        &["bun", "pm", "pack"],
    ))
}

fn rewrite_npm_ci_to_bun(tokens: &[ParsedToken<'_>], command_index: usize) -> BunRewrite {
    let args = &tokens[command_index + 2..];
    let uncertain_items = args
        .iter()
        .filter(|token| token.value.starts_with('-'))
        .map(|token| token.raw.to_string())
        .collect::<Vec<_>>();

    if !uncertain_items.is_empty() {
        return BunRewrite::NeedsManualTranslation {
            items: uncertain_items,
            note: "Translate npm ci flags manually after checking Bun's install docs instead of guessing.",
        };
    }

    BunRewrite::Exact(replace_command(
        tokens,
        command_index,
        2,
        &["bun", "install", "--frozen-lockfile"],
    ))
}

fn matches_npm_lifecycle_subcommand(subcommand: &str) -> bool {
    subcommand.eq_ignore_ascii_case("test")
        || subcommand.eq_ignore_ascii_case("t")
        || subcommand.eq_ignore_ascii_case("tst")
        || subcommand.eq_ignore_ascii_case("start")
        || subcommand.eq_ignore_ascii_case("stop")
        || subcommand.eq_ignore_ascii_case("restart")
}

fn normalize_npm_lifecycle_name(subcommand: &str) -> &'static str {
    if subcommand.eq_ignore_ascii_case("test")
        || subcommand.eq_ignore_ascii_case("t")
        || subcommand.eq_ignore_ascii_case("tst")
    {
        "test"
    } else if subcommand.eq_ignore_ascii_case("start") {
        "start"
    } else if subcommand.eq_ignore_ascii_case("stop") {
        "stop"
    } else {
        "restart"
    }
}

fn rewrite_npm_lifecycle_to_bun(
    tokens: &[ParsedToken<'_>],
    command_index: usize,
    script: &'static str,
) -> BunRewrite {
    let args = &tokens[command_index + 2..];
    let mut uncertain_items = Vec::new();

    for token in args {
        let value = token.value.as_ref();
        if value == "--" {
            break;
        }
        if value.starts_with('-') {
            uncertain_items.push(token.raw.to_string());
        }
    }

    if !uncertain_items.is_empty() {
        return BunRewrite::NeedsManualTranslation {
            items: uncertain_items,
            note: "Translate npm lifecycle flags manually after checking Bun's script runner docs instead of assuming the same CLI shape.",
        };
    }

    BunRewrite::Exact(replace_command(
        tokens,
        command_index,
        2,
        &["bun", "run", script],
    ))
}

fn rewrite_npm_init_to_bun(tokens: &[ParsedToken<'_>], command_index: usize) -> BunRewrite {
    let args = &tokens[command_index + 2..];

    if args.is_empty() {
        return BunRewrite::Exact(replace_command(
            tokens,
            command_index,
            2,
            &["bun", "init"],
        ));
    }

    let all_yes = args
        .iter()
        .all(|t| matches!(t.value.as_ref(), "-y" | "--yes"));
    if all_yes {
        return BunRewrite::Exact(replace_command(
            tokens,
            command_index,
            2 + args.len(),
            &["bun", "init"],
        ));
    }

    let items = args.iter().map(|t| t.raw.to_string()).collect::<Vec<_>>();
    BunRewrite::NeedsManualTranslation {
        items,
        note: "Translate npm init arguments manually. `bun init` creates a new project; for template scaffolding use `bun create` instead.",
    }
}

fn rewrite_npm_link_to_bun(tokens: &[ParsedToken<'_>], command_index: usize) -> BunRewrite {
    let args = &tokens[command_index + 2..];
    let uncertain_items = args
        .iter()
        .filter(|token| token.value.starts_with('-'))
        .map(|token| token.raw.to_string())
        .collect::<Vec<_>>();

    if !uncertain_items.is_empty() {
        return BunRewrite::NeedsManualTranslation {
            items: uncertain_items,
            note: "Translate npm link flags manually after checking `bun link --help` instead of guessing.",
        };
    }

    BunRewrite::Exact(replace_command(
        tokens,
        command_index,
        2,
        &["bun", "link"],
    ))
}

fn format_bun_manual_translation_message(base: &str, items: &[String], note: &str) -> String {
    let mut message = String::from(base);

    if !items.is_empty() {
        message
            .push_str("\nFlags or arguments requiring manual translation before switching to bun:");
        for item in items {
            message.push_str("\n  ");
            message.push_str(item);
        }
    }

    message.push('\n');
    message.push_str(note);
    message
}

fn is_simple_bun_exec_target(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(
            |byte| matches!(byte, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'_' | b'-'),
        )
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

fn into_exact_suggestion_message(base: &str, mut suggestion: String) -> String {
    const EXACT_SUGGESTION_PREFIX: &str = "\nSuggested replacement:\n  ";

    suggestion.reserve(base.len() + EXACT_SUGGESTION_PREFIX.len());
    suggestion.insert_str(0, EXACT_SUGGESTION_PREFIX);
    suggestion.insert_str(0, base);
    suggestion
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
        GrepRewrite::Exact(suggestion) => BlockDecision::new(into_exact_suggestion_message(
            rule_spec(RuleId::Ripgrep).guidance,
            suggestion,
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
    Bun,
    Bunx,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BlockedCommand {
    Grep(GrepKind),
    Python,
    Pip,
    Npm,
    Npx,
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
        b"bun" => TokenKind::Allowed(AllowedCommand::Bun),
        b"bunx" => TokenKind::Allowed(AllowedCommand::Bunx),
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
        b"npm" => TokenKind::Blocked(BlockedCommand::Npm),
        b"npx" => TokenKind::Blocked(BlockedCommand::Npx),
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
    fn suggests_bun_rewrites() {
        let install = decision_message("npm install");
        assert!(install.contains(BUN_MESSAGE));
        assert!(install.contains("bun install"));

        let add = decision_message("npm install react");
        assert!(add.contains("bun add react"));

        let add_dev = decision_message("npm install --save-dev typescript");
        assert!(add_dev.contains("bun add -d typescript"));

        let remove = decision_message("npm uninstall react");
        assert!(remove.contains("bun remove react"));

        let run = decision_message("npm run dev");
        assert!(run.contains("bun run dev"));

        let exec = decision_message("npm exec vite -- --host");
        assert!(exec.contains("bun vite -- --host"));

        let create = decision_message("npm create vite@latest app");
        assert!(create.contains("bun create vite@latest app"));

        let publish = decision_message("npm publish dist");
        assert!(publish.contains("bun publish dist"));

        let update = decision_message("npm update --latest vite");
        assert!(update.contains("bun update --latest vite"));

        let pack = decision_message("npm pack");
        assert!(pack.contains("bun pm pack"));

        let npx = decision_message("npx prettier .");
        assert!(npx.contains("bunx prettier ."));

        let ci = decision_message("npm ci");
        assert!(ci.contains("bun install --frozen-lockfile"));

        let test = decision_message("npm test");
        assert!(test.contains("bun run test"));

        let t = decision_message("npm t");
        assert!(t.contains("bun run test"));

        let start = decision_message("npm start");
        assert!(start.contains("bun run start"));

        let stop = decision_message("npm stop");
        assert!(stop.contains("bun run stop"));

        let restart = decision_message("npm restart");
        assert!(restart.contains("bun run restart"));

        let init = decision_message("npm init");
        assert!(init.contains("bun init"));

        let init_y = decision_message("npm init -y");
        assert!(init_y.contains("bun init"));
        assert!(!init_y.contains("-y"));

        let link = decision_message("npm link");
        assert!(link.contains("bun link"));

        let link_pkg = decision_message("npm link my-package");
        assert!(link_pkg.contains("bun link my-package"));

        let add_dry_run = decision_message("npm install --dry-run react");
        assert!(add_dry_run.contains("bun add --dry-run react"));
    }

    #[test]
    fn requires_manual_translation_for_uncertain_bun_mappings() {

        let npx = decision_message("npx --yes create-vite");
        assert!(npx.contains("manual translation"));
        assert!(npx.contains("\n  --yes"));

        let exec = decision_message("npm exec @scope/tool");
        assert!(exec.contains("@scope/tool"));
        assert!(exec.contains("whether the Bun equivalent should be `bun` or `bunx`"));

        let init_template = decision_message("npm init react-app my-app");
        assert!(init_template.contains("bun create"));

        let link_flags = decision_message("npm link --save");
        assert!(link_flags.contains("manual translation"));

        let test_flags = decision_message("npm test --ignore-scripts");
        assert!(test_flags.contains("manual translation"));
    }

    #[test]
    fn allows_uv_rg_and_bun_usage() {
        assert_eq!(evaluate_command("uv run pytest", RuleSet::all()), None);
        assert_eq!(
            evaluate_command("uv --directory repo run pytest", RuleSet::all()),
            None
        );
        assert_eq!(evaluate_command("uvx ruff check .", RuleSet::all()), None);
        assert_eq!(evaluate_command("rg pattern .", RuleSet::all()), None);
        assert_eq!(evaluate_command("ripgrep pattern .", RuleSet::all()), None);
        assert_eq!(evaluate_command("bun run dev", RuleSet::all()), None);
        assert_eq!(evaluate_command("bunx prettier .", RuleSet::all()), None);
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
    fn parses_list_rules_flag() {
        let config = Config::parse(["--list-rules".to_string()]).unwrap();

        match config.mode {
            Mode::ListRules => {}
            mode => panic!("unexpected mode: {mode:?}"),
        }
    }

    #[test]
    fn reads_shared_rule_catalog_from_install_script() {
        let catalog = shared_rule_catalog();

        assert_eq!(catalog.len(), 3);
        assert_eq!(catalog[0].cli_name, "rg");
        assert_eq!(catalog[0].aliases.as_slice(), ["ripgrep"]);
        assert_eq!(catalog[1].cli_name, "uv");
        assert!(catalog[1].aliases.is_empty());
        assert_eq!(catalog[2].cli_name, "bun");
        assert!(catalog[2].aliases.is_empty());
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
        assert_eq!(
            evaluate_command("npm install react", RuleSet::only(RuleId::Uv)),
            None
        );

        let rg_only =
            decision_message_with_rules("grep -rn pattern .", RuleSet::only(RuleId::Ripgrep));
        assert!(rg_only.contains("rg pattern ."));

        let uv_only = decision_message_with_rules("python -m pytest", RuleSet::only(RuleId::Uv));
        assert!(uv_only.contains("uv run python -m pytest"));

        let bun_only = decision_message_with_rules("npm run dev", RuleSet::only(RuleId::Bun));
        assert!(bun_only.contains("bun run dev"));
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
