use std::{any, io};

use crossterm::event::KeyCode;
use futures::{
    channel::mpsc::{Receiver, Sender},
    SinkExt, StreamExt,
};
use rusty_pipe::youtube_extractor::{
    search_extractor::YTSearchItem, stream_extractor::YTStreamExtractor,
};
use unicode_width::UnicodeWidthStr;

use crate::{
    server::schema::{PlayOptions, PlayerMessage, PlayerStatus, ToPlayerMessages},
    yt_downloader::YTDownloader,
};

use async_std::prelude::*;
use tui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans, Text},
    widgets::{Block, Borders, Gauge, LineGauge, List, ListItem, Paragraph},
    Terminal,
};
mod util;

enum InputMode {
    Normal,
    Editing,
}

enum IMsg {
    PlayerData(PlayerMessage),
    CrossTermEvent(Result<crossterm::event::Event, std::io::Error>),
}
/// App holds the state of the application
struct App {
    /// Current value of the input box
    input: String,
    /// Current input mode
    input_mode: InputMode,
    /// History of recorded messages
    results: Vec<rusty_pipe::youtube_extractor::search_extractor::YTSearchItem>,

    selected_result: Option<YTSearchItem>,

    player_status: Option<PlayerStatus>,
}

impl Default for App {
    fn default() -> App {
        App {
            input: String::new(),
            input_mode: InputMode::Normal,
            results: Vec::new(),
            selected_result: None,
            player_status: None,
        }
    }
}

pub async fn run_tui_pipe(
    mut msg_receiver: Receiver<PlayerMessage>,
    mut msg_sender: Sender<ToPlayerMessages>,
) {
    let receiver_msg_stream = msg_receiver.map(|e| IMsg::PlayerData(e));
    let mut stdout = io::stdout();

    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)
        .expect("Cant enter alternate screen");
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).expect("Cant create terminal");

    // Setup event handlers
    let mut input_events_stream = crossterm::event::EventStream::new();
    let msg_stream = input_events_stream.map(|e| IMsg::CrossTermEvent(e));
    let mut mixed_stream = futures::stream_select!(receiver_msg_stream, msg_stream);
    // Create default app state
    let mut app = App::default();
    crossterm::terminal::enable_raw_mode();
    loop {
        // Draw UI
        terminal.clear().expect("Cant clear terminal");
        terminal
            .draw(|f| {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(2)
                    .constraints(
                        [
                            Constraint::Length(1),
                            Constraint::Length(3),
                            Constraint::Min(1),
                            Constraint::Length(1),
                        ]
                        .as_ref(),
                    )
                    .split(f.size());

                let (msg, style) = match app.input_mode {
                    InputMode::Normal => (
                        vec![
                            Span::raw("Press "),
                            Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(" to exit, "),
                            Span::styled("e", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(" to start editing."),
                        ],
                        Style::default().add_modifier(Modifier::DIM),
                    ),
                    InputMode::Editing => (
                        vec![
                            Span::raw("Press "),
                            Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(" to stop editing, "),
                            Span::styled("Enter", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(" to search"),
                        ],
                        Style::default(),
                    ),
                };
                let mut text = Text::from(Spans::from(msg));
                text.patch_style(style);
                let help_message = Paragraph::new(text);
                f.render_widget(help_message, chunks[0]);

                let input = Paragraph::new(app.input.as_ref())
                    .style(match app.input_mode {
                        InputMode::Normal => Style::default(),
                        InputMode::Editing => Style::default().fg(Color::Yellow),
                    })
                    .block(Block::default().borders(Borders::ALL).title("Search"));
                f.render_widget(input, chunks[1]);
                match app.input_mode {
                    InputMode::Normal =>
                        // Hide the cursor. `Frame` does this by default, so we don't need to do anything here
                        {}

                    InputMode::Editing => {
                        // Make the cursor visible and ask tui-rs to put it at the specified coordinates after rendering
                        f.set_cursor(
                            // Put cursor past the end of the input text
                            chunks[1].x + app.input.width() as u16 + 1,
                            // Move one line down, from the border to the input line
                            chunks[1].y + 1,
                        )
                    }
                }

                let messages: Vec<ListItem> = app
                    .results
                    .iter()
                    .enumerate()
                    .map(|(i, m)| {
                        let content = vec![Spans::from(Span::raw(format!(
                            "{}: {}",
                            i,
                            match m {
                                YTSearchItem::StreamInfoItem(info) =>
                                    info.get_name().unwrap_or("Name error".into()),
                                YTSearchItem::ChannelInfoItem(info) =>
                                    info.get_name().unwrap_or("Name error".into()),
                                YTSearchItem::PlaylistInfoItem(info) =>
                                    info.get_name().unwrap_or("Name error".into()),
                            }
                        )))];
                        let mut item = ListItem::new(content);
                        if let Some(selected_item) = &app.selected_result {
                            if selected_item == m {
                                item =
                                    item.style(Style::default().bg(Color::White).fg(Color::Black))
                            }
                        }
                        item
                    })
                    .collect();
                let messages = List::new(messages)
                    .block(Block::default().borders(Borders::ALL).title("Results"));
                f.render_widget(messages, chunks[2]);

                let player_row = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Length(15),
                        Constraint::Min(1),
                        Constraint::Length(15),
                    ])
                    .split(chunks[3]);
                let progress = Gauge::default()
                    .block(Block::default().borders(Borders::empty()))
                    .gauge_style(Style::default().fg(Color::White).bg(Color::Black))
                    .label("")
                    .percent({
                        if let Some(data) = &app.player_status {
                            ((data.current_status.unwrap_or(0) as f32
                                / data.total_time.unwrap_or(1) as f32)
                                * 100.0)
                                .clamp(0.0, 100.0) as u16
                        } else {
                            0
                        }
                    });
                let left_prog = if let Some(pd) = &app.player_status {
                    if let Some((cur, dur)) = pd.current_status.zip(pd.total_time) {
                        let secs = cur % 60;
                        let mins = cur / 60;
                        let hours = cur / (60 * 60);
                        format!(
                            "{} {}:{:0>2}:{:0>2}",
                            if pd.playing { "⏵" } else { "⏸︎" },
                            hours,
                            mins,
                            secs
                        )
                    } else {
                        format!("")
                    }
                } else {
                    format!("Not Playing")
                };
                let right_prog = if let Some(pd) = &app.player_status {
                    if let Some((cur, dur)) = pd.current_status.zip(pd.total_time) {
                        let secs = dur % 60;
                        let mins = dur / 60;
                        let hours = dur / (60 * 60);
                        format!("{}:{:0>2}:{:0>2}", hours, mins, secs)
                    } else {
                        format!("")
                    }
                } else {
                    format!("")
                };
                let progress_text_left =
                    Paragraph::new(vec![Spans::from(vec![Span::raw(&left_prog)])]);
                let progress_text_right =
                    Paragraph::new(vec![Spans::from(vec![Span::raw(&right_prog)])])
                        .alignment(tui::layout::Alignment::Right);

                f.render_widget(progress_text_left, player_row[0]);
                f.render_widget(progress, player_row[1]);
                f.render_widget(progress_text_right, player_row[2]);
            })
            .expect("Cant render");

        // Handle input

        let data = mixed_stream.next().await;
        if let Some(data) = data {
            match data {
                IMsg::PlayerData(pd) => match pd {
                    PlayerMessage::Status(status) => {
                        app.player_status = Some(status);
                    }
                },
                IMsg::CrossTermEvent(event) => match event {
                    Ok(event) => match event {
                        crossterm::event::Event::Key(key) => match app.input_mode {
                            InputMode::Normal => match &key.code {
                                KeyCode::Char('e') => {
                                    app.input_mode = InputMode::Editing;
                                }
                                KeyCode::Char('q') => {
                                    break;
                                }
                                KeyCode::Char(' ') => {
                                    if let Some(status) = &app.player_status {
                                        if status.playing {
                                            msg_sender.send(ToPlayerMessages::Pause).await;
                                        } else {
                                            msg_sender.send(ToPlayerMessages::Resume).await;
                                        }
                                    } else {
                                        msg_sender.send(ToPlayerMessages::Resume).await;
                                    }
                                }
                                KeyCode::Enter => {
                                    if let Some(item) = &app.selected_result {
                                        if let YTSearchItem::StreamInfoItem(video) = item {
                                            if let Ok(video_id) = video.video_id() {
                                                if let Ok((url, size)) = play_video(&video_id).await
                                                {
                                                    let mut cache_dir = dirs::audio_dir();
                                                    let mut path = None;
                                                    if let Some(dir) = &mut cache_dir {
                                                        dir.push("RustyPipe");

                                                        let res =
                                                            async_std::fs::create_dir_all(&dir)
                                                                .await;

                                                        dir.push(format!("{}", video_id));
                                                        dir.set_extension("m4a");
                                                        match res {
                                                            Ok(_) => {
                                                                path = dir
                                                                    .to_str()
                                                                    .to_owned()
                                                                    .map(|f| f.to_string());
                                                            }
                                                            Err(err) => {
                                                                log::error!(
                                                                    "Cant create cache dir {:#?}",
                                                                    err
                                                                )
                                                            }
                                                        }
                                                    }
                                                    msg_sender
                                                        .send(ToPlayerMessages::Play(PlayOptions {
                                                            url,
                                                            length: size,
                                                            video_id,
                                                            file_path: path,
                                                        }))
                                                        .await;
                                                }
                                            }
                                        }
                                    }
                                }
                                KeyCode::Down => {
                                    if let Some(selected_item) = &app.selected_result {
                                        let i =
                                            app.results.iter().position(|it| it == selected_item);
                                        if let Some(i) = i {
                                            if let Some(item) = app.results.get(i + 1) {
                                                app.selected_result = Some(item.clone())
                                            }
                                        }
                                    }
                                }
                                KeyCode::Up => {
                                    if let Some(selected_item) = &app.selected_result {
                                        let i =
                                            app.results.iter().position(|it| it == selected_item);
                                        if let Some(i) = i {
                                            if i > 0 {
                                                if let Some(item) = app.results.get(i - 1) {
                                                    app.selected_result = Some(item.clone())
                                                }
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            },
                            InputMode::Editing => match key.code {
                                KeyCode::Enter => {
                                    let query = app.input.drain(..).collect::<String>();
                                    let extractor = rusty_pipe::youtube_extractor::search_extractor::YTSearchExtractor::new::<YTDownloader>(&query, None).await.expect("Cant create search extractor");
                                    let items = extractor.search_results();
                                    if let Ok(items) = items {
                                        app.results = items
                                            .into_iter()
                                            .filter(|f| {
                                                if let YTSearchItem::StreamInfoItem(_) = f {
                                                    true
                                                } else {
                                                    false
                                                }
                                            })
                                            .collect();
                                        app.selected_result =
                                            app.results.get(0).and_then(|f| Some(f.clone()));
                                        app.input_mode = InputMode::Normal;
                                    }
                                }
                                KeyCode::Char(c) => {
                                    app.input.push(c);
                                }
                                KeyCode::Backspace => {
                                    app.input.pop();
                                }
                                KeyCode::Esc => {
                                    app.input_mode = InputMode::Normal;
                                }

                                KeyCode::Down => {
                                    if let Some(selected_item) = &app.selected_result {
                                        let i =
                                            app.results.iter().position(|it| it == selected_item);
                                        if let Some(i) = i {
                                            if let Some(item) = app.results.get(i + 1) {
                                                app.selected_result = Some(item.clone())
                                            }
                                        }
                                    }
                                }
                                KeyCode::Up => {
                                    if let Some(selected_item) = &app.selected_result {
                                        let i =
                                            app.results.iter().position(|it| it == selected_item);
                                        if let Some(i) = i {
                                            if i > 0 {
                                                if let Some(item) = app.results.get(i - 1) {
                                                    app.selected_result = Some(item.clone())
                                                }
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            },
                        },
                        crossterm::event::Event::Mouse(_) => {}
                        crossterm::event::Event::Resize(_, _) => {}
                    },
                    Err(err) => log::warn!("{:#?}", err),
                },
            }
        }
    }

    crossterm::terminal::disable_raw_mode();

    terminal.clear();
    crossterm::execute!(io::stdout(), crossterm::terminal::EnterAlternateScreen);
}

pub async fn play_video(id: &str) -> Result<(String, Option<usize>), anyhow::Error> {
    let mut stream_extractor = YTStreamExtractor::new(id, YTDownloader {})
        .await
        .map_err(|e| anyhow::anyhow!("{:#?}", e))?;
    let audio_streams = stream_extractor
        .get_audio_streams()
        .map_err(|er| anyhow::anyhow!("Cant get audio streams"))?;
    let stream_info = audio_streams
        .iter()
        .filter(|f| f.mimeType.contains("mp4"))
        .nth(0)
        .ok_or(anyhow::anyhow!("No m4a stream found"))?;
    let url = stream_info
        .url
        .clone()
        .ok_or(anyhow::anyhow!("No url in stream"))?;

    let response = surf::get(&url).send().await.unwrap();
    let length = response.len();
    Ok((url, length))
}
