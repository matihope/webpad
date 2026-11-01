mod admin;
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

use crate::admin::AdminAuth;
use crate::backend::BackendKind;
use crate::server::AppState;
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
    /// Password for the Manage page. Without one, anyone on the network can manage players
    #[arg(long, env = "WEBPAD_ADMIN_PASSWORD", hide_env_values = true)]
    admin_password: Option<String>,
    /// Code players must enter to join (the join QR code includes it)
    #[arg(long, env = "WEBPAD_JOIN_CODE", hide_env_values = true)]
    join_code: Option<String>,
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
    let base = format!("http://{}:{}", local_ip_address::local_ip().unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)), args.port);
    let join_code = args.join_code.filter(|c| !c.is_empty());
    let join_url = match &join_code {
        Some(code) => format!("{base}/pad?code={}", percent_encode(code)),
        None => format!("{base}/pad"),
    };
    let admin_password = args.admin_password.filter(|p| !p.is_empty());
    print_join_info(&join_url, &base, admin_password.is_some());

    let app = Arc::new(AppState { sessions, auth: AdminAuth::new(admin_password), join_code, join_url });
    // No graceful shutdown: open WebSockets would keep Ctrl-C waiting, and the OS
    // unplugs every virtual device when the process exits anyway.
    axum::serve(listener, server::router(app)).await?;
    Ok(())
}

fn print_join_info(join_url: &str, base: &str, admin_password: bool) {
    if let Ok(code) = QrCode::new(join_url) {
        println!("\n{}", code.render::<Dense1x2>().quiet_zone(true).build());
    }
    println!("Players: scan the code or open {join_url} (same Wi-Fi network).");
    let access = if admin_password { "password required" } else { "open to everyone, set --admin-password to protect it" };
    println!("Manage:  {base}/admin ({access})\n");
}

/// Percent-encode everything except unreserved URL characters.
fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
