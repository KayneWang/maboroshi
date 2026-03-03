use ratatui::{
    style::{Color, Modifier, Style},
    widgets::ListState,
};
use std::time::{SystemTime, UNIX_EPOCH};

// ── 颜色主题 ──────────────────────────────────────────────────────────────────

pub const COLOR_NEON_CYAN: Color = Color::Rgb(0, 230, 255);
pub const COLOR_NEON_PINK: Color = Color::Rgb(255, 80, 200);
pub const COLOR_NEON_GREEN: Color = Color::Rgb(120, 255, 120);
pub const COLOR_BG_HIGHLIGHT: Color = Color::Rgb(35, 35, 55);
pub const COLOR_WARNING: Color = Color::Rgb(255, 190, 90);

// ── 通用辅助函数 ──────────────────────────────────────────────────────────────

pub fn spinner_frame() -> &'static str {
    const FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
    let tick = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        / 120;
    FRAMES[(tick as usize) % FRAMES.len()]
}

pub fn truncate_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    if max_chars <= 1 {
        return "…".to_string();
    }
    let head: String = text.chars().take(max_chars - 1).collect();
    format!("{}…", head)
}

pub fn style_for_log_line(line: &str) -> Style {
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

/// 选中项的统一高亮样式
pub fn selected_style() -> Style {
    Style::default()
        .fg(Color::White)
        .bg(COLOR_BG_HIGHLIGHT)
        .add_modifier(Modifier::BOLD)
}

/// 构建已选中指定索引的 ListState
pub fn make_list_state(selected: usize) -> ListState {
    let mut state = ListState::default();
    state.select(Some(selected));
    state
}
