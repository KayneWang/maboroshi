use crate::app::{App, PlayerStatus};
use crate::ui::theme::{
    make_list_state, selected_style, spinner_frame, style_for_log_line, truncate_text,
    COLOR_NEON_CYAN, COLOR_NEON_GREEN, COLOR_NEON_PINK, COLOR_WARNING,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
    Frame,
};

pub fn render_title(app: &App, frame: &mut Frame, area: Rect) {
    let total_pages_text = if app.total_pages == usize::MAX {
        "?".to_string()
    } else {
        app.total_pages.to_string()
    };
    let loading_badge = if app.is_loading_page || matches!(app.status, PlayerStatus::Searching) {
        format!(" [{} LOADING]", spinner_frame())
    } else {
        String::new()
    };
    let source_badge = app.current_source.to_uppercase();

    let title_text = format!(
        " 🌀 Maboroshi - 幻 | {} [{}] [P{}/{}] [VOL:{}%]{} ",
        app.get_play_mode_text(),
        source_badge,
        app.current_page,
        total_pages_text,
        app.volume,
        loading_badge
    );
    let title = Paragraph::new(title_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_NEON_CYAN))
            .style(Style::default().fg(COLOR_NEON_CYAN)),
    );
    frame.render_widget(title, area);
}

pub fn render_status_and_gauge(app: &App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // 状态文本
            Constraint::Min(1),    // 进度条
        ])
        .split(area);

    let status_text = match &app.status {
        PlayerStatus::Waiting => {
            if app.favorites.is_empty() {
                "💡 按 's' 搜索音乐开始使用".to_string()
            } else {
                "💡 等待播放".to_string()
            }
        }
        PlayerStatus::Searching => format!("{} 正在搜索...", spinner_frame()),
        PlayerStatus::SearchResults => format!("🎯 找到 {} 首", app.search_results.len()),
        PlayerStatus::Playing => format!("▶ {}", app.current_song),
        PlayerStatus::Paused => format!("⏸ {}", app.current_song),
        PlayerStatus::Error(e) => format!("❌ {}", e),
    };

    let gauge_color = match app.status {
        PlayerStatus::Playing => COLOR_NEON_PINK,
        PlayerStatus::Paused => COLOR_WARNING,
        PlayerStatus::Searching => COLOR_NEON_CYAN,
        PlayerStatus::SearchResults => COLOR_NEON_GREEN,
        PlayerStatus::Error(_) => Color::Red,
        PlayerStatus::Waiting => Color::LightBlue,
    };

    let favorite_indicator = if app.is_favorite() { " ⭐" } else { "" };
    let progress_label = if matches!(app.status, PlayerStatus::Playing | PlayerStatus::Paused) {
        let pct = if app.progress.is_finite() {
            app.progress
        } else {
            0.0
        };
        format!("{:.0}%", pct * 100.0)
    } else {
        String::new()
    };

    let status_line = Paragraph::new(format!("{}{}", status_text, favorite_indicator)).block(
        Block::default()
            .title("状态")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(gauge_color)),
    );
    frame.render_widget(status_line, chunks[0]);

    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(gauge_color))
        .percent((app.progress * 100.0).clamp(0.0, 100.0) as u16)
        .label(progress_label);
    frame.render_widget(gauge, chunks[1]);
}

pub fn render_list(app: &mut App, frame: &mut Frame, area: Rect) {
    let list_text_max = area.width.saturating_sub(8) as usize;

    if matches!(app.status, PlayerStatus::SearchResults) && !app.search_results.is_empty() {
        let search_items: Vec<ListItem> = app
            .search_results
            .iter()
            .enumerate()
            .map(|(i, result)| {
                let is_selected = i == app.selected_search_result;
                let style = if is_selected {
                    selected_style()
                } else {
                    Style::default()
                };
                let base = format!("{}. {}", i + 1, result.title);
                let marker = if is_selected { "›" } else { " " };
                ListItem::new(format!(
                    "{} {}",
                    marker,
                    truncate_text(&base, list_text_max)
                ))
                .style(style)
            })
            .collect();

        let search_list = List::new(search_items)
            .block(
                Block::default()
                    .title(format!(
                        "🎯 搜索结果 ({}) - 第 {} 页 | ←→ 上一页/下一页 | ↑↓ 选择 | Enter 播放 | 'f' 收藏",
                        app.search_results.len(),
                        app.current_page
                    ))
                    .border_style(Style::default().fg(COLOR_NEON_CYAN))
                    .borders(Borders::ALL),
            )
            .highlight_style(selected_style());

        let mut list_state = make_list_state(app.selected_search_result);
        frame.render_stateful_widget(search_list, area, &mut list_state);
    } else {
        // 显示收藏列表
        let favorite_items: Vec<ListItem> = app
            .favorites
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let is_playing = item.title == app.current_song
                    && matches!(app.status, PlayerStatus::Playing | PlayerStatus::Paused);
                let is_selected = i == app.selected_favorite;

                let style = if is_selected {
                    selected_style()
                } else if is_playing {
                    Style::default()
                        .fg(COLOR_NEON_GREEN)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                let display_text = if item.source == "yt" {
                    item.title.clone()
                } else {
                    format!("{} [{}]", item.title, item.source)
                };
                let marker = if is_selected {
                    "›"
                } else if is_playing {
                    "▶"
                } else {
                    "♥"
                };

                ListItem::new(format!(
                    "{} {}",
                    marker,
                    truncate_text(&display_text, list_text_max)
                ))
                .style(style)
            })
            .collect();

        let favorites_list = List::new(favorite_items).block(
            Block::default()
                .title(format!(
                    "♥ 收藏列表 ({}) - ↑↓ 选择 | Enter 播放 | 'f' 添加/移除",
                    app.favorites.len()
                ))
                .border_style(Style::default().fg(COLOR_NEON_PINK))
                .borders(Borders::ALL),
        );

        let mut list_state = make_list_state(app.selected_favorite);
        frame.render_stateful_widget(favorites_list, area, &mut list_state);
    }
}

pub fn render_logs(app: &App, frame: &mut Frame, area: Rect) {
    let log_height = area.height.saturating_sub(2) as usize;
    let log_start = app.logs.len().saturating_sub(log_height);
    let log_lines: Vec<Line> = app
        .logs
        .iter()
        .skip(log_start)
        .map(|line| Span::styled(line.clone(), style_for_log_line(line)))
        .map(Line::from)
        .collect();

    let logs = Paragraph::new(Text::from(log_lines)).block(
        Block::default()
            .title("📋 日志")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_NEON_CYAN)),
    );
    frame.render_widget(logs, area);
}

pub fn render_help(app: &App, frame: &mut Frame, area: Rect) {
    let help_text = if app.input_mode {
        let history_hint = if app.search_history.is_empty() {
            String::new()
        } else {
            format!(" | ↑↓ 历史({} 条)", app.search_history.len())
        };
        format!(
            " 输入: {} | Enter 搜索 | Esc 取消{} ",
            app.input_buffer, history_hint
        )
    } else if matches!(app.status, PlayerStatus::SearchResults) {
        " ↑↓ 选择 | ←→ 翻页 | Enter 播放 | f 收藏 | Esc 返回 | q 退出 ".to_string()
    } else if matches!(app.status, PlayerStatus::Playing | PlayerStatus::Paused) {
        " Space 暂停/继续 | ←→ 快退/快进 | +/- 音量 | f 收藏 | m 模式 | s 搜索 | q 退出 "
            .to_string()
    } else {
        " s 搜索 | ↑↓ 选择收藏 | Enter 播放 | f 收藏 | m 模式 | q 退出 ".to_string()
    };

    let help = Paragraph::new(help_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_NEON_CYAN)),
        )
        .style(if app.input_mode {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        });
    frame.render_widget(help, area);
}
