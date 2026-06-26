//! Entry point and bootstrap: start the async runtime, build the server
//! config (including the host key), and run the SSH listener.

use std::sync::Arc;
use std::time::Duration;

use russh::server::{Config, Server}; // Server trait brings `run_on_address` into scope

// Declare the other source files as modules of this crate. Without these two
// lines, `server.rs` and `tui.rs` would simply be ignored by the compiler.
mod server;
mod tui;

use crate::server::AppServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config {
        inactivity_timeout: Some(Duration::from_secs(3600)),
        auth_rejection_time: Duration::from_secs(3),
        keys: vec![load_or_generate_host_key()],
        ..Default::default()
    };

    let mut server = AppServer::new();
    println!("shPlank SSH server listening on 0.0.0.0:2222 — Ctrl-C to stop");
    server.run_on_address(Arc::new(config), ("0.0.0.0", 2222)).await?;
    Ok(())
}

/// Load the SSH host key from disk, generating and persisting one on first run.
/// The key file is gitignored, so each machine creates and keeps its own.
fn load_or_generate_host_key() -> russh::keys::PrivateKey {
    use std::path::Path;
    use russh::keys::ssh_key::{Algorithm, LineEnding};

    let path = Path::new("server_key");
    if path.exists() {
        russh::keys::PrivateKey::read_openssh_file(path)
            .expect("failed to read existing server_key")
    } else {
        let key = russh::keys::PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
            .expect("failed to generate host key");
        key.write_openssh_file(path, LineEnding::LF)
            .expect("failed to write server_key");
        println!("Generated a new host key at ./server_key");
        key
    }
}
