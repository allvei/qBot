//! Local control socket for headless mode, plus the `qbot ctl` client.
//!
//! The bot listens on `qbot.sock` in its working directory. `qbot ctl <command>` connects to it,
//! sends one JSON line and prints the reply. Stop and restart are forwarded to the command task
//! as `GuiCommand`s, so they share the same cleanup code as the GUI buttons.

use std::io::{BufRead, BufReader as SyncBufReader, Write};
use std::os::unix::net::UnixStream as SyncUnixStream;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::gui::commands::GuiCommand;
use crate::shutdown::ShutdownNotice;

pub const SOCKET_PATH: &str = "qbot.sock";
const MAX_REASON_LEN: usize = 200;

const HELP: &str = "\
qBot control

Usage: qbot ctl <command> [options]

Commands:
  status               Show whether the bot is running
  stop                 Stop the bot. Removes empty team VCs and marks dashboards offline
  restart              Restart the bot. Keeps team VCs and marks dashboards restarting
  help                 Show this help

Options (stop, restart):
  --reason <text>      Reason shown on dashboards (max 200 characters)
  --eta <duration>     Expected return time shown on dashboards: 90s, 5m, 2h (plain number = seconds)

Examples:
  qbot ctl restart --reason \"deploy\" --eta 5m
  qbot ctl stop --reason \"maintenance\"
";

#[derive(Serialize, Deserialize)]
struct Request {
  cmd: String,
  reason: Option<String>,
  eta_secs: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct Reply {
  ok: bool,
  message: String,
}

impl Reply {
  fn ok(message: impl Into<String>) -> Self {
    Self { ok: true, message: message.into() }
  }

  fn fail(message: impl Into<String>) -> Self {
    Self { ok: false, message: message.into() }
  }
}

/// Accept control connections until the process exits. Runs as a background task in headless mode.
pub async fn serve(cmd_tx: mpsc::Sender<GuiCommand>) {
  let _ = std::fs::remove_file(SOCKET_PATH);
  let listener = match UnixListener::bind(SOCKET_PATH) {
    Ok(listener) => listener,
    Err(e) => return warn!("Control socket unavailable at {}: {}", SOCKET_PATH, e),
  };
  info!("Control socket listening on {}", SOCKET_PATH);

  loop {
    match listener.accept().await {
      Ok((stream, _)) => {
        tokio::spawn(handle_connection(stream, cmd_tx.clone()));
      }
      Err(e) => warn!("Control socket accept failed: {}", e),
    }
  }
}

async fn handle_connection(stream: UnixStream, cmd_tx: mpsc::Sender<GuiCommand>) {
  let (reader, mut writer) = stream.into_split();
  let mut line = String::new();
  let reply = match BufReader::new(reader).read_line(&mut line).await {
    Ok(_) => dispatch(&line, &cmd_tx).await,
    Err(e) => Reply::fail(format!("Failed to read request: {}", e)),
  };
  if let Ok(json) = serde_json::to_string(&reply) {
    let _ = writer.write_all(format!("{}\n", json).as_bytes()).await;
  }
}

async fn dispatch(line: &str, cmd_tx: &mpsc::Sender<GuiCommand>) -> Reply {
  let req: Request = match serde_json::from_str(line) {
    Ok(req) => req,
    Err(e) => return Reply::fail(format!("Invalid request: {}", e)),
  };

  let reason = req.reason.map(|r| r.trim().chars().take(MAX_REASON_LEN).collect::<String>()).filter(|r| !r.is_empty());
  let notice = ShutdownNotice { reason, eta: req.eta_secs.map(|secs| chrono::Utc::now().timestamp() + secs as i64) };

  let (cmd, message) = match req.cmd.as_str() {
    "status" => return Reply::ok(format!("qBot is online (pid {})", std::process::id())),
    "stop" => (GuiCommand::GracefulShutdown(notice), "Stopping qBot"),
    "restart" => (GuiCommand::GracefulRestart(notice), "Restarting qBot"),
    other => return Reply::fail(format!("Unknown command: {}", other)),
  };

  match cmd_tx.send(cmd).await {
    Ok(_) => Reply::ok(message),
    Err(_) => Reply::fail("qBot is already shutting down"),
  }
}

/// Entry point for `qbot ctl ...`. `args` are the arguments after `ctl`.
pub fn run_ctl(args: &[String]) -> Result<()> {
  let Some(cmd) = args.first().map(String::as_str) else {
    print!("{}", HELP);
    return Ok(());
  };
  if matches!(cmd, "help" | "-h" | "--help") {
    print!("{}", HELP);
    return Ok(());
  }

  let req = parse_request(cmd, &args[1..])?;
  let reply = send(&req)?;
  if !reply.ok {
    eprintln!("{}", reply.message);
    std::process::exit(1);
  }
  println!("{}", reply.message);
  Ok(())
}

fn parse_request(cmd: &str, args: &[String]) -> Result<Request> {
  let mut req = Request { cmd: cmd.to_string(), reason: None, eta_secs: None };

  match cmd {
    "status" if !args.is_empty() => bail!("'status' takes no options\n\n{}", HELP),
    "status" => {}
    "stop" | "restart" => {
      let mut i = 0;
      while i < args.len() {
        let value = args.get(i + 1).ok_or_else(|| anyhow!("Missing value for {}", args[i]))?;
        match args[i].as_str() {
          "--reason" => req.reason = Some(value.clone()),
          "--eta" => req.eta_secs = Some(parse_duration_secs(value)?),
          other => bail!("Unknown option '{}'\n\n{}", other, HELP),
        }
        i += 2;
      }
    }
    other => bail!("Unknown command '{}'\n\n{}", other, HELP),
  }
  Ok(req)
}

/// Parse `90s`, `5m`, `2h` or a plain number of seconds.
fn parse_duration_secs(s: &str) -> Result<u64> {
  let (num, mult) = if let Some(n) = s.strip_suffix('s') {
    (n, 1)
  } else if let Some(n) = s.strip_suffix('m') {
    (n, 60)
  } else if let Some(n) = s.strip_suffix('h') {
    (n, 3600)
  } else {
    (s, 1)
  };
  num.parse::<u64>().ok().and_then(|n| n.checked_mul(mult)).ok_or_else(|| anyhow!("Invalid duration '{}' (use e.g. 90s, 5m, 2h)", s))
}

fn send(req: &Request) -> Result<Reply> {
  let mut stream = SyncUnixStream::connect(SOCKET_PATH).map_err(|_| anyhow!("qBot is not running (no {} in the current directory)", SOCKET_PATH))?;
  writeln!(stream, "{}", serde_json::to_string(req)?)?;

  let mut line = String::new();
  SyncBufReader::new(stream).read_line(&mut line)?;
  Ok(serde_json::from_str(&line)?)
}
