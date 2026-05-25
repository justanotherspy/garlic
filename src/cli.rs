//! argparse-equivalent CLI parsing and subcommand dispatch.

use std::io::{self, Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Local;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

use crate::commands::{
    cmd_ignore, cmd_reset, cmd_set, cmd_setup, cmd_status, cmd_statusline, cmd_sync, cmd_version,
    Confirm,
};
use crate::hooks::{hook_prompt, hook_session_end, hook_session_start, hook_stop};
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
    /// Push locally-tracked time to the shared backend (for cron/manual sync)
    Sync,
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
        Command::Status { json, week, month } => cmd_status(
            remote.as_ref(),
            &paths,
            json,
            week,
            month,
            Local::now().naive_local(),
            &mut out,
            &mut err,
        ),
        Command::Statusline => cmd_statusline(&paths, &mut out),
        Command::Sync => cmd_sync(remote.as_ref(), &paths, &mut out, &mut err),
        Command::Ignore => cmd_ignore(remote.as_ref(), &paths, &mut out, &mut err),
        Command::Set { assignment } => cmd_set(&paths, &assignment, &mut out, &mut err),
        Command::Reset { yes } => cmd_reset(
            remote.as_ref(),
            &paths,
            yes,
            &mut StdinConfirm,
            &mut out,
            &mut err,
        ),
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
    // Hooks are local-first: they always account time into `state.toml` and
    // never block on the network. When a backend is configured they sync only
    // if `GARLIC_SYNC=blocking` is set (the ephemeral/managed-host case).
    let result = match event {
        HookEvent::SessionStart => hook_session_start(paths, remote, &session_id, now, debug, out),
        HookEvent::Prompt => hook_prompt(
            paths,
            remote,
            &session_id,
            now,
            Local::now().naive_local(),
            debug,
            out,
        ),
        HookEvent::Stop => hook_stop(paths, remote, &session_id, now, debug),
        HookEvent::SessionEnd => hook_session_end(paths, remote, &session_id, now, debug),
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
