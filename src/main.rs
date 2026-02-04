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
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;
use std::{
    io,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<()> {
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
                                app_lock.status = PlayerStatus::Waiting;
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
