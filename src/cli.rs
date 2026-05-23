//! argparse-equivalent CLI parsing and subcommand dispatch.

use std::io::{self, Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Local;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

use crate::commands::{
    cmd_ignore, cmd_reset, cmd_set, cmd_setup, cmd_stats, cmd_status, cmd_statusline, cmd_version,
    cmd_week, Confirm,
};
use crate::hooks::{hook_prompt, hook_session_end, hook_session_start, hook_stop};
use crate::paths::{ClaudePaths, Paths};

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
    /// Show accumulated active time today
    Status {
        /// Emit status as a JSON object
        #[arg(long)]
        json: bool,
    },
    /// Output a compact status line string for Claude Code
    Statusline,
    /// Show rolling 7-day usage summary
    Week,
    /// Show monthly totals, streaks, and averages
    Stats,
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
        Command::Status { json } => cmd_status(&paths, json, &mut out),
        Command::Statusline => cmd_statusline(&paths, &mut out),
        Command::Week => cmd_week(&paths, Local::now().naive_local(), &mut out),
        Command::Stats => cmd_stats(&paths, Local::now().naive_local(), &mut out),
        Command::Ignore => cmd_ignore(&paths, &mut out),
        Command::Set { assignment } => cmd_set(&paths, &assignment, &mut out, &mut err),
        Command::Reset { yes } => cmd_reset(&paths, yes, &mut StdinConfirm, &mut out),
        Command::Hook { hook_event, debug } => run_hook(&paths, hook_event, debug, &mut out),
    }
}

fn run_hook(paths: &Paths, event: HookEvent, debug: bool, out: &mut dyn Write) -> i32 {
    // Consume the JSON Claude Code writes to stdin (the content is unused).
    let mut buf = String::new();
    let _ = io::stdin().read_to_string(&mut buf);

    let now = unix_now();
    let result = match event {
        HookEvent::SessionStart => hook_session_start(paths, now, debug, out),
        HookEvent::Prompt => hook_prompt(paths, now, Local::now().naive_local(), debug, out),
        HookEvent::Stop => hook_stop(paths, now, debug),
        HookEvent::SessionEnd => hook_session_end(paths, now, debug),
    };
    match result {
        Ok(()) => 0,
        Err(_) => 1,
    }
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
