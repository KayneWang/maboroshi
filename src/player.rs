use crate::app::{App, PlayerStatus};
use crate::audio::{AudioBackend, PauseState};
use crate::config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

const LOG_CHANNEL_CAPACITY: usize = 256;

pub struct Player {
    audio: Arc<AudioBackend>,
    app: Arc<Mutex<App>>,
    config: Config,
    active_task: Mutex<Option<JoinHandle<()>>>,
}

impl Player {
    pub fn new(audio: Arc<AudioBackend>, app: Arc<Mutex<App>>, config: Config) -> Self {
        Self {
            audio,
            app,
            config,
            active_task: Mutex::new(None),
        }
    }

    async fn replace_active_task(&self, next: JoinHandle<()>) {
        let mut active_task = self.active_task.lock().await;
        if let Some(prev) = active_task.take() {
            prev.abort();
        }
        *active_task = Some(next);
    }

    async fn cancel_active_task(&self) {
        let mut active_task = self.active_task.lock().await;
        if let Some(prev) = active_task.take() {
            prev.abort();
        }
    }

    pub async fn search(&self, keyword: String) {
        let mut app_lock = self.app.lock().await;
        app_lock.save_status_before_search();
        app_lock.status = PlayerStatus::Searching;
        app_lock.clear_search_results();
        let request_id = app_lock.begin_async_request();
        drop(app_lock);

        let audio_c = Arc::clone(&self.audio);
        let app_c = Arc::clone(&self.app);
        let page_size = self.config.search.max_results;
        let keyword_clone = keyword.clone();

        let task = tokio::spawn(async move {
            let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(LOG_CHANNEL_CAPACITY);

            let app_log = app_c.clone();
            tokio::spawn(async move {
                while let Some(log) = log_rx.recv().await {
                    app_log.lock().await.add_log(log);
                }
            });

            let result = audio_c
                .search(&keyword, 1, |log| {
                    let _ = log_tx.try_send(log);
                })
                .await;

            match result {
                Ok(results) => {
                    let mut a = app_c.lock().await;
                    if !a.is_active_request(request_id) {
                        return;
                    }
                    if results.is_empty() {
                        a.status = PlayerStatus::Waiting;
                        a.add_log("未找到搜索结果".to_string());
                    } else {
                        let count = results.len();
                        a.current_page = 1;
                        a.total_pages = if count < page_size { 1 } else { usize::MAX };
                        a.cache_page(1, results.clone());
                        a.set_search_results(results, keyword_clone);
                        a.add_log(format!("找到 {} 个结果，使用 ↑↓ 选择，Enter 播放", count));
                    }
                }
                Err(e) => {
                    let mut a = app_c.lock().await;
                    if !a.is_active_request(request_id) {
                        return;
                    }
                    a.status = PlayerStatus::Error(e.to_string());
                    a.add_log(format!("搜索错误: {}", e));
                }
            }
        });

        self.replace_active_task(task).await;
    }

    pub async fn play_selected_result(&self) {
        let mut app_lock = self.app.lock().await;

        if let Some(result) = app_lock.get_selected_search_result() {
            let title = result.title.clone();
            let request_id = app_lock.begin_async_request();
            app_lock.clear_search_results();
            drop(app_lock);

            let audio_c = Arc::clone(&self.audio);
            let app_c = Arc::clone(&self.app);

            let task = tokio::spawn(async move {
                let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(LOG_CHANNEL_CAPACITY);

                let app_log = app_c.clone();
                tokio::spawn(async move {
                    while let Some(log) = log_rx.recv().await {
                        app_log.lock().await.add_log(log);
                    }
                });

                {
                    let mut a = app_c.lock().await;
                    if !a.is_active_request(request_id) {
                        return;
                    }
                    a.add_log(format!("从搜索结果播放: {}", title));
                    a.status = PlayerStatus::Searching;
                    a.current_song = title.clone();
                    a.progress = 0.0;
                }

                let result = audio_c
                    .search_and_play(&title, |log| {
                        let _ = log_tx.try_send(log);
                    })
                    .await;

                match result {
                    Ok(_) => {
                        let mut a = app_c.lock().await;
                        if !a.is_active_request(request_id) {
                            return;
                        }
                        a.add_log("播放成功，设置状态".to_string());
                        a.status = PlayerStatus::Playing;
                        a.current_song = title.clone();
                        a.sync_selected_favorite();
                    }
                    Err(e) => {
                        let mut a = app_c.lock().await;
                        if !a.is_active_request(request_id) {
                            return;
                        }
                        a.add_log(format!("播放失败: {}", e));
                        a.status = PlayerStatus::Error(e.to_string());
                    }
                }
            });

            self.replace_active_task(task).await;
        }
    }

    pub async fn search_and_play(&self, song: String) {
        let mut app_lock = self.app.lock().await;
        let request_id = app_lock.begin_async_request();
        app_lock.status = PlayerStatus::Searching;
        app_lock.current_song = song.clone();
        app_lock.progress = 0.0;
        drop(app_lock);

        let audio_c = Arc::clone(&self.audio);
        let app_c = Arc::clone(&self.app);

        let task = tokio::spawn(async move {
            let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(LOG_CHANNEL_CAPACITY);

            let app_log = app_c.clone();
            tokio::spawn(async move {
                while let Some(log) = log_rx.recv().await {
                    app_log.lock().await.add_log(log);
                }
            });

            let result = audio_c
                .search_and_play(&song, |log| {
                    let _ = log_tx.try_send(log);
                })
                .await;

            match result {
                Ok(_) => {
                    let mut a = app_c.lock().await;
                    if !a.is_active_request(request_id) {
                        return;
                    }
                    a.status = PlayerStatus::Playing;
                    a.current_song = song.clone();
                    a.sync_selected_favorite();
                }
                Err(e) => {
                    let mut a = app_c.lock().await;
                    if !a.is_active_request(request_id) {
                        return;
                    }
                    a.add_log(format!("播放失败: {}", e));
                    a.status = PlayerStatus::Error(e.to_string());
                }
            }
        });

        self.replace_active_task(task).await;
    }

    pub async fn toggle_pause(&self) {
        let should_pause = {
            let app_lock = self.app.lock().await;
            match app_lock.status {
                PlayerStatus::Playing => Some(true),
                PlayerStatus::Paused => Some(false),
                _ => None,
            }
        };

        if let Some(should_pause) = should_pause {
            let pause_value = if should_pause { "yes" } else { "no" };
            if let Err(e) = self
                .audio
                .send_command(vec!["set_property", "pause", pause_value])
                .await
            {
                let mut app_lock = self.app.lock().await;
                app_lock.add_log(format!("切换暂停失败: {}", e));
                return;
            }

            let mut app_lock = self.app.lock().await;
            app_lock.status = if should_pause {
                PlayerStatus::Paused
            } else {
                PlayerStatus::Playing
            };
        }
    }

    pub async fn check_and_play_next(&self) {
        let current_status = {
            let app_lock = self.app.lock().await;
            app_lock.status.clone()
        };

        // 错误恢复：检测到错误状态时自动播放下一首
        if let PlayerStatus::Error(_) = current_status {
            let next_song = {
                let mut app_lock = self.app.lock().await;
                if let Some(next_song) = app_lock.get_next_song() {
                    app_lock.add_log(format!("自动跳过错误，播放下一首: {}", next_song));
                    Some(next_song)
                } else {
                    app_lock.add_log("没有更多歌曲可播放".to_string());
                    None
                }
            };

            if let Some(next_song) = next_song {
                self.search_and_play(next_song).await;
            }
            return;
        }

        if !matches!(current_status, PlayerStatus::Playing | PlayerStatus::Paused) {
            return;
        }

        let progress_result = self.audio.get_progress().await;
        let pause_state_result = self.audio.get_pause_state().await;

        let next_song = {
            let mut app_lock = self.app.lock().await;

            match progress_result {
                Ok(p) => {
                    app_lock.progress = p;
                }
                Err(_) => {}
            }

            match pause_state_result {
                Ok(PauseState::Paused) => {
                    if !matches!(app_lock.status, PlayerStatus::Paused) {
                        app_lock.status = PlayerStatus::Paused;
                    }
                    None
                }
                Ok(PauseState::Playing) => {
                    if matches!(app_lock.status, PlayerStatus::Paused) {
                        app_lock.status = PlayerStatus::Playing;
                    }
                    None
                }
                Ok(PauseState::Stopped) => {
                    if let Some(next_song) = app_lock.get_next_song() {
                        app_lock.add_log(format!("自动播放下一首: {}", next_song));
                        Some(next_song)
                    } else {
                        app_lock.status = PlayerStatus::Waiting;
                        app_lock.add_log("播放完成".to_string());
                        None
                    }
                }
                Err(_) => None,
            }
        };

        if let Some(next_song) = next_song {
            self.search_and_play(next_song).await;
        }
    }

    pub async fn quit(&self) {
        self.cancel_active_task().await;
        self.audio.quit().await;
    }

    pub async fn seek_forward(&self) {
        self.seek_with_log(self.config.playback.seek_seconds, "快进")
            .await;
    }

    pub async fn seek_backward(&self) {
        self.seek_with_log(-self.config.playback.seek_seconds, "快退")
            .await;
    }

    async fn seek_with_log(&self, seconds: i32, direction: &str) {
        let log_message = match self.audio.seek(seconds).await {
            Ok(_) => format!("{} {} 秒", direction, seconds.abs()),
            Err(e) => format!("{}失败: {}", direction, e),
        };

        let mut app_lock = self.app.lock().await;
        app_lock.add_log(log_message);
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

        let request_id = app_lock.begin_async_request();
        app_lock.is_loading_page = true;
        drop(app_lock);

        // 缓存未命中，执行搜索
        let audio_c = Arc::clone(&self.audio);
        let app_c = Arc::clone(&self.app);
        let page_size = self.config.search.max_results;
        let keyword_clone = keyword.to_string();

        let task = tokio::spawn(async move {
            let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(LOG_CHANNEL_CAPACITY);

            let app_log = app_c.clone();
            tokio::spawn(async move {
                while let Some(log) = log_rx.recv().await {
                    app_log.lock().await.add_log(log);
                }
            });

            let result = audio_c
                .search(&keyword_clone, page, |log| {
                    let _ = log_tx.try_send(log);
                })
                .await;

            match result {
                Ok(results) => {
                    let mut a = app_c.lock().await;
                    if !a.is_active_request(request_id) {
                        return;
                    }
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
                        if count < page_size {
                            a.total_pages = page;
                        }
                        a.cache_page(page, results.clone());
                        a.set_search_results(results, keyword_clone);
                        a.add_log(format!("第 {} 页，找到 {} 个结果", page, count));
                    }
                    a.is_loading_page = false;
                }
                Err(e) => {
                    let mut a = app_c.lock().await;
                    if !a.is_active_request(request_id) {
                        return;
                    }
                    a.add_log(format!("搜索失败: {}", e));
                    a.is_loading_page = false;
                }
            }
        });

        self.replace_active_task(task).await;
    }
}
