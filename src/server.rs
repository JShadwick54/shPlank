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
use crate::db::{Comment, Post};

use crate::tui::{TerminalHandle, ComposeField};


/// Which screen the client is currently viewing. The `usize` payloads index
/// into `ClientHandler.posts` (the post being viewed / commented on).
#[derive(Copy, Clone)]
enum Screen {
    List,                   // the scrollable list of posts
    Detail(usize),          // one post's title + body + comments
    ComposePost,            // writing a new post (title + body)
    ComposeComment(usize),  // writing a comment on posts[usize]
    SetName,                // first-time visitor choosing a display name
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
            screen: Screen::List, comments: Vec::new(),
            compose_title: String::new(),
            compose_body: String::new(),
            compose_field: ComposeField::Body,
            current_user_id: None,
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
    comments: Vec<Comment>,
    compose_title: String,
    compose_body: String,
    compose_field: ComposeField,
    current_user_id: Option<i64>,
}

impl ClientHandler {
    /// Render the current screen to the client. This is the single place that
    /// draws — every event handler calls it after updating state. The locals
    /// are pulled out as *direct field* borrows so they stay disjoint from the
    /// `&mut self.terminal` borrow (a method call here would borrow all of self).
    fn redraw(&mut self) -> Result<(), russh::Error> {
        if let Some(terminal) = self.terminal.as_mut() {
            let posts = &self.posts;
            let comments = &self.comments;
            let list_state = &mut self.list_state;
            let compose_title = &self.compose_title;
            let compose_body = &self.compose_body;
            let compose_field = self.compose_field;
            let screen = self.screen;
            terminal.draw(|frame| {
                match screen {
                    Screen::List => crate::tui::draw_list(frame, posts, list_state),
                    Screen::Detail(i) => {
                        if let Some(p) = posts.get(i) {
                            crate::tui::draw_detail(frame, p, comments);
                        }
                    }
                    Screen::ComposePost => {
                        crate::tui::draw_compose_post(frame, compose_title, compose_body, compose_field);
                    }
                    Screen::ComposeComment(_) => {
                        crate::tui::draw_compose_comment(frame, compose_body);
                    }
                    Screen::SetName => {
                        crate::tui::draw_set_name(frame, compose_body);
                    }
                }
            })?;
        }
        Ok(())
    }

    /// Route typed bytes into whichever post-composer field currently has focus.
    fn edit_field(&mut self, data: &[u8]) {
        match self.compose_field {
            ComposeField::Title => push_printable(&mut self.compose_title, data),
            ComposeField::Body => push_printable(&mut self.compose_body, data),
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

    // Session channel opened → build the ratatui terminal over the SSH channel.
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

    // Client reported its terminal size → match our viewport to it.
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

    // Shell started → resolve the user, load posts, and draw the first screen.
    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        // Resolve identity from the SSH key fingerprint captured at auth.
        let fingerprint = self.fingerprint.clone().unwrap_or_default();
        match crate::db::get_user_by_fingerprint(&self.db, &fingerprint).await {
            Ok(Some(user)) => {
                self.current_user_id = Some(user.id);
            }
            Ok(None) => {
                // First time we've seen this key → prompt for a name.
                self.current_user_id = None;
                self.compose_body.clear();
                self.screen = Screen::SetName;
            }
            Err(e) => {
                eprintln!("[db] failed to look up user: {e}");
                self.current_user_id = None;
                self.compose_body.clear();
                self.screen = Screen::SetName;
            }
        }

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
        self.redraw()?;
        session.channel_success(channel)?;
        Ok(())
    }

    // User pressed a key → raw terminal bytes arrive here. We route them based
    // on the current screen, then redraw.
    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        // Ctrl+C always quits, from any screen (it's not printable text).
        if data.contains(&0x03) {
            session.close(channel)?;
            return Ok(());
        }

        match self.screen {
            Screen::List => {
                // q quits — but only here, never while composing.
                if data == b"q" {
                    session.close(channel)?;
                    return Ok(());
                }
                // n starts a new post (works even if the list is empty).
                if data == b"n" {
                    self.compose_title.clear();
                    self.compose_body.clear();
                    self.compose_field = ComposeField::Title;
                    self.screen = Screen::ComposePost;
                } else {
                    let len = self.posts.len();
                    if len > 0 {
                        let selected = self.list_state.selected().unwrap_or(0);
                        if data == b"\x1b[A" || data == b"\x1bOA" {
                            self.list_state.select(Some(selected.saturating_sub(1)));
                        } else if data == b"\x1b[B" || data == b"\x1bOB" {
                            self.list_state.select(Some((selected + 1).min(len - 1)));
                        } else if data == b"\r" {
                            self.screen = Screen::Detail(selected);
                            self.comments = match crate::db::list_comments(&self.db, self.posts[selected].id).await {
                                Ok(comments) => comments,
                                Err(e) => {
                                    eprintln!("[db] failed to load comments: {e}");
                                    Vec::new()
                                }
                            };
                        }
                    }
                }
            }

            Screen::Detail(i) => {
                if data == b"q" {
                    session.close(channel)?;
                    return Ok(());
                }
                if data == b"b" || data == b"\x1b" {
                    self.screen = Screen::List;
                } else if data == b"c" {
                    // c starts a comment on the post we're viewing.
                    self.compose_body.clear();
                    self.compose_field = ComposeField::Body;
                    self.screen = Screen::ComposeComment(i);
                }
            }

            Screen::ComposePost => {
                if data == b"\x1b" {
                    self.screen = Screen::List; // Esc cancels
                } else if data == b"\x04" {
                    // Ctrl+D submits (requires a non-empty title).
                    if !self.compose_title.trim().is_empty() {
                        let author = self.current_user_id.unwrap_or(1);
                        if let Err(e) = crate::db::insert_post(&self.db, author, &self.compose_title, &self.compose_body).await {
                            eprintln!("[db] failed to insert post: {e}");
                        }
                        self.posts = crate::db::list_posts(&self.db).await.unwrap_or_default();
                        if !self.posts.is_empty() {
                            self.list_state.select(Some(0));
                        }
                    }
                    self.screen = Screen::List;
                } else if data == b"\r" {
                    // Enter: Title field → move to Body; Body field → newline.
                    match self.compose_field {
                        ComposeField::Title => self.compose_field = ComposeField::Body,
                        ComposeField::Body => self.compose_body.push('\n'),
                    }
                } else {
                    self.edit_field(data);
                }
            }

            Screen::ComposeComment(i) => {
                if data == b"\x1b" {
                    self.screen = Screen::Detail(i); // Esc cancels
                } else if data == b"\x04" {
                    // Ctrl+D submits (requires a non-empty body).
                    if !self.compose_body.trim().is_empty() {
                        let post_id = self.posts[i].id;
                        let author = self.current_user_id.unwrap_or(1);
                        if let Err(e) = crate::db::insert_comment(&self.db, post_id, author, &self.compose_body).await {
                            eprintln!("[db] failed to insert comment: {e}");
                        }
                        self.comments = crate::db::list_comments(&self.db, post_id).await.unwrap_or_default();
                    }
                    self.screen = Screen::Detail(i);
                } else if data == b"\r" {
                    self.compose_body.push('\n');
                } else {
                    push_printable(&mut self.compose_body, data);
                }
            }

            Screen::SetName => {
                if data == b"\r" || data == b"\x04" {
                    let name = self.compose_body.trim().to_string();
                    if !name.is_empty() {
                        let fp = self.fingerprint.clone().unwrap_or_default();
                        match crate::db::create_user(&self.db, &fp, &name).await {
                            Ok(id) => {
                                self.current_user_id = Some(id);
                                self.screen = Screen::List;
                            }
                            Err(e) => eprintln!("[db] failed to create user: {e}"),
                        }
                    }
                } else {
                    push_printable(&mut self.compose_body, data);
                }
            }
        }

        self.redraw()?;
        Ok(())
    }

    // User resized their terminal window → resize the viewport and redraw.
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
        }
        self.redraw()?;
        Ok(())
    }
}

// Runs automatically when the handler is freed, used here to log a disconnect.
impl Drop for ClientHandler {
    fn drop(&mut self) {
        println!("[disconnect] client #{}", self.id);
    }
}

/// Append typed bytes to a text buffer: printable ASCII gets pushed,
/// Backspace (0x7f or 0x08) deletes the last character, everything else
/// (control codes, escape sequences) is ignored.
fn push_printable(buf: &mut String, data: &[u8]) {
    for &b in data {
        if b == 0x7f || b == 0x08 {
            buf.pop();
        } else if (0x20..=0x7e).contains(&b) {
            buf.push(b as char);
        }
    }
}
