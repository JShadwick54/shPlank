use std::sync::Arc;
use std::time::Duration;

use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Terminal, TerminalOptions, Viewport};

use russh::keys::ssh_key::PublicKey;
use russh::server::*; // Server, Handler, Config, Auth, Session, Handle, Msg
use russh::{Channel, ChannelId, Pty};

use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

// ── Bridge: ratatui writes bytes here; we forward them over the SSH channel ─
struct TerminalHandle {
    sender: UnboundedSender<Vec<u8>>,
    sink: Vec<u8>, // ratatui writes in small pieces; we buffer until flush
}

impl TerminalHandle {
    fn start(handle: Handle, channel_id: ChannelId) -> Self {
        let (sender, mut receiver) = unbounded_channel::<Vec<u8>>();
        // A background task that takes buffered bytes and sends them to the
        // client. tokio::spawn = "run this concurrently"; it ends when the
        // sender is dropped (i.e. when this connection closes).
        tokio::spawn(async move {
            while let Some(data) = receiver.recv().await {
                if let Err(e) = handle.data(channel_id, data).await {
                    eprintln!("failed to send data to client: {e:?}");
                    break;
                }
            }
        });
        Self { sender, sink: Vec::new() }
    }
}

impl std::io::Write for TerminalHandle {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.sink.extend_from_slice(buf); // just accumulate
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // Hand the buffered bytes off to the background sender, then clear.
        let data = std::mem::take(&mut self.sink);
        self.sender
            .send(data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e))?;
        Ok(())
    }
}

// What the static screen looks like. Called every time we (re)draw.
fn draw_ui(frame: &mut ratatui::Frame) {
    let block = Block::default()
        .title(" shPlank ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let body = Paragraph::new(
        "Welcome to shPlank — the SSH forum.\n\n\
         Nothing here yet, but you're looking at a real ratatui screen\n\
         rendered over SSH.\n\n\
         (Close the connection with ~. or Ctrl-C for now.)",
    )
        .block(block);

    frame.render_widget(body, frame.area());
}

// ── Factory: one handler per connection ──────────────────────────────────
#[derive(Clone)]
struct AppServer {
    next_id: usize,
}

impl Server for AppServer {
    type Handler = ClientHandler;

    fn new_client(&mut self, peer_addr: Option<std::net::SocketAddr>) -> ClientHandler {
        self.next_id += 1;
        let id = self.next_id;
        println!("[connect]    client #{id} from {peer_addr:?}");
        ClientHandler { id, fingerprint: None, terminal: None }
    }
}

// ── Per-connection handler ────────────────────────────────────────────────
struct ClientHandler {
    id: usize,
    fingerprint: Option<String>,
    // None until the session channel opens, then holds the live TUI terminal.
    terminal: Option<Terminal<CrosstermBackend<TerminalHandle>>>,
}

impl Handler for ClientHandler {
    type Error = russh::Error;

    async fn auth_publickey(
        &mut self,
        user: &str,
        public_key: &PublicKey,
    ) -> Result<Auth, Self::Error> {
        let fingerprint = public_key.fingerprint(russh::keys::HashAlg::Sha256).to_string();
        println!("[auth]       client #{} user='{user}' key={fingerprint}", self.id);
        self.fingerprint = Some(fingerprint);
        Ok(Auth::Accept)
    }

    // 1) Session channel opens → build the ratatui terminal over the channel.
    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        session: &mut Session,
    ) -> Result<bool, Self::Error> {
        let handle = session.handle();
        let terminal_handle = TerminalHandle::start(handle, channel.id());
        let backend = CrosstermBackend::new(terminal_handle);
        let options = TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 80, 24)), // resized on pty_request
        };
        self.terminal = Some(Terminal::with_options(backend, options)?);
        Ok(true)
    }

    // 2) Client reports terminal size → resize our viewport to match.
    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let rect = Rect::new(0, 0, col_width as u16, row_height as u16);
        if let Some(terminal) = self.terminal.as_mut() {
            terminal.resize(rect)?;
        }
        session.channel_success(channel)?;
        Ok(())
    }

    // 3) Shell starts → draw the screen.
    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if let Some(terminal) = self.terminal.as_mut() {
            terminal.draw(draw_ui)?;
        }
        session.channel_success(channel)?;
        Ok(())
    }

    // 4) User resized their window → resize + redraw.
    async fn window_change_request(
        &mut self,
        _channel: ChannelId,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        let rect = Rect::new(0, 0, col_width as u16, row_height as u16);
        if let Some(terminal) = self.terminal.as_mut() {
            terminal.resize(rect)?;
            terminal.draw(draw_ui)?;
        }
        Ok(())
    }
}

impl Drop for ClientHandler {
    fn drop(&mut self) {
        println!("[disconnect] client #{}", self.id);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config {
        inactivity_timeout: Some(Duration::from_secs(3600)),
        auth_rejection_time: Duration::from_secs(3),
        keys: vec![load_or_generate_host_key()],
        ..Default::default()
    };

    let mut server = AppServer { next_id: 0 };
    println!("shPlank SSH server listening on 0.0.0.0:2222 — Ctrl-C to stop");
    server.run_on_address(Arc::new(config), ("0.0.0.0", 2222)).await?;
    Ok(())
}

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