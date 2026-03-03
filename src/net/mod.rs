mod mpv;
mod ytdlp;

pub use mpv::{PauseState, PlaybackState};
pub use ytdlp::SearchResult;

use crate::config::Config;
use anyhow::Result;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use ytdlp::UrlCache;

pub struct AudioBackend {
    socket_path: String,
    cache: Mutex<UrlCache>,
    config: Config,
    /// Lock ordering: ipc_task → playback_state → mpv_process
    ipc_task: Mutex<Option<JoinHandle<()>>>,
    playback_state: Arc<Mutex<PlaybackState>>,
    mpv_process: Mutex<Option<tokio::process::Child>>,
}

impl AudioBackend {
    pub fn new(config: Config) -> Self {
        Self {
            socket_path: config.paths.socket_path.clone(),
            cache: Mutex::new(UrlCache::new()),
            config,
            ipc_task: Mutex::new(None),
            playback_state: Arc::new(Mutex::new(PlaybackState {
                progress: 0.0,
                pause_state: PauseState::Stopped,
                volume: 100,
            })),
            mpv_process: Mutex::new(None),
        }
    }

    fn is_cache_valid(&self, cached_at: SystemTime) -> bool {
        if let Ok(elapsed) = SystemTime::now().duration_since(cached_at) {
            elapsed.as_secs() < self.config.cache.url_cache_ttl
        } else {
            false
        }
    }

    // ── 搜索 ──────────────────────────────────────────────────────────────────

    pub async fn search<F>(
        &self,
        keyword: &str,
        page: usize,
        log_fn: F,
    ) -> Result<Vec<SearchResult>>
    where
        F: FnMut(String),
    {
        ytdlp::search(&self.config, keyword, page, log_fn).await
    }

    // ── 搜索并播放 ────────────────────────────────────────────────────────────

    pub async fn search_and_play<F>(&self, keyword: &str, mut log_fn: F) -> Result<()>
    where
        F: FnMut(String),
    {
        // 清理旧进程和 socket
        log_fn("清理旧进程和 socket".to_string());
        self.quit().await;
        if Path::new(&self.socket_path).exists() {
            let _ = std::fs::remove_file(&self.socket_path);
        }

        let stream_url = ytdlp::fetch_stream_url(
            &self.config,
            &self.cache,
            keyword,
            |cached_at| self.is_cache_valid(cached_at),
            &mut log_fn,
        )
        .await?;

        // 启动 mpv
        log_fn("启动 mpv 播放器".to_string());
        let path = ytdlp::get_extended_path();
        let child = Command::new("mpv")
            .env("PATH", &path)
            .args([
                "--no-video",
                &format!("--input-ipc-server={}", self.socket_path),
                "--cache=yes",
                &stream_url,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;

        {
            let mut process_lock = self.mpv_process.lock().await;
            *process_lock = Some(child);
        }

        log_fn("mpv 已启动，等待 socket 就绪...".to_string());

        // 等待 socket 文件创建（最多等待 network.play_timeout 秒）
        let socket_path = self.socket_path.clone();
        let wait_timeout_secs = self.config.network.play_timeout.max(1);
        let max_attempts = (wait_timeout_secs * 10) as usize;
        let mut socket_ready = false;
        for i in 0..max_attempts {
            if Path::new(&socket_path).exists() {
                log_fn(format!("socket 就绪 ({}ms)", i * 100));
                socket_ready = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        if !socket_ready {
            log_fn("警告: socket 文件未创建，但继续播放".to_string());
        } else {
            // 遵守锁定顺序 (ipc_task → playback_state → mpv_process)
            // 1. 先锁 ipc_task，杀死旧任务
            let mut ipc_task_lock = self.ipc_task.lock().await;
            if let Some(task) = ipc_task_lock.take() {
                task.abort();
            }

            // 2. 再锁 playback_state，初始化状态
            {
                let mut state = self.playback_state.lock().await;
                state.progress = 0.0;
                state.pause_state = PauseState::Playing;
            }

            // 3. 启动 IPC 监听任务
            let handle =
                mpv::spawn_ipc_task(self.socket_path.clone(), Arc::clone(&self.playback_state));
            *ipc_task_lock = Some(handle);
        }

        Ok(())
    }

    // ── 播放状态查询 ──────────────────────────────────────────────────────────

    pub async fn get_progress(&self) -> f64 {
        let state = self.playback_state.lock().await;
        state.progress
    }

    /// 获取 mpv 播放状态。
    /// - PauseState::Paused: mpv 正在暂停
    /// - PauseState::Playing: mpv 正在播放
    /// - PauseState::Stopped: 播放器已停止（socket 不存在或连接已断开）
    pub async fn get_pause_state(&self) -> PauseState {
        let state = self.playback_state.lock().await;
        state.pause_state
    }

    pub async fn get_volume(&self) -> u8 {
        self.playback_state.lock().await.volume
    }

    // ── mpv IPC 命令 ──────────────────────────────────────────────────────────

    pub async fn send_command(&self, args: Vec<&str>) -> Result<()> {
        mpv::send_command(&self.socket_path, args).await
    }

    /// 调整音量。delta 为正数增大，负数减小；范围 0–130。
    pub async fn change_volume(&self, delta: i32) -> Result<()> {
        let delta_str = delta.to_string();
        self.send_command(vec!["add", "volume", &delta_str]).await
    }

    pub async fn seek(&self, seconds: i32) -> Result<()> {
        let seconds_str = seconds.to_string();
        self.send_command(vec!["seek", &seconds_str, "relative"])
            .await
    }

    // ── 退出 ──────────────────────────────────────────────────────────────────

    pub async fn quit(&self) {
        // 遵守锁定顺序 (ipc_task → playback_state → mpv_process)
        // 1. 先关闭 IPC 监听任务
        {
            let mut ipc_task_lock = self.ipc_task.lock().await;
            if let Some(task) = ipc_task_lock.take() {
                task.abort();
            }
        }

        // 2. 重置播放状态
        {
            let mut state = self.playback_state.lock().await;
            state.pause_state = PauseState::Stopped;
            state.progress = 0.0;
        }

        // 3. 优先通过 IPC socket 优雅退出 mpv（不持有任何 Mutex）
        let _ = self.send_command(vec!["quit"]).await;
        // 清理 socket 文件
        let _ = std::fs::remove_file(&self.socket_path);

        // 4. 如果进程还在，通过进程句柄杀掉并等待结束
        let mut process_lock = self.mpv_process.lock().await;
        if let Some(mut child) = process_lock.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }
}

impl Drop for AudioBackend {
    fn drop(&mut self) {
        // 防止程序异常退出时 socket 文件残留，导致下次启动或其他实例出现冲突
        let _ = std::fs::remove_file(&self.socket_path);
    }
}
