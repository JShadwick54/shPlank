//! The rendering layer: the `TerminalHandle` byte-bridge that carries ratatui's
//! output over the SSH channel, plus the per-screen `draw_*` functions.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use russh::ChannelId;
use russh::server::Handle;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::db::{Comment, Post};

/// Which field of the post composer currently has focus.
#[derive(Copy, Clone, PartialEq)]
pub enum ComposeField {
    Title,
    Body,
}

/// Bridges ratatui to the SSH channel.
///
/// ratatui only knows how to write bytes to "something that implements
/// `std::io::Write`". Normally that's your local terminal; here it's this
/// adapter, which forwards those bytes over SSH to the connected client.
pub struct TerminalHandle {
    // A queue sender. `flush` drops a frame's bytes here; the background task
    // (spawned in `start`) pulls them off and ships them over SSH.
    sender: UnboundedSender<Vec<u8>>,
    // ratatui writes output in many small pieces; we accumulate them here and
    // send the whole batch at once when `flush` is called (once per frame).
    sink: Vec<u8>,
}

impl TerminalHandle {
    /// Build the bridge and spawn the background task that pumps bytes to the
    /// client over `channel_id`.
    pub fn start(handle: Handle, channel_id: ChannelId) -> Self {
        let (sender, mut receiver) = unbounded_channel::<Vec<u8>>();

        // tokio::spawn = "run this concurrently on the runtime". The task loops
        // until the sender half is dropped — which happens automatically when
        // the connection closes and this TerminalHandle is freed — then exits.
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

// Implementing `std::io::Write` is exactly what lets ratatui treat this as a
// terminal it can render into.
impl std::io::Write for TerminalHandle {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // Just accumulate; nothing leaves until `flush`.
        self.sink.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // `std::mem::take` swaps `sink` for an empty Vec and hands back the old
        // contents — so we move the buffer onto the queue without copying it,
        // leaving `sink` empty and ready for the next frame.
        let data = std::mem::take(&mut self.sink);
        self.sender
            .send(data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e))?;
        Ok(())
    }
}

/// The post list — one row per post, with the selected row highlighted.
pub fn draw_list(frame: &mut ratatui::Frame, posts: &[Post], list_state: &mut ListState) {
    let items: Vec<ListItem> = posts
        .iter()
        .map(|p| ListItem::new(p.title.clone()))
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" shPlank ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan))
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, frame.area(), list_state);
}

/// A single post: title, author, body, and its comments below.
pub fn draw_detail(frame: &mut ratatui::Frame, post: &Post, comments: &[Comment]) {
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(&post.title, Style::default().add_modifier(Modifier::BOLD))),
        Line::styled(format!("by {}", post.author_name), Style::default().fg(Color::DarkGray)),
        Line::raw(""),
    ];

    // Split body on newlines so paragraphs render as separate lines.
    for line in post.body.split('\n') {
        lines.push(Line::raw(line.to_owned()));
    }

    if comments.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled("No comments yet.", Style::default().fg(Color::DarkGray)));
    } else {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            format!("── {} comment{} ──", comments.len(), if comments.len() == 1 { "" } else { "s" }),
            Style::default().fg(Color::DarkGray),
        ));
        for comment in comments {
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                format!("{}:", comment.author_name),
                Style::default().fg(Color::Cyan),
            ));
            for line in comment.body.split('\n') {
                lines.push(Line::raw(line.to_owned()));
            }
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::styled("[ b ] back", Style::default().fg(Color::DarkGray)));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" shPlank ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, frame.area());
}

/// The new-post composer: a Title field and a Body field; `field` marks focus.
pub fn draw_compose_post(frame: &mut ratatui::Frame, title: &str, body: &str, field: ComposeField) {
    let mut lines: Vec<Line> = Vec::new();

    let title_label = if field == ComposeField::Title { "Title ▶" } else { "Title :" };
    lines.push(Line::styled(title_label, Style::default().fg(Color::DarkGray)));
    lines.push(Line::raw(title.to_owned()));
    lines.push(Line::raw(""));

    let body_label = if field == ComposeField::Body { "Body ▶" } else { "Body :" };
    lines.push(Line::styled(body_label, Style::default().fg(Color::DarkGray)));
    for l in body.split('\n') {
        lines.push(Line::raw(l.to_owned()));
    }

    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "[ Enter ] next/newline   [ Ctrl+D ] submit   [ Esc ] cancel",
        Style::default().fg(Color::DarkGray),
    ));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" New Post ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, frame.area());
}

/// The new-comment composer: a single body field.
pub fn draw_compose_comment(frame: &mut ratatui::Frame, body: &str) {
    let mut lines: Vec<Line> = Vec::new();
    for l in body.split('\n') {
        lines.push(Line::raw(l.to_owned()));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "[ Enter ] newline   [ Ctrl+D ] submit   [ Esc ] cancel",
        Style::default().fg(Color::DarkGray),
    ));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" New Comment ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, frame.area());
}

/// First-connect screen where a new visitor types their display name.
pub fn draw_set_name(frame: &mut ratatui::Frame, name: &str) {
    let lines: Vec<Line> = vec![
        Line::styled("Welcome to shPlank!", Style::default().add_modifier(Modifier::BOLD)),
        Line::raw(""),
        Line::raw("Choose a display name (shown on your posts and comments):"),
        Line::raw(""),
        Line::styled(format!("▶ {}", name), Style::default().fg(Color::Cyan)),
        Line::raw(""),
        Line::styled(
            "[ Enter ] confirm   [ Ctrl+C ] disconnect",
            Style::default().fg(Color::DarkGray),
        ),
    ];

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" New User ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, frame.area());
}
