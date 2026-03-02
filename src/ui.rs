use crate::app::App;
use crate::app::PlayerStatus;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph},
    Frame,
};
use std::time::{SystemTime, UNIX_EPOCH};

const COLOR_NEON_CYAN: Color = Color::Rgb(0, 230, 255);
const COLOR_NEON_PINK: Color = Color::Rgb(255, 80, 200);
const COLOR_NEON_GREEN: Color = Color::Rgb(120, 255, 120);
const COLOR_BG_HIGHLIGHT: Color = Color::Rgb(35, 35, 55);
const COLOR_WARNING: Color = Color::Rgb(255, 190, 90);

fn spinner_frame() -> &'static str {
    const FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
    let tick = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        / 120;
    FRAMES[(tick as usize) % FRAMES.len()]
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    if max_chars <= 1 {
        return "…".to_string();
    }
    let head: String = text.chars().take(max_chars - 1).collect();
    format!("{}…", head)
}

fn style_for_log_line(line: &str) -> Style {
    if line.contains("失败") || line.contains("错误") || line.contains('❌') {
        Style::default().fg(Color::Red)
    } else if line.contains("警告") || line.contains("超时") {
        Style::default().fg(COLOR_WARNING)
    } else if line.contains('✓') || line.contains("成功") || line.contains("就绪") {
        Style::default().fg(COLOR_NEON_GREEN)
    } else {
        Style::default().fg(Color::Gray)
    }
}

pub fn render(app: &mut App, frame: &mut Frame) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // 标题
            Constraint::Length(4), // 播放状态 + 进度条
            Constraint::Min(8),    // 主体区域（列表 + 日志）
            Constraint::Length(3), // 帮助栏
        ])
        .split(frame.size());

    let body_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(70), // 列表区域
            Constraint::Percentage(30), // 日志区域
        ])
        .split(chunks[2]);

    let status_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // 状态文本
            Constraint::Min(1),    // 进度条
        ])
        .split(chunks[1]);

    let list_text_max = body_chunks[0].width.saturating_sub(8) as usize;

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
    frame.render_widget(title, chunks[0]);

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
        format!("{:.0}%", app.progress * 100.0)
    } else {
        String::new()
    };

    let status_line = Paragraph::new(format!("{}{}", status_text, favorite_indicator)).block(
        Block::default()
            .title("状态")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(gauge_color)),
    );
    frame.render_widget(status_line, status_chunks[0]);

    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(gauge_color))
        .percent((app.progress * 100.0).clamp(0.0, 100.0) as u16)
        .label(progress_label);
    frame.render_widget(gauge, status_chunks[1]);

    // 列表区域：搜索结果或收藏列表
    if matches!(app.status, PlayerStatus::SearchResults) && !app.search_results.is_empty() {
        let search_items: Vec<ListItem> = app
            .search_results
            .iter()
            .enumerate()
            .map(|(i, result)| {
                let is_selected = i == app.selected_search_result;
                let style = if is_selected {
                    Style::default()
                        .fg(Color::White)
                        .bg(COLOR_BG_HIGHLIGHT)
                        .add_modifier(Modifier::BOLD)
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
            .highlight_style(
                Style::default()
                    .fg(Color::White)
                    .bg(COLOR_BG_HIGHLIGHT)
                    .add_modifier(Modifier::BOLD),
            );

        let mut list_state = ListState::default();
        list_state.select(Some(app.selected_search_result));
        frame.render_stateful_widget(search_list, body_chunks[0], &mut list_state);
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
                    Style::default()
                        .fg(Color::White)
                        .bg(COLOR_BG_HIGHLIGHT)
                        .add_modifier(Modifier::BOLD)
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

        let mut list_state = ListState::default();
        list_state.select(Some(app.selected_favorite));
        frame.render_stateful_widget(favorites_list, body_chunks[0], &mut list_state);
    }

    // 日志区域
    let log_height = body_chunks[1].height.saturating_sub(2) as usize;
    let log_start = if app.logs.len() > log_height {
        app.logs.len() - log_height
    } else {
        0
    };
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
    frame.render_widget(logs, body_chunks[1]);

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
    frame.render_widget(help, chunks[3]);
}
