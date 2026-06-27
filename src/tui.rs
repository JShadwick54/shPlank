//! The terminal/rendering layer: turning ratatui's output into bytes on the
//! SSH channel... and screen-drawing code itself.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use russh::ChannelId;
use russh::server::Handle;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::db::Post;

/// Bridges ratatui to the SSH channel.
///
/// ratatui only knows how to write bytes to "something that implements
/// `std::io::Write`". Normally that's your local terminal; here it's this
/// adapter, which forwards those bytes over SSH to the connected client.
pub struct TerminalHandle {
    // A queue sender.
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

/// Describes the entire screen for one frame. ratatui calls this, diffs the
/// result against what's already on the client's screen, and sends only the
/// bytes that changed.
/// Describes the screen for one frame: the posts as a vertical list.
pub fn draw_ui(frame: &mut ratatui::Frame, posts: &[Post], list_state: &mut ListState, detail: Option<&Post>) {
    match detail {
        None => draw_list(frame, posts, list_state),
        Some(post) => draw_detail(frame, post),
    }
}

fn draw_list(frame: &mut ratatui::Frame, posts: &[Post], list_state: &mut ListState) {
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

fn draw_detail(frame: &mut ratatui::Frame, post: &Post) {
    let title_line = Line::from(vec![
        Span::styled(&post.title, Style::default().add_modifier(Modifier::BOLD)),
    ]);

    let content = vec![
        title_line,
        Line::raw(""),
        Line::raw(&post.body),
        Line::raw(""),
        Line::styled("[ b ] back", Style::default().fg(Color::DarkGray)),
    ];

    let paragraph = Paragraph::new(content)
        .block(
            Block::default()
                .title(" shPlank ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, frame.area());
}
