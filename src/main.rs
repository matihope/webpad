mod backend;
mod protocol;
mod server;
mod session;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use axum::serve::ListenerExt;
use clap::Parser;
use qrcode::QrCode;
use qrcode::render::unicode::Dense1x2;

use crate::backend::BackendKind;
use crate::session::Sessions;

/// Use phones as game controllers: serves a controller web page and turns each
/// connected phone into a virtual gamepad on this machine.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// Port to listen on
    #[arg(long, default_value_t = 8080)]
    port: u16,
    /// Where controller input goes
    #[arg(long, value_enum, default_value_t = BackendKind::Auto)]
    backend: BackendKind,
    /// Maximum number of simultaneous players
    #[arg(long, default_value_t = 4, value_parser = clap::value_parser!(u8).range(1..=16))]
    max_players: u8,
    /// Seconds a disconnected player's pad stays plugged in, waiting for a reconnect
    #[arg(long, default_value_t = 30)]
    grace: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "webpad=info".into()),
        )
        .with_target(false)
        .init();
    let args = Args::parse();

    let backend = backend::create(args.backend)?;
    tracing::info!("backend: {}", backend.name());
    let sessions = Arc::new(Sessions::new(backend, args.max_players, Duration::from_secs(args.grace)));

    let reaper = sessions.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            reaper.reap(Instant::now());
        }
    });

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), args.port);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("could not listen on port {}", args.port))?
        .tap_io(|tcp| {
            // Input frames are tiny; don't let Nagle batch them.
            let _ = tcp.set_nodelay(true);
        });
    print_join_info(args.port);

    // No graceful shutdown: open WebSockets would keep Ctrl-C waiting, and the OS
    // unplugs every virtual device when the process exits anyway.
    axum::serve(listener, server::router(sessions)).await?;
    Ok(())
}

fn print_join_info(port: u16) {
    let ip = local_ip_address::local_ip().unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let url = format!("http://{ip}:{port}");
    if let Ok(code) = QrCode::new(&url) {
        println!("\n{}", code.render::<Dense1x2>().quiet_zone(true).build());
    }
    println!("Scan the code or open {url} on your phone (same Wi-Fi network).\n");
}
