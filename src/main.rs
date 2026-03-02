mod app;
mod audio;
mod config;
mod player;
mod ui;

use crate::app::{App, PlayerStatus};
use crate::audio::AudioBackend;
use crate::config::Config;
use crate::player::Player;
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
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

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal_cleanup_guard = TerminalCleanupGuard::activate();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let config = Config::load();
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
        app_lock.add_log("配置加载完成".to_string());
        app_lock.add_log(format!(
            "数据源: {} ({})",
            config.search.source,
            config.get_search_prefix()
        ));
        if play_mode_ok {
            app_lock.add_log(format!("默认播放模式: {}", config.playback.default_mode));
        } else {
            app_lock.add_log(format!(
                "默认播放模式无效: {}，已回退为 shuffle",
                config.playback.default_mode
            ));
        }
        app_lock.add_log(format!(
            "缓存设置: {} 首歌曲, {} 秒有效期",
            config.cache.url_cache_size, config.cache.url_cache_ttl
        ));
    }

    let audio = Arc::new(AudioBackend::new(config.clone()));
    let player = Player::new(Arc::clone(&audio), Arc::clone(&app), config);

    let tick_rate = Duration::from_millis(200);
    let mut last_tick = Instant::now();

    enum PendingAction {
        Search(String),
        PlaySelectedResult,
        SearchAndPlay(String),
        TogglePause,
        SeekForward,
        SeekBackward,
        VolumeUp,
        VolumeDown,
        NextPage,
        PrevPage,
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
            if let Event::Key(key) = event::read()? {
                let mut app_lock = app.lock().await;
                if app_lock.input_mode {
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
                            app_lock.add_log("取消搜索".to_string());
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
                            app_lock.add_log("取消搜索结果".to_string());
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
                            app_lock.add_log("进入搜索模式".to_string());
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
                                app_lock.add_log(format!("从收藏播放: {} [{}]", song, source));
                                app_lock.current_source = source;
                                pending_action = Some(PendingAction::SearchAndPlay(song));
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
            Some(PendingAction::SearchAndPlay(song)) => {
                player.search_and_play(song).await;
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
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
