mod app;
mod config;
mod net;
mod player;
mod ui;

use crate::app::{App, PlayerStatus};
use crate::config::Config;
use crate::net::AudioBackend;
use crate::player::Player;
use anyhow::Result;
use crossterm::{
    event::{self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::{
    env, io,
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const VERSION: &str = env!("CARGO_PKG_VERSION");

struct TerminalCleanupGuard {
    active: bool,
}

impl TerminalCleanupGuard {
    fn activate() -> Self {
        Self { active: true }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for TerminalCleanupGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = disable_raw_mode();
            let mut stdout = io::stdout();
            let _ = execute!(stdout, LeaveAlternateScreen);
        }
    }
}

fn check_dependencies() -> Result<()> {
    let missing: Vec<&str> = [("mpv", "--version"), ("yt-dlp", "--version")]
        .iter()
        .filter(|(cmd, arg)| {
            std::process::Command::new(cmd)
                .arg(arg)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_err()
        })
        .map(|(cmd, _)| *cmd)
        .collect();

    if !missing.is_empty() {
        eprintln!("\n❌ 启动失败：以下依赖未找到：");
        for dep in &missing {
            eprintln!("   - {dep}");
        }
        eprintln!("\n请先安装缺少的依赖后再启动：");
        eprintln!("   brew install {}", missing.join(" "));
        eprintln!();
        anyhow::bail!("缺少必要依赖：{}", missing.join(", "));
    }
    Ok(())
}

fn print_version() {
    println!("maboroshi v{}", VERSION);
}

fn upgrade() -> Result<()> {
    println!("🔄 正在升级 maboroshi...");

    let status = Command::new("sh")
        .arg("-c")
        .arg(
            "curl -fsSL https://raw.githubusercontent.com/KayneWang/maboroshi/main/install.sh | sh",
        )
        .status()?;

    if status.success() {
        println!("✅ 升级成功！");
        Ok(())
    } else {
        anyhow::bail!("升级失败")
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 {
        match args[1].as_str() {
            "--version" | "-v" => {
                print_version();
                return Ok(());
            }
            "--upgrade" | "--update" => {
                return upgrade();
            }
            "--help" | "-h" => {
                println!("maboroshi v{}", VERSION);
                println!("\n用法:");
                println!("  maboroshi              启动音乐播放器");
                println!("  maboroshi --version    显示版本信息");
                println!("  maboroshi --upgrade    升级到最新版本");
                println!("  maboroshi --help       显示帮助信息");
                return Ok(());
            }
            _ => {
                eprintln!("未知参数: {}", args[1]);
                eprintln!("使用 --help 查看帮助");
                std::process::exit(1);
            }
        }
    }

    // 进入 TUI 前检查外部依赖，失败时直接打印友好错误信息并退出
    check_dependencies()?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
    let mut terminal_cleanup_guard = TerminalCleanupGuard::activate();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (config, config_warn) = Config::load_with_warning();
    let _ = Config::save_example();

    // 动态生成 socket 路径（基于 PID），避免多实例冲突
    let mut config = config;
    if config.paths.socket_path == "/tmp/maboroshi.sock" {
        config.paths.socket_path = format!("/tmp/maboroshi-{}.sock", std::process::id());
    }

    let app = Arc::new(Mutex::new(App::new(&config.paths.favorites_file)));

    {
        let mut app_lock = app.lock().await;
        app_lock.current_source = config.search.source.clone();
        let play_mode_ok = app_lock.set_play_mode_from_config(&config.playback.default_mode);
        // 只在有警告/错误时记录日志
        if let Some(warn) = config_warn {
            app_lock.add_log(format!("⚠ 配置警告: {}", warn));
        }
        if !play_mode_ok {
            app_lock.add_log(format!(
                "⚠ 播放模式配置无效: {}，已回退为 shuffle",
                config.playback.default_mode
            ));
        }
    }

    let audio = Arc::new(AudioBackend::new(config.clone()));
    let player = Player::new(Arc::clone(&audio), Arc::clone(&app), config);

    let tick_rate = Duration::from_millis(200);
    let mut last_tick = Instant::now();

    enum PendingAction {
        Search(String),
        PlaySelectedResult,
        SearchAndPlay(String, Option<String>),
        TogglePause,
        SeekForward,
        SeekBackward,
        VolumeUp,
        VolumeDown,
        NextPage,
        PrevPage,
        CreateGroup(String),
        Quit,
    }

    loop {
        {
            let mut app_lock = app.lock().await;
            terminal.draw(|f| ui::render(&mut app_lock, f))?;
            if !app_lock.running {
                break;
            }
        }

        let mut pending_action = None;

        if event::poll(Duration::from_millis(10))? {
            let evt = event::read()?;
            // 括号粘贴模式：整段粘贴内容作为 Event::Paste 投递，不含换行，不会误触 Enter
            if let Event::Paste(pasted) = evt {
                let mut app_lock = app.lock().await;
                if app_lock.input_mode {
                    // 去掉粘贴内容中的换行符后追加到 buffer
                    let clean: String = pasted
                        .chars()
                        .filter(|c| *c != '\n' && *c != '\r')
                        .collect();
                    app_lock.input_buffer.push_str(&clean);
                    app_lock.history_reset();
                }
                continue;
            }
            if let Event::Key(key) = evt {
                let mut app_lock = app.lock().await;
                // ── 删除分组二次确认 ──────────────────────────────────
                if app_lock.delete_confirm_mode {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            app_lock.delete_confirm_mode = false;
                            app_lock.delete_current_group();
                        }
                        _ => {
                            app_lock.delete_confirm_mode = false;
                        }
                    }
                // ── 重命名分组输入模式 ──────────────────────────────
                } else if app_lock.rename_mode {
                    match key.code {
                        KeyCode::Enter => {
                            if !app_lock.input_buffer.is_empty() {
                                let new_name = app_lock.input_buffer.clone();
                                app_lock.rename_mode = false;
                                app_lock.input_buffer.clear();
                                app_lock.rename_group(new_name);
                            }
                        }
                        KeyCode::Esc => {
                            app_lock.rename_mode = false;
                            app_lock.input_buffer.clear();
                        }
                        KeyCode::Backspace => {
                            app_lock.input_buffer.pop();
                        }
                        KeyCode::Char(c) => {
                            app_lock.input_buffer.push(c);
                        }
                        _ => {}
                    }
                // ── 移动模式：分组选择浮层 ─────────────────────────────
                } else if app_lock.move_mode {
                    match key.code {
                        KeyCode::Enter => {
                            app_lock.confirm_move_song();
                        }
                        KeyCode::Esc => {
                            app_lock.move_mode = false;
                        }
                        KeyCode::Down => {
                            app_lock.move_mode_next();
                        }
                        KeyCode::Up => {
                            app_lock.move_mode_prev();
                        }
                        _ => {}
                    }
                // ── 新建分组输入模式 ─────────────────────────────
                } else if app_lock.group_input_mode {
                    match key.code {
                        KeyCode::Enter => {
                            if !app_lock.input_buffer.is_empty() {
                                let name = app_lock.input_buffer.clone();
                                app_lock.group_input_mode = false;
                                app_lock.input_buffer.clear();
                                pending_action = Some(PendingAction::CreateGroup(name));
                            }
                        }
                        KeyCode::Esc => {
                            app_lock.group_input_mode = false;
                            app_lock.input_buffer.clear();
                        }
                        KeyCode::Backspace => {
                            app_lock.input_buffer.pop();
                        }
                        KeyCode::Char(c) => {
                            app_lock.input_buffer.push(c);
                        }
                        _ => {}
                    }
                // ── 搜索关键词输入模式 ─────────────────────────────────
                } else if app_lock.input_mode {
                    match key.code {
                        KeyCode::Enter => {
                            if !app_lock.input_buffer.is_empty() {
                                let keyword = app_lock.input_buffer.clone();
                                app_lock.add_to_search_history(&keyword);
                                app_lock.history_reset();
                                app_lock.input_mode = false;
                                app_lock.input_buffer.clear();
                                pending_action = Some(PendingAction::Search(keyword));
                            }
                        }
                        KeyCode::Esc => {
                            app_lock.history_reset();
                            app_lock.input_mode = false;
                            app_lock.input_buffer.clear();
                        }
                        KeyCode::Up => {
                            app_lock.history_prev();
                        }
                        KeyCode::Down => {
                            app_lock.history_next();
                        }
                        KeyCode::Backspace => {
                            app_lock.input_buffer.pop();
                            // 输入时退出历史导航模式
                            app_lock.history_reset();
                        }
                        KeyCode::Char(c) => {
                            app_lock.input_buffer.push(c);
                            // 输入时退出历史导航模式
                            app_lock.history_reset();
                        }
                        _ => {}
                    }
                } else if matches!(app_lock.status, PlayerStatus::SearchResults) {
                    // 搜索结果状态下的键盘操作
                    match key.code {
                        KeyCode::Char('q') => {
                            pending_action = Some(PendingAction::Quit);
                        }
                        KeyCode::Esc => {
                            app_lock.clear_search_results();
                            app_lock.restore_status_after_search();
                        }
                        KeyCode::Up => {
                            app_lock.select_prev_search_result();
                        }
                        KeyCode::Down => {
                            app_lock.select_next_search_result();
                        }
                        KeyCode::Enter => {
                            pending_action = Some(PendingAction::PlaySelectedResult);
                        }
                        KeyCode::Char('f') => {
                            app_lock.toggle_favorite_from_search_result();
                        }
                        KeyCode::Char('F') => {
                            app_lock.favorite_all_results();
                        }
                        KeyCode::Right => {
                            pending_action = Some(PendingAction::NextPage);
                        }
                        KeyCode::Left => {
                            pending_action = Some(PendingAction::PrevPage);
                        }
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Char('q') => {
                            pending_action = Some(PendingAction::Quit);
                        }
                        KeyCode::Char('s') => {
                            app_lock.input_mode = true;
                            app_lock.input_buffer.clear();
                        }
                        // 新建分组
                        KeyCode::Char('g') => {
                            app_lock.group_input_mode = true;
                            app_lock.input_buffer.clear();
                        }
                        // 重命名当前分组（预填当前名称）
                        KeyCode::Char('R') => {
                            let current_name = app_lock.active_group().name.clone();
                            app_lock.rename_mode = true;
                            app_lock.input_buffer = current_name;
                        }
                        // 删除当前分组（需要二次确认）
                        KeyCode::Char('D') => {
                            if app_lock.groups.len() > 1 {
                                let group_name = app_lock.active_group().name.clone();
                                app_lock.delete_confirm_mode = true;
                                app_lock.add_log(format!(
                                    "⚠ 删除分组「{}」? 按 y 确认，任意键取消",
                                    group_name
                                ));
                            } else {
                                app_lock.add_log("至少保留一个分组".to_string());
                            }
                        }
                        // 移动歌曲到其他分组
                        KeyCode::Char('M') => {
                            app_lock.enter_move_mode();
                        }
                        // 切换分组
                        KeyCode::Tab => {
                            app_lock.select_next_group();
                        }
                        KeyCode::BackTab => {
                            app_lock.select_prev_group();
                        }
                        KeyCode::Char('f') => {
                            if matches!(
                                app_lock.status,
                                PlayerStatus::Playing | PlayerStatus::Paused
                            ) {
                                // 播放中：切换当前播放歌曲的收藏状态
                                app_lock.toggle_favorite();
                            } else {
                                // 收藏列表浏览中：直接移除选中的条目
                                app_lock.remove_selected_favorite();
                            }
                        }
                        KeyCode::Char('m') => {
                            app_lock.toggle_play_mode();
                        }
                        KeyCode::Up => {
                            app_lock.select_prev_favorite();
                        }
                        KeyCode::Down => {
                            app_lock.select_next_favorite();
                        }
                        KeyCode::Enter => {
                            if let Some(item) = app_lock.get_selected_favorite() {
                                let song = item.title.clone();
                                let source = item.source.clone();
                                let path = item.local_path.clone();
                                app_lock.add_log(format!("从收藏播放: {} [{}]", song, source));
                                app_lock.current_source = source;
                                pending_action = Some(PendingAction::SearchAndPlay(song, path));
                            }
                        }
                        KeyCode::Char(' ') => {
                            pending_action = Some(PendingAction::TogglePause);
                        }
                        KeyCode::Right => {
                            if matches!(
                                app_lock.status,
                                PlayerStatus::Playing | PlayerStatus::Paused
                            ) {
                                pending_action = Some(PendingAction::SeekForward);
                            }
                        }
                        KeyCode::Left => {
                            if matches!(
                                app_lock.status,
                                PlayerStatus::Playing | PlayerStatus::Paused
                            ) {
                                pending_action = Some(PendingAction::SeekBackward);
                            }
                        }
                        KeyCode::Char('+') | KeyCode::Char('=') => {
                            pending_action = Some(PendingAction::VolumeUp);
                        }
                        KeyCode::Char('-') => {
                            pending_action = Some(PendingAction::VolumeDown);
                        }
                        _ => {}
                    }
                }
            }
        }

        match pending_action {
            Some(PendingAction::Search(keyword)) => {
                player.search(keyword).await;
                continue;
            }
            Some(PendingAction::PlaySelectedResult) => {
                player.play_selected_result().await;
                continue;
            }
            Some(PendingAction::SearchAndPlay(song, local_path)) => {
                player.search_and_play(song, local_path).await;
                continue;
            }
            Some(PendingAction::TogglePause) => {
                player.toggle_pause().await;
                continue;
            }
            Some(PendingAction::SeekForward) => {
                player.seek_forward().await;
                continue;
            }
            Some(PendingAction::SeekBackward) => {
                player.seek_backward().await;
                continue;
            }
            Some(PendingAction::VolumeUp) => {
                player.volume_up().await;
                continue;
            }
            Some(PendingAction::VolumeDown) => {
                player.volume_down().await;
                continue;
            }
            Some(PendingAction::NextPage) => {
                player.next_page().await;
                continue;
            }
            Some(PendingAction::PrevPage) => {
                player.prev_page().await;
                continue;
            }
            Some(PendingAction::CreateGroup(name)) => {
                let mut app_lock = app.lock().await;
                app_lock.create_group(name);
                continue;
            }
            Some(PendingAction::Quit) => {
                player.quit().await;
                break;
            }
            None => {}
        }

        if last_tick.elapsed() >= tick_rate {
            player.check_and_play_next().await;
            last_tick = Instant::now();
        }
    }

    terminal_cleanup_guard.disarm();
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableBracketedPaste
    )?;
    Ok(())
}
