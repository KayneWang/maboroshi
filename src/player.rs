use crate::app::{App, PlayerStatus};
use crate::audio::AudioBackend;
use crate::config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct Player {
    audio: Arc<AudioBackend>,
    app: Arc<Mutex<App>>,
    config: Config,
}

impl Player {
    pub fn new(audio: Arc<AudioBackend>, app: Arc<Mutex<App>>, config: Config) -> Self {
        Self { audio, app, config }
    }

    pub async fn search(&self, keyword: String) {
        let mut app_lock = self.app.lock().await;
        app_lock.save_status_before_search();
        app_lock.status = PlayerStatus::Searching;
        app_lock.clear_search_results();
        drop(app_lock);

        let audio_c = Arc::clone(&self.audio);
        let app_c = Arc::clone(&self.app);
        let keyword_clone = keyword.clone();

        tokio::spawn(async move {
            let (log_tx, mut log_rx) = tokio::sync::mpsc::unbounded_channel();

            let app_log = app_c.clone();
            tokio::spawn(async move {
                while let Some(log) = log_rx.recv().await {
                    app_log.lock().await.add_log(log);
                }
            });

            let result = audio_c
                .search(&keyword, 1, |log| {
                    let _ = log_tx.send(log);
                })
                .await;

            match result {
                Ok(results) => {
                    let mut a = app_c.lock().await;
                    if results.is_empty() {
                        a.status = PlayerStatus::Waiting;
                        a.add_log("未找到搜索结果".to_string());
                    } else {
                        let count = results.len();
                        a.current_page = 1;
                        a.total_pages = usize::MAX;
                        a.cache_page(1, results.clone());
                        a.set_search_results(results, keyword_clone);
                        a.add_log(format!("找到 {} 个结果，使用 ↑↓ 选择，Enter 播放", count));
                    }
                }
                Err(e) => {
                    let mut a = app_c.lock().await;
                    a.status = PlayerStatus::Error(e.to_string());
                    a.add_log(format!("搜索错误: {}", e));
                }
            }
        });
    }

    pub async fn play_selected_result(&self) {
        let mut app_lock = self.app.lock().await;

        if let Some(result) = app_lock.get_selected_search_result() {
            let title = result.title.clone();
            app_lock.clear_search_results();
            drop(app_lock);

            let audio_c = Arc::clone(&self.audio);
            let app_c = Arc::clone(&self.app);

            tokio::spawn(async move {
                let (log_tx, mut log_rx) = tokio::sync::mpsc::unbounded_channel();

                let app_log = app_c.clone();
                tokio::spawn(async move {
                    while let Some(log) = log_rx.recv().await {
                        app_log.lock().await.add_log(log);
                    }
                });

                {
                    let mut a = app_c.lock().await;
                    a.add_log(format!("从搜索结果播放: {}", title));
                    a.status = PlayerStatus::Searching;
                    a.current_song = title.clone();
                    a.progress = 0.0;
                }

                let result = audio_c
                    .search_and_play(&title, |log| {
                        let _ = log_tx.send(log);
                    })
                    .await;

                match result {
                    Ok(_) => {
                        let mut a = app_c.lock().await;
                        a.add_log("播放成功，设置状态".to_string());
                        a.status = PlayerStatus::Playing;
                        a.current_song = title.clone();
                        a.sync_selected_favorite();
                    }
                    Err(e) => {
                        let mut a = app_c.lock().await;
                        a.add_log(format!("播放失败: {}", e));
                        a.status = PlayerStatus::Error(e.to_string());
                    }
                }
            });
        }
    }

    pub async fn search_and_play(&self, song: String) {
        let mut app_lock = self.app.lock().await;
        app_lock.status = PlayerStatus::Searching;
        app_lock.current_song = song.clone();
        app_lock.progress = 0.0;
        drop(app_lock);

        let audio_c = Arc::clone(&self.audio);
        let app_c = Arc::clone(&self.app);

        tokio::spawn(async move {
            let (log_tx, mut log_rx) = tokio::sync::mpsc::unbounded_channel();

            let app_log = app_c.clone();
            tokio::spawn(async move {
                while let Some(log) = log_rx.recv().await {
                    app_log.lock().await.add_log(log);
                }
            });

            let result = audio_c
                .search_and_play(&song, |log| {
                    let _ = log_tx.send(log);
                })
                .await;

            match result {
                Ok(_) => {
                    let mut a = app_c.lock().await;
                    a.status = PlayerStatus::Playing;
                    a.current_song = song.clone();
                    a.sync_selected_favorite();
                }
                Err(e) => {
                    let mut a = app_c.lock().await;
                    a.add_log(format!("播放失败: {}", e));
                    a.status = PlayerStatus::Error(e.to_string());
                }
            }
        });
    }

    pub async fn toggle_pause(&self) {
        let audio_c = Arc::clone(&self.audio);
        let app_c = Arc::clone(&self.app);

        tokio::spawn(async move {
            let mut a = app_c.lock().await;
            match a.status {
                PlayerStatus::Playing => {
                    let _ = audio_c
                        .send_command(vec!["set_property", "pause", "yes"])
                        .await;
                    a.status = PlayerStatus::Paused;
                }
                PlayerStatus::Paused => {
                    let _ = audio_c
                        .send_command(vec!["set_property", "pause", "no"])
                        .await;
                    a.status = PlayerStatus::Playing;
                }
                _ => {}
            }
        });
    }

    pub async fn check_and_play_next(&self) {
        let mut app_lock = self.app.lock().await;

        // 错误恢复：检测到错误状态时自动播放下一首
        if let PlayerStatus::Error(_) = &app_lock.status {
            if let Some(next_song) = app_lock.get_next_song() {
                app_lock.add_log(format!("自动跳过错误，播放下一首: {}", next_song));
                drop(app_lock);
                self.search_and_play(next_song).await;
                return;
            } else {
                app_lock.add_log("没有更多歌曲可播放".to_string());
                return;
            }
        }

        if let PlayerStatus::Playing | PlayerStatus::Paused = app_lock.status {
            // 无论是播放还是暂停状态，都更新进度
            match self.audio.get_progress().await {
                Ok(p) => {
                    app_lock.progress = p;
                }
                Err(_) => {
                    if app_lock.progress == 0.0 {
                        app_lock.add_log("等待 mpv 准备就绪...".to_string());
                    }
                }
            }

            // 检查播放状态并同步到应用状态（单次 socket 查询）
            match self.audio.get_pause_state().await {
                Some(true) => {
                    // mpv 处于暂停状态
                    if !matches!(app_lock.status, PlayerStatus::Paused) {
                        app_lock.status = PlayerStatus::Paused;
                    }
                }
                Some(false) => {
                    // mpv 正在播放
                    if matches!(app_lock.status, PlayerStatus::Paused) {
                        app_lock.status = PlayerStatus::Playing;
                    }
                }
                None => {
                    // 无法连接 mpv，播放已结束，尝试播放下一首
                    if let Some(next_song) = app_lock.get_next_song() {
                        app_lock.add_log(format!("自动播放下一首: {}", next_song));
                        drop(app_lock);
                        self.search_and_play(next_song).await;
                    } else {
                        app_lock.status = PlayerStatus::Waiting;
                        app_lock.add_log("播放完成".to_string());
                    }
                }
            }
        }
    }

    pub async fn quit(&self) {
        self.audio.quit().await;
    }

    pub async fn seek_forward(&self) {
        let seconds = self.config.playback.seek_seconds;
        if let Err(e) = self.audio.seek(seconds).await {
            let mut app_lock = self.app.lock().await;
            app_lock.add_log(format!("快进失败: {}", e));
        } else {
            let mut app_lock = self.app.lock().await;
            app_lock.add_log(format!("快进 {} 秒", seconds));
        }
    }

    pub async fn seek_backward(&self) {
        let seconds = self.config.playback.seek_seconds;
        if let Err(e) = self.audio.seek(-seconds).await {
            let mut app_lock = self.app.lock().await;
            app_lock.add_log(format!("快退失败: {}", e));
        } else {
            let mut app_lock = self.app.lock().await;
            app_lock.add_log(format!("快退 {} 秒", seconds));
        }
    }

    pub async fn next_page(&self) {
        let app_lock = self.app.lock().await;
        let keyword = app_lock.last_search_keyword.clone();
        let current_page = app_lock.current_page;
        let total_pages = app_lock.total_pages;
        drop(app_lock);

        if keyword.is_empty() || current_page >= total_pages {
            return;
        }

        let next_page = current_page + 1;
        self.search_page(&keyword, next_page).await;
    }

    pub async fn prev_page(&self) {
        let app_lock = self.app.lock().await;
        let keyword = app_lock.last_search_keyword.clone();
        let current_page = app_lock.current_page;
        drop(app_lock);

        if keyword.is_empty() || current_page <= 1 {
            return;
        }

        let prev_page = current_page - 1;
        self.search_page(&keyword, prev_page).await;
    }

    async fn search_page(&self, keyword: &str, page: usize) {
        // 先检查缓存
        let mut app_lock = self.app.lock().await;
        if let Some(cached_results) = app_lock.get_cached_page(page) {
            let cached_results = cached_results.clone();
            app_lock.current_page = page;
            app_lock.set_search_results(cached_results, keyword.to_string());
            app_lock.add_log(format!("第 {} 页（来自缓存）", page));
            return;
        }

        if app_lock.is_loading_page {
            app_lock.add_log("正在加载中，请稍候...".to_string());
            return;
        }

        app_lock.is_loading_page = true;
        drop(app_lock);

        // 缓存未命中，执行搜索
        let audio_c = Arc::clone(&self.audio);
        let app_c = Arc::clone(&self.app);
        let keyword_clone = keyword.to_string();

        tokio::spawn(async move {
            let (log_tx, mut log_rx) = tokio::sync::mpsc::unbounded_channel();

            let app_log = app_c.clone();
            tokio::spawn(async move {
                while let Some(log) = log_rx.recv().await {
                    app_log.lock().await.add_log(log);
                }
            });

            let result = audio_c
                .search(&keyword_clone, page, |log| {
                    let _ = log_tx.send(log);
                })
                .await;

            match result {
                Ok(results) => {
                    let mut a = app_c.lock().await;
                    if results.is_empty() {
                        if page > 1 {
                            a.total_pages = page - 1;
                            a.add_log(format!("已到达最后一页（第 {} 页）", page - 1));
                        } else {
                            a.add_log("没有找到结果".to_string());
                        }
                    } else {
                        let count = results.len();
                        a.current_page = page;
                        a.cache_page(page, results.clone());
                        a.set_search_results(results, keyword_clone);
                        a.add_log(format!("第 {} 页，找到 {} 个结果", page, count));
                    }
                    a.is_loading_page = false;
                }
                Err(e) => {
                    let mut a = app_c.lock().await;
                    a.add_log(format!("搜索失败: {}", e));
                    a.is_loading_page = false;
                }
            }
        });
    }
}
