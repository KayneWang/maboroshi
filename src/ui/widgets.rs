use crate::app::{App, PlayerStatus};
use crate::ui::theme::{
    self, selected_style, spinner_frame, style_for_log_line, truncate_text, COLOR_NEON_CYAN,
    COLOR_NEON_PINK,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, Paragraph, Wrap},
    Frame,
};

pub fn render_status_and_gauge(app: &App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // 标题与状态同行
            Constraint::Length(1), // 进度条
        ])
        .margin(1) // 为外围Block留出空间
        .split(area);

    let gauge_color = match app.status {
        PlayerStatus::Playing => theme::COLOR_NEON_PINK,
        PlayerStatus::Paused => theme::COLOR_WARNING,
        PlayerStatus::Searching => theme::COLOR_NEON_CYAN,
        PlayerStatus::SearchResults => theme::COLOR_NEON_GREEN,
        PlayerStatus::Error(_) => Color::Red,
        PlayerStatus::Waiting => theme::COLOR_INACTIVE,
    };

    // --- Header Text ---
    let title_prefix = format!(
        "🌀 Maboroshi | {} [{}] ",
        app.get_play_mode_text(),
        app.current_source.to_uppercase()
    );

    let status_text = match &app.status {
        PlayerStatus::Waiting => "💡 按 's' 搜索音乐".to_string(),
        PlayerStatus::Searching => format!("{} 正在搜索...", spinner_frame()),
        PlayerStatus::SearchResults => format!("🎯 找到 {} 首", app.search_results.len()),
        PlayerStatus::Playing => format!("▶ 正在播放: {}", app.current_song),
        PlayerStatus::Paused => format!("⏸ 暂停: {}", app.current_song),
        PlayerStatus::Error(e) => format!("❌ {}", e),
    };

    let favorite_indicator = if app.is_favorite() { " ⭐" } else { "" };
    let vol_text = format!(" [VOL:{}%]", app.volume);

    let full_status = format!(
        "{}{}{}{}",
        title_prefix, status_text, favorite_indicator, vol_text
    );

    let header_line = Paragraph::new(Span::styled(
        full_status,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ));

    // --- Progress Gauge ---
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

    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(gauge_color))
        .percent((app.progress * 100.0).clamp(0.0, 100.0) as u16)
        .label(Span::styled(
            progress_label,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));

    // --- Container Block ---
    let block = theme::default_block()
        .title(" 控制台 ")
        .border_style(Style::default().fg(gauge_color.clone()));

    frame.render_widget(block, area);
    frame.render_widget(header_line, chunks[0]);
    frame.render_widget(gauge, chunks[1]);
}

pub fn render_groups(app: &mut App, frame: &mut Frame, area: Rect) {
    let group_items: Vec<ListItem> = app
        .groups
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let is_selected = i == app.selected_group;

            let style = if is_selected {
                selected_style()
            } else {
                Style::default().fg(theme::COLOR_INACTIVE)
            };

            let marker = if is_selected { "▶" } else { " " };
            ListItem::new(format!("{} {}", marker, g.name)).style(style)
        })
        .collect();

    let groups_list = List::new(group_items).block(
        theme::default_block()
            .title(" 🗂  分组 (Tab) ")
            .border_style(Style::default().fg(theme::COLOR_NEON_CYAN)),
    );

    let mut list_state = theme::make_list_state(app.selected_group);
    frame.render_stateful_widget(groups_list, area, &mut list_state);
}

pub fn render_items(app: &mut App, frame: &mut Frame, area: Rect) {
    let list_text_max = area.width.saturating_sub(6) as usize;

    if !app.search_results.is_empty() {
        // --- 渲染搜索结果 ---
        let search_items: Vec<ListItem> = app
            .search_results
            .iter()
            .enumerate()
            .map(|(i, result)| {
                let is_selected = i == app.selected_search_result;
                let is_playing = result.title == app.current_song
                    && matches!(app.status, PlayerStatus::Playing | PlayerStatus::Paused);

                let style = if is_selected {
                    selected_style()
                } else if is_playing {
                    Style::default()
                        .fg(theme::COLOR_NEON_GREEN)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                let marker = if is_playing {
                    "▶"
                } else if is_selected {
                    "›"
                } else {
                    " "
                };
                let base = format!("{}. {}", i + 1, result.title);

                ListItem::new(format!(
                    "{} {}",
                    marker,
                    truncate_text(&base, list_text_max)
                ))
                .style(style)
            })
            .collect();

        let search_list = List::new(search_items).block(
            theme::default_block()
                .title(format!(
                    " 🎯 搜索结果 ({}) - 第 {} 页 ",
                    app.search_results.len(),
                    app.current_page
                ))
                .border_style(Style::default().fg(theme::COLOR_NEON_PINK)),
        );

        let mut list_state = theme::make_list_state(app.selected_search_result);
        frame.render_stateful_widget(search_list, area, &mut list_state);
    } else {
        // --- 渲染分组曲目 ---
        let active_items = app.active_items();
        let favorite_items: Vec<ListItem> = active_items
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
                        .fg(theme::COLOR_NEON_GREEN)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                let display_text = if item.source == "yt" {
                    item.title.clone()
                } else {
                    format!("{} [{}]", item.title, item.source)
                };

                let marker = if is_playing {
                    "▶"
                } else if is_selected {
                    "›"
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

        let group_name = app.active_group().name.clone();
        let favorites_list = List::new(favorite_items).block(
            theme::default_block()
                .title(format!(
                    " 🎵 {} ({}) ",
                    group_name,
                    app.active_items().len()
                ))
                .border_style(Style::default().fg(theme::COLOR_NEON_PINK)),
        );

        let mut list_state = theme::make_list_state(app.selected_favorite);
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
    let mut spans = Vec::new();

    // 辅助函数：生成形如 " [Key] Action " 的样式
    let add_bind = |s: &mut Vec<Span<'static>>, key: &str, action: &str| {
        s.push(Span::raw(" "));
        s.push(Span::styled(
            format!("[{}]", key),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));
        s.push(Span::styled(
            format!(" {}", action),
            Style::default().fg(Color::Gray),
        ));
    };

    let border_color = if app.delete_confirm_mode {
        spans.push(Span::styled(
            format!(
                " ⚠️  确认删除分组「{}」及其 {} 首收藏？ ",
                app.active_group().name,
                app.active_items().len()
            ),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
        add_bind(&mut spans, "y", "确认");
        add_bind(&mut spans, "Esc", "取消");
        Color::Red
    } else if app.rename_mode {
        spans.push(Span::styled(
            format!(" 重命名分组: {} ", app.input_buffer),
            Style::default().fg(Color::Yellow),
        ));
        add_bind(&mut spans, "Enter", "确认");
        add_bind(&mut spans, "Esc", "取消");
        theme::COLOR_NEON_CYAN
    } else if app.move_mode {
        spans.push(Span::styled(
            " 移动到: ",
            Style::default().fg(Color::Yellow),
        ));
        add_bind(&mut spans, "↑↓", "切换分组");
        add_bind(&mut spans, "Enter", "确认");
        add_bind(&mut spans, "Esc", "取消");
        theme::COLOR_NEON_CYAN
    } else if app.group_input_mode {
        spans.push(Span::styled(
            format!(" 新建分组: {} ", app.input_buffer),
            Style::default().fg(Color::Yellow),
        ));
        add_bind(&mut spans, "Enter", "确认");
        add_bind(&mut spans, "Esc", "取消");
        theme::COLOR_NEON_CYAN
    } else if app.input_mode {
        let history_hint = if app.search_history.is_empty() {
            String::new()
        } else {
            format!(" ({} 历史记录)", app.search_history.len())
        };
        spans.push(Span::styled(
            format!(" 输入搜索: {} ", app.input_buffer),
            Style::default().fg(Color::Yellow),
        ));
        add_bind(&mut spans, "Enter", "搜索");
        if !app.search_history.is_empty() {
            add_bind(&mut spans, "↑↓", &format!("历史{}", history_hint));
        }
        add_bind(&mut spans, "Esc", "取消");
        theme::COLOR_NEON_CYAN
    } else if !app.search_results.is_empty() {
        if matches!(app.status, PlayerStatus::Playing | PlayerStatus::Paused) {
            add_bind(&mut spans, "Space", "暂停/继续");
        }
        add_bind(&mut spans, "↑↓", "选择");
        add_bind(&mut spans, "←→", "翻页");
        add_bind(&mut spans, "Enter", "播放");
        add_bind(&mut spans, "f", "收藏");
        add_bind(&mut spans, "F", "全部收藏");
        add_bind(&mut spans, "Esc", "返回");
        add_bind(&mut spans, "q", "退出");
        theme::COLOR_NEON_CYAN
    } else {
        if matches!(app.status, PlayerStatus::Playing | PlayerStatus::Paused) {
            add_bind(&mut spans, "Space", "暂停/继续");
            add_bind(&mut spans, "←→", "快退/快进");
            add_bind(&mut spans, "+/-", "音量");
        }
        add_bind(&mut spans, "s", "搜索");
        add_bind(&mut spans, "q", "退出");
        add_bind(&mut spans, "?", "操作帮助");
        theme::COLOR_NEON_CYAN
    };

    let help = Paragraph::new(Line::from(spans))
        .block(
            theme::default_block()
                .title(" ⌨️ 快捷键 ")
                .border_style(Style::default().fg(border_color)),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(help, area);
}

/// 移动模式下的分组选择浮层
pub fn render_move_overlay(app: &App, frame: &mut Frame) {
    if !app.move_mode {
        return;
    }
    // 计算浮层大小：宽 40，高 = 分组数 + 4
    let height = (app.groups.len() as u16 + 4).min(frame.size().height);
    let width = 44u16.min(frame.size().width);
    let x = (frame.size().width.saturating_sub(width)) / 2;
    let y = (frame.size().height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    // 先用 Clear 清空背景，避免透溏
    frame.render_widget(Clear, popup_area);

    let item_label = app
        .active_items()
        .get(app.selected_favorite)
        .map(|i| truncate_text(&i.title, 30))
        .unwrap_or_default();

    let items: Vec<ListItem> = app
        .groups
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != app.selected_group) // 过滤掉当前分组
        .map(|(i, g)| {
            let is_target = i == app.move_target_group;
            let marker = if is_target { "›" } else { " " };
            let style = if is_target {
                Style::default()
                    .fg(COLOR_NEON_PINK)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(format!("{} {}", marker, g.name)).style(style)
        })
        .collect();

    let popup = List::new(items).block(
        Block::default()
            .title(format!("移动「{}」到", item_label))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_NEON_PINK)),
    );
    frame.render_widget(popup, popup_area);
}

pub fn render_help_overlay(app: &App, frame: &mut Frame) {
    if !app.help_mode {
        return;
    }

    let help_text = vec![
        Line::from(Span::styled("【全局操作】", Style::default().fg(theme::COLOR_NEON_PINK).add_modifier(Modifier::BOLD))),
        Line::from(" [q] 退出程序        [s] 搜索网络歌曲        [?] 打开/关闭帮助        [m] 切换播放模式"),
        Line::from(""),
        Line::from(Span::styled("【播放控制】", Style::default().fg(theme::COLOR_NEON_PINK).add_modifier(Modifier::BOLD))),
        Line::from(" [Space] 暂停/继续   [Enter] 播放选定歌曲    [←/→] 快退/快进      [+/-] 调节音量"),
        Line::from(""),
        Line::from(Span::styled("【列表 & 分组】", Style::default().fg(theme::COLOR_NEON_PINK).add_modifier(Modifier::BOLD))),
        Line::from(" [↑/↓] 上下移动      [Tab/Shift+Tab] 切换上下分组"),
        Line::from(" [g] 新建分组        [R] 重命名当前分组      [D] 删除当前分组"),
        Line::from(" [M] 移动当前歌曲    [f] 收藏/取消收藏       [F] 收藏搜索列表所有歌曲"),
        Line::from(""),
    ];

    let height = (help_text.len() as u16 + 2).min(frame.size().height);
    let width = 86u16.min(frame.size().width);
    let x = (frame.size().width.saturating_sub(width)) / 2;
    let y = (frame.size().height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, popup_area);

    let popup = Paragraph::new(help_text).block(
        theme::default_block()
            .title(" 全部快捷键说明 ")
            .border_style(Style::default().fg(theme::COLOR_NEON_CYAN)),
    );
    frame.render_widget(popup, popup_area);
}
