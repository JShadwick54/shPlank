//! The SSH layer: the connection factory (`AppServer`) and the per-connection
//! handler (`ClientHandler`), whose methods russh calls as SSH events occur.

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::{TerminalOptions, Viewport};

use russh::keys::ssh_key::PublicKey;
use russh::server::*; // Server, Handler, Auth, Session, Msg, ...
use russh::{Channel, ChannelId, Pty};

use sqlx::SqlitePool;
use crate::db::Post;

use crate::tui::{TerminalHandle, draw_ui};

/// The "factory". One `AppServer` owns the whole listener; russh asks it for a
/// fresh handler each time a client connects.
#[derive(Clone)]
pub struct AppServer {
    // Per-connection counter, just for the log lines. Private to this module —
    // callers use `AppServer::new()` instead of poking at the field.
    next_id: usize,
    db: SqlitePool,
}

impl AppServer {
    pub fn new(db: SqlitePool) -> Self {
        Self { next_id: 0, db }
    }
}

impl Server for AppServer {
    type Handler = ClientHandler;

    // Called once per incoming TCP connection. We mint that client's handler.
    fn new_client(&mut self, peer_addr: Option<std::net::SocketAddr>) -> ClientHandler {
        self.next_id += 1;
        let id = self.next_id;
        println!("[connect]    client #{id} from {peer_addr:?}");
        ClientHandler { id, fingerprint: None, terminal: None, db: self.db.clone(), posts: Vec::new() }
    }
}

/// Per-connection state and behavior. russh calls the methods below as the SSH
/// session progresses: auth → open channel → pty → shell → (resize) → close.
pub struct ClientHandler {
    id: usize,
    fingerprint: Option<String>,
    terminal: Option<Terminal<CrosstermBackend<TerminalHandle>>>,
    db: SqlitePool,
    posts: Vec<Post>,
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
        Ok(Auth::Accept) // accept ANY key for now
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
        self.posts = match crate::db::list_posts(&self.db).await {
            Ok(posts) => posts,
            Err(e) => {
                eprintln!("[db] failed to load posts: {e}");
                Vec::new()
            }
        };

        if let Some(terminal) = self.terminal.as_mut() {
            let posts = &self.posts;
            terminal.draw(|frame| draw_ui(frame, posts))?;
        }
        session.channel_success(channel)?;
        Ok(())
    }

    // User pressed a key → raw terminal bytes arrive here.
    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        // `q` (0x71) or Ctrl+C (0x03) → close the channel.
        // Closing tears down the connection, which drops the handler,
        // which logs [disconnect] via our existing Drop impl.
        if data.contains(&b'q') || data.contains(&0x03) {
            session.close(channel)?;
        }
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
            let posts = &self.posts;
            terminal.draw(|frame| draw_ui(frame, posts))?;
        }
        Ok(())
    }
}

// Runs automatically when the handler is freed (connection closed). This is
// ownership's "freed at scope end" rule (RAII), used here to log a disconnect.
impl Drop for ClientHandler {
    fn drop(&mut self) {
        println!("[disconnect] client #{}", self.id);
    }
}
