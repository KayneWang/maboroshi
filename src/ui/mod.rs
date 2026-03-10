mod theme;
mod widgets;

use crate::app::App;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    Frame,
};

pub fn render(app: &mut App, frame: &mut Frame) {
    let has_error = matches!(app.status, crate::app::PlayerStatus::Error(_));

    // 整体：左右分栏
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(26), // 左侧面板固定宽度
            Constraint::Min(0),     // 右侧面板填满剩余空间
        ])
        .split(frame.size());

    let left_chunk = main_chunks[0];

    // 右侧面板：垂直分布 (Header区域, 歌曲/搜索列表区域, 错误日志区域, 底部Help)
    let right_constraints = if has_error {
        vec![
            Constraint::Length(4),      // Header (Title + Gauge)
            Constraint::Percentage(70), // List
            Constraint::Percentage(30), // Logs
            Constraint::Length(3),      // Help (Increased to fit wrapping text)
        ]
    } else {
        vec![
            Constraint::Length(4),
            Constraint::Min(0),    // List 填满剩余
            Constraint::Length(0), // Logs
            Constraint::Length(3), // Help (Increased to fit wrapping text)
        ]
    };

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(right_constraints)
        .split(main_chunks[1]);

    // 调用 widgets 渲染
    widgets::render_status_and_gauge(app, frame, right_chunks[0]);

    // 左侧渲染分组，右侧渲染歌曲列表
    widgets::render_groups(app, frame, left_chunk);
    widgets::render_items(app, frame, right_chunks[1]);

    if has_error {
        widgets::render_logs(app, frame, right_chunks[2]);
    }
    widgets::render_help(app, frame, right_chunks[3]);

    // 移动模式浮层最后渲染，覆盖在所有内容之上
    widgets::render_move_overlay(app, frame);

    // 快捷键帮助浮层（最高优先级覆盖）
    widgets::render_help_overlay(app, frame);
}
