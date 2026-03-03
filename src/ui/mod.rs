mod theme;
mod widgets;

use crate::app::App;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    Frame,
};

pub fn render(app: &mut App, frame: &mut Frame) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // 标题
            Constraint::Length(4), // 播放状态 + 进度条
            Constraint::Min(8),    // 主体区域（列表 + 日志）
            Constraint::Length(3), // 帮助栏
        ])
        .split(frame.size()); // ratatui 0.26.x：frame.area() 在 0.28+ 才可用

    let body_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(70), // 列表区域
            Constraint::Percentage(30), // 日志区域
        ])
        .split(chunks[2]);

    widgets::render_title(app, frame, chunks[0]);
    widgets::render_status_and_gauge(app, frame, chunks[1]);
    widgets::render_list(app, frame, body_chunks[0]);
    widgets::render_logs(app, frame, body_chunks[1]);
    widgets::render_help(app, frame, chunks[3]);
}
