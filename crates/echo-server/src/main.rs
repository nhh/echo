//! echo-server: der Sprachserver zum echo-Client.
//!
//! Beim ersten Start entstehen Host-Schluessel, Zertifikat und ein Kanal
//! "Lobby". Der erste Client, der sich verbindet, wird Besitzer.

// Die Konsole ist die Ausgabe des Servers.
#![allow(clippy::print_stdout)]

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use echo_server::{EchoServer, ServerOptions, ServerPaths};

#[derive(Debug, Parser)]
#[command(
    name = "echo-server",
    version,
    about = "Der Sprachserver zum echo-Client"
)]
struct Args {
    /// Datenordner; Standard ist das Profil (%AppData%\echo-server bzw. ~/.config/echo-server).
    #[arg(long)]
    data: Option<PathBuf>,

    /// Port fuer diesen Lauf.
    #[arg(long)]
    port: Option<u16>,

    /// Servername (wird gespeichert).
    #[arg(long)]
    name: Option<String>,

    /// Serverpasswort (wird gespeichert; "" entfernt es).
    #[arg(long)]
    password: Option<String>,

    /// Unbekannte muessen erst freigeschaltet werden: on oder off (wird gespeichert).
    #[arg(long, value_parser = parse_switch)]
    approval: Option<bool>,
}

fn parse_switch(value: &str) -> Result<bool, String> {
    match value {
        "on" | "an" | "true" => Ok(true),
        "off" | "aus" | "false" => Ok(false),
        other => Err(format!("on oder off erwartet, nicht „{other}“")),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let paths = ServerPaths::new(args.data).context("Datenordner nicht anlegbar")?;
    if args.name.is_some() || args.password.is_some() || args.approval.is_some() {
        EchoServer::update_settings(&paths, |settings| {
            if let Some(name) = args.name {
                settings.name = name;
            }
            if let Some(password) = args.password {
                settings.password = (!password.is_empty()).then_some(password);
            }
            if let Some(approval) = args.approval {
                settings.require_approval = approval;
            }
        })
        .context("Einstellungen nicht gespeichert")?;
    }

    let log: echo_server::Log = Arc::new(|line| {
        let now = time::OffsetDateTime::now_utc();
        println!(
            "[{:02}:{:02}:{:02}] {line}",
            now.hour(),
            now.minute(),
            now.second()
        );
    });
    let mut options = ServerOptions::default();
    if let Some(port) = args.port {
        options.bind = Some(std::net::SocketAddr::from((
            std::net::Ipv6Addr::UNSPECIFIED,
            port,
        )));
    }

    let server = EchoServer::start(&paths, None, &options, log).context("Server startet nicht")?;
    tokio::signal::ctrl_c()
        .await
        .context("Strg+C nicht abfangbar")?;
    println!("Beende …");
    server.stop().await;
    Ok(())
}
