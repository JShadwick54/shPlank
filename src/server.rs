//! The SSH layer: the connection factory (`AppServer`) and the per-connection
//! handler (`ClientHandler`), whose methods russh calls as SSH events occur.

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::{TerminalOptions, Viewport};
use ratatui::widgets::ListState;

use russh::keys::ssh_key::PublicKey;
use russh::server::*; // Server, Handler, Auth, Session, Msg, ...
use russh::{Channel, ChannelId, Pty};

use sqlx::SqlitePool;
use crate::db::Post;

use crate::tui::{TerminalHandle, draw_ui};


/// Which screen the client is currently viewing.
#[derive(Copy, Clone)]
enum Screen {
    List,
    Detail(usize),
}


/// The "factory". One `AppServer` owns the whole listener; russh asks it for a
/// fresh handler each time a client connects.
#[derive(Clone)]
pub struct AppServer {
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
    fn new_client(&mut self, peer_addr: Option<std::net::SocketAddr>) -> ClientHandler {
        self.next_id += 1;
        let id = self.next_id;
        println!("[connect]    client #{id} from {peer_addr:?}");
        ClientHandler {
            id, fingerprint: None, terminal: None, db: self.db.clone(),
            posts: Vec::new(), list_state: ListState::default(),
            screen: Screen::List,
        }
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
    list_state: ListState,
    screen: Screen,
}

impl ClientHandler {
    fn current_post(&self) -> Option<&Post> {
        match self.screen {
            Screen::List => None,
            Screen::Detail(i) => self.posts.get(i),
        }
    }
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
        if !self.posts.is_empty() {
            self.list_state.select(Some(0));
        }

        if let Some(terminal) = self.terminal.as_mut() {
            let posts = &self.posts;
            let state = &mut self.list_state;
            let detail = match self.screen {
                Screen::List => None,
                Screen::Detail(i) => posts.get(i),
            };
            terminal.draw(|frame| draw_ui(frame, posts, state, detail))?;
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
        // q / Ctrl+C still quit.
        if data.contains(&b'q') || data.contains(&0x03) {
            session.close(channel)?;
            return Ok(());
        }

        //input
        match self.screen {
            Screen::List => {
                let len = self.posts.len();
                if len > 0 {
                    let selected = self.list_state.selected().unwrap_or(0);
                    if data == b"\x1b[A" || data == b"\x1bOA" {
                        self.list_state.select(Some(selected.saturating_sub(1)));
                    } else if data == b"\x1b[B" || data == b"\x1bOB" {
                        self.list_state.select(Some((selected + 1).min(len - 1)));
                    } else if data == b"\r" {
                        // Enter — open the selected post.
                        self.screen = Screen::Detail(selected);
                    }
                }
            }
            Screen::Detail(_) => {
                // b or Escape — go back to the list.
                if data == b"b" || data == b"\x1b" {
                    self.screen = Screen::List;
                }
            }
        }

        // Redraw so the highlight moves.
        if let Some(terminal) = self.terminal.as_mut() {
            let posts = &self.posts;
            let state = &mut self.list_state;
            let detail = match self.screen {
                Screen::List => None,
                Screen::Detail(i) => posts.get(i),
            };
            terminal.draw(|frame| draw_ui(frame, posts, state, detail))?;
        }
        Ok(())
    }

    //User resized their window
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
            let state = &mut self.list_state;
            let detail = match self.screen {
                Screen::List => None,
                Screen::Detail(i) => posts.get(i),
            };
            terminal.draw(|frame| draw_ui(frame, posts, state, detail))?;
        }
        Ok(())
    }
}

// Runs automatically when the handler is freed, used here to log a disconnect.
impl Drop for ClientHandler {
    fn drop(&mut self) {
        println!("[disconnect] client #{}", self.id);
    }
}
