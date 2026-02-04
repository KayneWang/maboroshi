use crate::app::App;
use crate::app::PlayerStatus;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph},
    Frame,
};

pub fn render(app: &mut App, frame: &mut Frame) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // 标题
            Constraint::Length(3),  // 播放状态和进度条（单行）
            Constraint::Min(10),    // 列表区域（自适应）
            Constraint::Length(10), // 日志区域
            Constraint::Length(3),  // 帮助栏
        ])
        .split(frame.size());

    let title_text = format!(" 🌀 Maboroshi - 幻 | {} ", app.get_play_mode_text());
    let title = Paragraph::new(title_text).block(Block::default().borders(Borders::ALL));
    frame.render_widget(title, chunks[0]);

    let status_text = match &app.status {
        PlayerStatus::Waiting => {
            if app.favorites.is_empty() {
                "💡 按 's' 搜索音乐开始使用".to_string()
            } else {
                "💡 等待播放".to_string()
            }
        }
        PlayerStatus::Searching => "🔍 正在搜索...".to_string(),
        PlayerStatus::SearchResults => format!("🎯 找到 {} 首", app.search_results.len()),
        PlayerStatus::Playing => format!("▶ {}", app.current_song),
        PlayerStatus::Paused => format!("⏸ {}", app.current_song),
        PlayerStatus::Error(e) => format!("❌ {}", e),
    };

    let gauge_color = match app.status {
        PlayerStatus::Playing => Color::Magenta,
        PlayerStatus::Paused => Color::Yellow,
        PlayerStatus::Searching => Color::Cyan,
        PlayerStatus::SearchResults => Color::Green,
        PlayerStatus::Error(_) => Color::Red,
        PlayerStatus::Waiting => Color::Blue,
    };

    let favorite_indicator = if app.is_favorite() { " ⭐" } else { "" };
    let progress_label = if matches!(app.status, PlayerStatus::Playing | PlayerStatus::Paused) {
        format!("{:.0}%", app.progress * 100.0)
    } else {
        String::new()
    };

    let gauge = Gauge::default()
        .block(
            Block::default()
                .title(format!("{}{}", status_text, favorite_indicator))
                .borders(Borders::ALL),
        )
        .gauge_style(Style::default().fg(gauge_color))
        .percent((app.progress * 100.0) as u16)
        .label(progress_label);

    frame.render_widget(gauge, chunks[1]);

    // 列表区域：搜索结果或收藏列表
    if matches!(app.status, PlayerStatus::SearchResults) && !app.search_results.is_empty() {
        let search_items: Vec<ListItem> = app
            .search_results
            .iter()
            .enumerate()
            .map(|(i, result)| {
                let style = if i == app.selected_search_result {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(format!("{}. {}", i + 1, result.title)).style(style)
            })
            .collect();

        let search_list = List::new(search_items)
            .block(
                Block::default()
                    .title(format!(
                        "🎯 搜索结果 ({}) - ↑↓ 选择 | Enter 播放 | 'f' 收藏",
                        app.search_results.len()
                    ))
                    .borders(Borders::ALL),
            )
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );

        let mut list_state = ListState::default();
        list_state.select(Some(app.selected_search_result));
        frame.render_stateful_widget(search_list, chunks[2], &mut list_state);
    } else {
        // 显示收藏列表
        let favorite_items: Vec<ListItem> = app
            .favorites
            .iter()
            .enumerate()
            .map(|(i, song)| {
                let is_playing = song == &app.current_song
                    && matches!(app.status, PlayerStatus::Playing | PlayerStatus::Paused);
                let is_selected = i == app.selected_favorite;

                let style = if is_playing {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                let prefix = if is_playing { "▶ " } else { "♥ " };
                ListItem::new(format!("{}{}", prefix, song)).style(style)
            })
            .collect();

        let favorites_list = List::new(favorite_items).block(
            Block::default()
                .title(format!(
                    "♥ 收藏列表 ({}) - ↑↓ 选择 | Enter 播放 | 'f' 添加/移除",
                    app.favorites.len()
                ))
                .borders(Borders::ALL),
        );

        let mut list_state = ListState::default();
        list_state.select(Some(app.selected_favorite));
        frame.render_stateful_widget(favorites_list, chunks[2], &mut list_state);
    }

    // 日志区域
    let log_height = chunks[3].height.saturating_sub(2) as usize;
    let log_start = if app.logs.len() > log_height {
        app.logs.len() - log_height
    } else {
        0
    };
    let recent_logs = &app.logs[log_start..];
    let log_text = recent_logs.join("\n");

    let logs = Paragraph::new(log_text)
        .block(Block::default().title("📋 日志").borders(Borders::ALL))
        .style(Style::default().fg(Color::Gray));
    frame.render_widget(logs, chunks[3]);

    let help_text = if app.input_mode {
        format!(" 搜索: {} (按 Enter 确认 | Esc 取消)", app.input_buffer)
    } else {
        " 'q' 退出 | 's' 搜索 | 'f' 收藏 | 'm' 切换模式 | '↑↓' 选择 | 'Enter' 播放 | 'space' 暂停 "
            .to_string()
    };

    let help = Paragraph::new(help_text)
        .block(Block::default().borders(Borders::ALL))
        .style(if app.input_mode {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        });
    frame.render_widget(help, chunks[4]);
}
