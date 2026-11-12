//! argparse-equivalent CLI parsing and subcommand dispatch.

use std::io::{self, Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Local;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

use crate::commands::{
    cmd_ignore, cmd_reset, cmd_set, cmd_setup, cmd_status, cmd_statusline, cmd_sync, cmd_version,
    Confirm, SetupMode, SetupOptions,
};
use crate::hooks::{
    hook_prompt, hook_session_end, hook_session_start, hook_stop, hook_tick, prompt_core,
    write_hook_json,
};
use crate::paths::{ClaudePaths, Paths};
use crate::remote::Remote;
use crate::setup::{classic_hooks_installed, PLUGIN_HINT};

#[derive(Parser)]
#[command(
    name = "garlic",
    version,
    about = "Track active coding time with Claude Code",
    after_help = PLUGIN_HINT
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Show installed version
    Version,
    /// Set garlic up: the Claude Code plugin (recommended) or settings hooks
    Setup {
        /// Set up for the Claude Code plugin: save the config, remove garlic's
        /// hooks from ~/.claude/settings.json, and print the install commands
        #[arg(long, conflicts_with_all = ["hooks", "remove_hooks", "debug"])]
        plugin: bool,
        /// Install the hooks into ~/.claude/settings.json by hand (the
        /// fallback), even when the plugin is enabled
        #[arg(long, conflicts_with = "remove_hooks")]
        hooks: bool,
        /// Only remove garlic's hooks and slash command from ~/.claude
        #[arg(long)]
        remove_hooks: bool,
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
        /// Print one JSON object (status, and any nudge unframed) instead of
        /// the hook's own output. For the plugin's hooks module.
        #[arg(long)]
        json: bool,
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
    /// No event, time passed: check nudges against the live total.
    #[value(name = "tick")]
    Tick,
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
        Command::Version => cmd_version(&paths, unix_now(), &mut StdinConfirm, &mut out),
        Command::Setup {
            plugin,
            hooks,
            remove_hooks,
            debug,
            yes,
            defaults,
        } => {
            let claude = ClaudePaths::resolve();
            let mode = if remove_hooks {
                SetupMode::RemoveHooks
            } else if plugin {
                SetupMode::Plugin
            } else if hooks {
                SetupMode::Hooks
            } else {
                SetupMode::Ask
            };
            cmd_setup(
                &paths,
                &claude,
                SetupOptions {
                    mode,
                    debug,
                    yes,
                    defaults,
                },
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
        Command::Hook {
            hook_event,
            debug,
            json,
        } => run_hook(&paths, remote.as_ref(), hook_event, debug, json, &mut out),
    }
}

fn run_hook(
    paths: &Paths,
    remote: Option<&Remote>,
    event: HookEvent,
    debug: bool,
    json: bool,
    out: &mut dyn Write,
) -> i32 {
    // Claude Code writes a JSON object to stdin; we use its `session_id` to
    // attribute intervals to the session that produced the event.
    let mut buf = String::new();
    let _ = io::stdin().read_to_string(&mut buf);
    let session_id = parse_session_id(&buf);

    let now = unix_now();
    let now_local = Local::now().naive_local();
    // Hooks are local-first: they always account time into `state.toml` and
    // never block on the network. When a backend is configured they sync only
    // if `GARLIC_SYNC=blocking` is set (the ephemeral/managed-host case).
    let result = if json {
        run_hook_json(
            paths,
            remote,
            event,
            &session_id,
            now,
            now_local,
            debug,
            out,
        )
    } else {
        match event {
            HookEvent::SessionStart => {
                hook_session_start(paths, remote, &session_id, now, debug, out)
            }
            HookEvent::Prompt => {
                hook_prompt(paths, remote, &session_id, now, now_local, debug, out)
            }
            HookEvent::Stop => hook_stop(paths, remote, &session_id, now, debug),
            HookEvent::SessionEnd => hook_session_end(paths, remote, &session_id, now, debug),
            // A tick has nothing to say outside `--json`: the nudge it may find
            // has nowhere to go from a settings hook, so it only keeps state.
            HookEvent::Tick => hook_tick(paths, now, now_local).map(|_| ()),
        }
    };
    match result {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

/// `garlic hook <event> --json`: handle the event exactly as the settings hook
/// would, then print one JSON object in place of the hook's own output.
#[allow(clippy::too_many_arguments)]
fn run_hook_json(
    paths: &Paths,
    remote: Option<&Remote>,
    event: HookEvent,
    session_id: &str,
    now: f64,
    now_local: chrono::NaiveDateTime,
    debug: bool,
    out: &mut dyn Write,
) -> io::Result<()> {
    let mut classic = None;
    let (name, nudge) = match event {
        HookEvent::SessionStart => {
            hook_session_start(paths, remote, session_id, now, debug, &mut io::sink())?;
            classic = Some(classic_hooks_installed(&ClaudePaths::resolve()));
            ("session-start", None)
        }
        HookEvent::Prompt => (
            "prompt",
            prompt_core(paths, remote, session_id, now, now_local, debug)?,
        ),
        HookEvent::Stop => {
            hook_stop(paths, remote, session_id, now, debug)?;
            ("stop", None)
        }
        HookEvent::SessionEnd => {
            hook_session_end(paths, remote, session_id, now, debug)?;
            ("session-end", None)
        }
        HookEvent::Tick => ("tick", hook_tick(paths, now, now_local)?),
    };
    write_hook_json(paths, name, nudge.as_ref(), classic, now, out)
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
