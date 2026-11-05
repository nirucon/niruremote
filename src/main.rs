mod agent;
mod client;
mod events;
mod json;
mod session;
mod ssh;
mod state;
mod tailscale;
mod util;
mod viewer;
mod x11vnc;

use client::ConnectOptions;
use std::process::ExitCode;
use std::time::Duration;

fn main() -> ExitCode {
    std::panic::set_hook(Box::new(|info| {
        let message = if let Some(message) = info.payload().downcast_ref::<&str>() {
            *message
        } else if let Some(message) = info.payload().downcast_ref::<String>() {
            message.as_str()
        } else {
            ""
        };
        if message.contains("Broken pipe") || message.contains("os error 32") {
            std::process::exit(0);
        }
        eprintln!("niruremote: internal error: {info}");
        std::process::exit(101);
    }));
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("niruremote: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let command = std::env::args().nth(1);
    let Some(command) = command else {
        print_help();
        return Ok(());
    };
    let rest: Vec<String> = std::env::args().skip(2).collect();

    match command.as_str() {
        "probe" => {
            let local = agent::probe();
            agent::print_local(&local, has_flag(&rest, "--json"))
        }
        "status" => {
            let local = agent::probe();
            agent::print_local(&local, has_flag(&rest, "--json"))
        }
        "enable" => agent::enable(),
        "disable" => agent::disable(),
        "ensure" => agent::ensure(has_flag(&rest, "--json")),
        "stream" => agent::stream(),
        "peers" => {
            let response = client::peers(has_flag(&rest, "--refresh"), timeout_from(&rest, 3)?)?;
            if has_flag(&rest, "--json") {
                let text =
                    serde_json::to_string_pretty(&response).map_err(|error| error.to_string())?;
                println!("{text}");
            } else {
                client::print_peers(&response);
            }
            Ok(())
        }
        "connect" => {
            let host = positional(&rest)
                .ok_or_else(|| "connect needs a Tailscale machine name or IP".to_string())?;
            client::connect(
                &host,
                &ConnectOptions {
                    print: has_flag(&rest, "--print"),
                    timeout: timeout_from(&rest, 3)?,
                },
            )
        }
        "setup" => agent::setup(has_flag(&rest, "--run")),
        "doctor" => agent::doctor(),
        "version" | "--version" | "-V" => {
            println!("niruremote {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => Err(format!(
            "unknown command: {other}\nRun `niruremote help` for usage."
        )),
    }
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|argument| argument == flag)
}

fn positional(args: &[String]) -> Option<String> {
    let mut index = 0;
    while index < args.len() {
        let argument = &args[index];
        if argument == "--timeout" {
            index += 2;
            continue;
        }
        if !argument.starts_with('-') {
            return Some(argument.clone());
        }
        index += 1;
    }
    None
}

fn timeout_from(args: &[String], default: u64) -> Result<Duration, String> {
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--timeout" {
            let value = args
                .get(index + 1)
                .ok_or_else(|| "--timeout needs a number of seconds".to_string())?;
            let seconds: u64 = value
                .parse()
                .map_err(|_| format!("invalid timeout: {value}"))?;
            if !(1..=30).contains(&seconds) {
                return Err("timeout must be between 1 and 30 seconds".into());
            }
            return Ok(Duration::from_secs(seconds));
        }
        index += 1;
    }
    Ok(Duration::from_secs(default))
}

fn print_help() {
    println!(
        "niruremote {version}

Graphical remote desktop for the NIRUX suite over Tailscale and SSH.

USAGE:
    niruremote <COMMAND> [OPTIONS]

AGENT (run on the machine that is shared):
    probe [--json]              Inspect the local X11 session and x11vnc
    status [--json]             Alias for probe
    enable                      Password-protected loopback listener for manual
                                Windows/macOS tunnels (until disable/logout)
    disable                     Stop the listener and end every remote session
    stream                      Serve one session on stdin/stdout (used by connect)
    setup [--run]               Check/install x11vnc and a VNC viewer
    doctor                      Print diagnostics

CLIENT (run on the machine you sit at):
    peers [--json] [--refresh] [--timeout N]
                                List Tailscale machines and remote readiness
    connect <MACHINE> [--print] [--timeout N]
                                Connect to a Linux host over SSH + VNC,
                                a Windows host over RDP, or a macOS host over VNC

    version                     Print version
    help                        Print this help

EXAMPLES:
    niruremote setup --run
    niruremote probe --json
    niruremote peers --refresh
    niruremote connect desktop
    niruremote connect laptop --print

A Linux connection runs x11vnc in -inetd mode on the SSH channel itself: no
port is opened on the host, the tailnet or the LAN, and the session ends with
the channel. Screen locks are never bypassed.",
        version = env!("CARGO_PKG_VERSION")
    );
}
