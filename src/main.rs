mod app;
mod audio;
mod player;
mod ui;

use crate::app::{App, PlayerStatus};
use crate::audio::AudioBackend;
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
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app = Arc::new(Mutex::new(App::new()));
    let audio = Arc::new(AudioBackend::new());
    let player = Player::new(Arc::clone(&audio), Arc::clone(&app));

    let tick_rate = Duration::from_millis(200);
    let mut last_tick = Instant::now();

    loop {
        {
            let mut app_lock = app.lock().await;
            terminal.draw(|f| ui::render(&mut app_lock, f))?;

            if event::poll(Duration::from_millis(10))? {
                if let Event::Key(key) = event::read()? {
                    if app_lock.input_mode {
                        match key.code {
                            KeyCode::Enter => {
                                if !app_lock.input_buffer.is_empty() {
                                    let keyword = app_lock.input_buffer.clone();
                                    app_lock.input_mode = false;
                                    app_lock.input_buffer.clear();
                                    drop(app_lock);
                                    player.search(keyword).await;
                                    continue;
                                }
                            }
                            KeyCode::Esc => {
                                app_lock.input_mode = false;
                                app_lock.input_buffer.clear();
                                app_lock.add_log("取消搜索".to_string());
                            }
                            KeyCode::Backspace => {
                                app_lock.input_buffer.pop();
                            }
                            KeyCode::Char(c) => {
                                app_lock.input_buffer.push(c);
                            }
                            _ => {}
                        }
                    } else if matches!(app_lock.status, PlayerStatus::SearchResults) {
                        // 搜索结果状态下的键盘操作
                        match key.code {
                            KeyCode::Char('q') => {
                                let _ = std::process::Command::new("pkill").arg("mpv").output();
                                break;
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
                                drop(app_lock);
                                player.play_selected_result().await;
                                continue;
                            }
                            KeyCode::Char('f') => {
                                app_lock.toggle_favorite_from_search_result();
                            }
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Char('q') => {
                                let _ = std::process::Command::new("pkill").arg("mpv").output();
                                break;
                            }
                            KeyCode::Char('s') => {
                                app_lock.input_mode = true;
                                app_lock.input_buffer.clear();
                                app_lock.add_log("进入搜索模式".to_string());
                            }
                            KeyCode::Char('f') => {
                                app_lock.toggle_favorite();
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
                                if let Some(song) = app_lock.get_selected_favorite() {
                                    app_lock.add_log(format!("从收藏播放: {}", song));
                                    drop(app_lock);
                                    player.search_and_play(song).await;
                                    continue;
                                }
                            }
                            KeyCode::Char(' ') => {
                                drop(app_lock);
                                player.toggle_pause().await;
                                continue;
                            }
                            _ => {}
                        }
                    }
                }
            }

            if !app_lock.running {
                break;
            }
        }

        if last_tick.elapsed() >= tick_rate {
            player.check_and_play_next().await;
            last_tick = Instant::now();
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
