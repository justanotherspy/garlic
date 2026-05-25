//! argparse-equivalent CLI parsing and subcommand dispatch.

use std::io::{self, Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Local;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

use crate::commands::{
    cmd_ignore, cmd_ignore_remote, cmd_reset, cmd_reset_remote, cmd_set, cmd_setup, cmd_status,
    cmd_status_remote, cmd_statusline, cmd_statusline_remote, cmd_version, Confirm,
};
use crate::hooks::{
    hook_prompt, hook_prompt_remote, hook_session_end, hook_session_end_remote, hook_session_start,
    hook_session_start_remote, hook_stop, hook_stop_remote,
};
use crate::paths::{ClaudePaths, Paths};
use crate::remote::Remote;

#[derive(Parser)]
#[command(
    name = "garlic",
    version,
    about = "Track active coding time with Claude Code"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Show installed version
    Version,
    /// Install hooks into ~/.claude/settings.json
    Setup {
        /// Install hooks with debug logging
        #[arg(long)]
        debug: bool,
        /// Skip interactive prompts and use all defaults
        #[arg(short = 'y', long = "yes")]
        yes: bool,
        /// Overwrite existing config with built-in defaults
        #[arg(long)]
        defaults: bool,
    },
    /// Show accumulated active time today (or a weekly/monthly summary)
    Status {
        /// Emit status as a JSON object
        #[arg(long)]
        json: bool,
        /// Show the rolling 7-day usage summary instead of today
        #[arg(long, conflicts_with = "month")]
        week: bool,
        /// Show monthly totals, streaks, and averages instead of today
        #[arg(long)]
        month: bool,
    },
    /// Output a compact status line string for Claude Code
    Statusline,
    /// Toggle nudging for the day
    Ignore,
    /// Update a config value (KEY=VALUE)
    Set {
        /// Config assignment (e.g. nudge_style=spicy)
        assignment: String,
    },
    /// Reset daily timer to zero
    Reset {
        /// Skip confirmation prompt
        #[arg(short = 'y', long = "yes")]
        yes: bool,
    },
    /// Handle a Claude Code hook event
    Hook {
        /// Which hook event to handle
        #[arg(value_enum)]
        hook_event: HookEvent,
        /// Log gap calculations to stderr
        #[arg(long)]
        debug: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum HookEvent {
    #[value(name = "session-start")]
    SessionStart,
    #[value(name = "prompt")]
    Prompt,
    #[value(name = "stop")]
    Stop,
    #[value(name = "session-end")]
    SessionEnd,
}

/// Current Unix time in seconds.
pub fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Reads confirmation lines from stdin, printing the prompt to stdout.
struct StdinConfirm;

impl Confirm for StdinConfirm {
    fn ask(&mut self, prompt: &str) -> Option<String> {
        print!("{prompt}");
        io::stdout().flush().ok();
        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim_end_matches(['\n', '\r']).to_string()),
        }
    }
}

/// Parse arguments and dispatch, returning the process exit code.
pub fn run() -> i32 {
    let cli = Cli::parse();
    let paths = Paths::resolve();

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let stderr = io::stderr();
    let mut err = stderr.lock();

    let Some(command) = cli.command else {
        let _ = Cli::command().print_help();
        return 1;
    };

    // Shared-state mode: when a backend is configured, state operations route
    // through it instead of the local state file. Config always stays local.
    let remote = Remote::from_env();

    match command {
        Command::Version => cmd_version(&paths, unix_now(), &mut out),
        Command::Setup {
            debug,
            yes,
            defaults,
        } => {
            let claude = ClaudePaths::resolve();
            cmd_setup(
                &paths,
                &claude,
                debug,
                yes,
                defaults,
                &mut StdinConfirm,
                &mut out,
            )
        }
        Command::Status { json, week, month } => match &remote {
            Some(r) => cmd_status_remote(r, &paths, json, week, month, &mut out, &mut err),
            None => cmd_status(
                &paths,
                json,
                week,
                month,
                Local::now().naive_local(),
                &mut out,
            ),
        },
        Command::Statusline => match &remote {
            Some(r) => cmd_statusline_remote(r, &paths, &mut out),
            None => cmd_statusline(&paths, &mut out),
        },
        Command::Ignore => match &remote {
            Some(r) => cmd_ignore_remote(r, &paths, &mut out, &mut err),
            None => cmd_ignore(&paths, &mut out),
        },
        Command::Set { assignment } => cmd_set(&paths, &assignment, &mut out, &mut err),
        Command::Reset { yes } => match &remote {
            Some(r) => cmd_reset_remote(r, &paths, yes, &mut StdinConfirm, &mut out, &mut err),
            None => cmd_reset(&paths, yes, &mut StdinConfirm, &mut out),
        },
        Command::Hook { hook_event, debug } => {
            run_hook(&paths, remote.as_ref(), hook_event, debug, &mut out)
        }
    }
}

fn run_hook(
    paths: &Paths,
    remote: Option<&Remote>,
    event: HookEvent,
    debug: bool,
    out: &mut dyn Write,
) -> i32 {
    // Claude Code writes a JSON object to stdin; we use its `session_id` to
    // attribute intervals to the session that produced the event.
    let mut buf = String::new();
    let _ = io::stdin().read_to_string(&mut buf);
    let session_id = parse_session_id(&buf);

    let now = unix_now();
    let result = match (event, remote) {
        (HookEvent::SessionStart, Some(r)) => {
            hook_session_start_remote(r, paths, &session_id, debug, out)
        }
        (HookEvent::SessionStart, None) => hook_session_start(paths, &session_id, now, debug, out),
        (HookEvent::Prompt, Some(r)) => hook_prompt_remote(r, paths, &session_id, debug, out),
        (HookEvent::Prompt, None) => hook_prompt(
            paths,
            &session_id,
            now,
            Local::now().naive_local(),
            debug,
            out,
        ),
        (HookEvent::Stop, Some(r)) => hook_stop_remote(r, paths, &session_id, debug),
        (HookEvent::Stop, None) => hook_stop(paths, &session_id, now, debug),
        (HookEvent::SessionEnd, Some(r)) => hook_session_end_remote(r, paths, &session_id, debug),
        (HookEvent::SessionEnd, None) => hook_session_end(paths, &session_id, now, debug),
    };
    match result {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

/// Extract `session_id` from the hook's stdin JSON, falling back to `"default"`
/// when it's missing or the payload isn't valid JSON (e.g. manual invocation).
fn parse_session_id(stdin: &str) -> String {
    serde_json::from_str::<serde_json::Value>(stdin)
        .ok()
        .as_ref()
        .and_then(|v| v.get("session_id"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("default")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses() {
        // Smoke test: the derived parser is well-formed.
        Cli::command().debug_assert();
    }
}
