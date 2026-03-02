use crate::config::Config;
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::timeout;

#[derive(Clone)]
struct CachedSong {
    url: String,
    cached_at: SystemTime,
}

pub struct AudioBackend {
    socket_path: String,
    cache: Mutex<HashMap<String, CachedSong>>,
    config: Config,
    /// Lock ordering: ipc_task → playback_state → mpv_process
    ipc_task: Mutex<Option<JoinHandle<()>>>,
    playback_state: Arc<Mutex<PlaybackState>>,
    mpv_process: Mutex<Option<Child>>,
}

pub struct PlaybackState {
    pub progress: f64,
    pub pause_state: PauseState,
    /// 当前音量 (0–130)，默认 100
    pub volume: u8,
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub title: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseState {
    Paused,
    Playing,
    Stopped,
}

impl AudioBackend {
    const YTDLP_STDERR_LOG_MAX_LINES: usize = 6;
    /// 在计算分页范围时额外预留的搜索结果数，避免因 yt-dlp 返回少于预期数量而误判为最后一页
    const SEARCH_RESULT_BUFFER: usize = 50;

    pub fn new(config: Config) -> Self {
        Self {
            socket_path: config.paths.socket_path.clone(),
            cache: Mutex::new(HashMap::new()),
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

    fn get_extended_path() -> String {
        let current_path = std::env::var("PATH").unwrap_or_default();
        // 如果 PATH 中已经包含 homebrew 路径，直接返回
        if current_path.contains("/opt/homebrew/bin") || current_path.contains("/usr/local/bin") {
            return current_path;
        }
        // 否则补充常见的 Homebrew 路径（开发环境可能需要）
        format!("/opt/homebrew/bin:/usr/local/bin:{}", current_path)
    }

    fn build_ytdlp_command(&self, path: &str) -> Command {
        let mut cmd = Command::new("yt-dlp");
        // 当超时或上层任务被取消时，确保子进程不会残留。
        cmd.kill_on_drop(true);
        cmd.env("PATH", path)
            .arg("--cookies-from-browser")
            .arg(&self.config.search.cookies_browser);
        cmd
    }

    fn log_ytdlp_stderr<F>(stderr: &[u8], log_fn: &mut F)
    where
        F: FnMut(String),
    {
        let stderr = String::from_utf8_lossy(stderr);
        let mut emitted = 0usize;
        let mut total = 0usize;

        for line in stderr.lines() {
            total += 1;
            if emitted < Self::YTDLP_STDERR_LOG_MAX_LINES {
                log_fn(format!("[yt-dlp] {}", line));
                emitted += 1;
            }
        }

        if total > Self::YTDLP_STDERR_LOG_MAX_LINES {
            log_fn(format!(
                "[yt-dlp] ... 其余 {} 行日志已省略",
                total - Self::YTDLP_STDERR_LOG_MAX_LINES
            ));
        }
    }

    pub async fn search<F>(
        &self,
        keyword: &str,
        page: usize,
        mut log_fn: F,
    ) -> Result<Vec<SearchResult>>
    where
        F: FnMut(String),
    {
        let path = Self::get_extended_path();
        log_fn(format!("开始搜索: {} (第 {} 页)", keyword, page));

        let search_prefix = self.config.get_search_prefix();
        let per_page = self.config.search.max_results;
        let start_index = (page - 1) * per_page + 1;
        let end_index = page * per_page;

        // 为搜索结果预留 buffer 位置
        let search_count = end_index + Self::SEARCH_RESULT_BUFFER;

        let mut yt_cmd = self.build_ytdlp_command(&path);
        yt_cmd.args([
            "--dump-json".to_string(),
            "--flat-playlist".to_string(),
            "--playlist-items".to_string(),
            format!("{}-{}", start_index, end_index),
            format!("{}{}:{}", search_prefix, search_count, keyword),
        ]);
        let yt_task = yt_cmd.output();

        log_fn("等待 yt-dlp 响应...".to_string());
        let search_timeout = self.config.search.timeout;
        let yt_output = match timeout(Duration::from_secs(search_timeout), yt_task).await {
            Ok(Ok(output)) => {
                log_fn(format!("yt-dlp 执行完成，退出码: {}", output.status));

                // 打印 stderr 中的所有日志
                Self::log_ytdlp_stderr(&output.stderr, &mut log_fn);

                if !output.status.success() {
                    return Err(anyhow::anyhow!("yt-dlp 搜索失败: {}", output.status));
                }

                output
            }
            Ok(Err(e)) => {
                log_fn(format!("yt-dlp 执行失败: {}", e));
                return Err(e.into());
            }
            Err(_) => {
                log_fn(format!("yt-dlp 超时（{}秒）", search_timeout));
                return Err(anyhow::anyhow!("yt-dlp 超时"));
            }
        };

        let output_str = String::from_utf8_lossy(&yt_output.stdout);
        let mut results = Vec::new();

        for line in output_str.lines() {
            if let Ok(json) = serde_json::from_str::<Value>(line) {
                if let Some(title) = json["title"].as_str() {
                    results.push(SearchResult {
                        title: title.to_string(),
                    });
                }
            }
        }

        log_fn(format!("找到 {} 个结果", results.len()));
        Ok(results)
    }

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

        let path = Self::get_extended_path();

        // 1. 检查缓存
        let stream_url = if let Some(cached_url) = {
            let cache = self.cache.lock().await;
            cache.get(keyword).and_then(|c| {
                if self.is_cache_valid(c.cached_at) {
                    Some(c.url.clone())
                } else {
                    None
                }
            })
        } {
            log_fn("✓ 使用缓存的 URL".to_string());
            cached_url
        } else {
            // 2. 缓存未命中，执行搜索
            log_fn(format!("开始搜索: {}", keyword));
            let search_prefix = self.config.get_search_prefix();
            let mut yt_cmd = self.build_ytdlp_command(&path);
            yt_cmd.args([
                "--get-url".to_string(),
                "-f".to_string(),
                "bestaudio".to_string(),
                format!("{}1:{}", search_prefix, keyword),
            ]);
            let yt_task = yt_cmd.output();

            log_fn("等待 yt-dlp 响应...".to_string());
            let search_timeout = self.config.search.timeout;
            let yt_output = match timeout(Duration::from_secs(search_timeout), yt_task).await {
                Ok(Ok(output)) => {
                    log_fn("yt-dlp 执行完成".to_string());
                    Self::log_ytdlp_stderr(&output.stderr, &mut log_fn);
                    if !output.status.success() {
                        return Err(anyhow::anyhow!("yt-dlp 获取音频流失败: {}", output.status));
                    }
                    output
                }
                Ok(Err(e)) => {
                    log_fn(format!("yt-dlp 执行失败: {}", e));
                    return Err(e.into());
                }
                Err(_) => {
                    log_fn(format!("yt-dlp 超时（{}秒）", search_timeout));
                    return Err(anyhow::anyhow!("yt-dlp 超时"));
                }
            };

            let url = String::from_utf8_lossy(&yt_output.stdout)
                .trim()
                .to_string();
            if url.is_empty() {
                log_fn("未找到音频流".to_string());
                return Err(anyhow::anyhow!("未找到音频流"));
            }
            log_fn(format!(
                "获取到 URL: {}...",
                &url.chars().take(50).collect::<String>()
            ));

            // 3. 写入缓存（二次检查避免并发填入）
            {
                let mut cache = self.cache.lock().await;
                if cache
                    .get(keyword)
                    .map_or(true, |c| !self.is_cache_valid(c.cached_at))
                {
                    cache.insert(
                        keyword.to_string(),
                        CachedSong {
                            url: url.clone(),
                            cached_at: SystemTime::now(),
                        },
                    );
                    if cache.len() > self.config.cache.url_cache_size {
                        if let Some(oldest_key) = cache
                            .iter()
                            .min_by_key(|(_, v)| v.cached_at)
                            .map(|(k, _)| k.clone())
                        {
                            cache.remove(&oldest_key);
                        }
                    }
                }
            }
            log_fn("✓ 已缓存 URL".to_string());
            url
        };

        // 4. 启动 mpv
        log_fn("启动 mpv 播放器".to_string());
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

            // 3. 启动 IPC 监听任务，clone Arc 传入 spawn
            let state_clone = Arc::clone(&self.playback_state);
            let socket_path_clone = self.socket_path.clone();

            let handle = tokio::spawn(async move {
                if let Ok(mut stream) = tokio::net::UnixStream::connect(&socket_path_clone).await {
                    let (reader, mut writer) = stream.split();
                    let mut buf_reader = BufReader::new(reader);

                    // 发送属性观察请求
                    let observe_percent =
                        serde_json::json!({ "command": ["observe_property", 1, "percent-pos"] });
                    let observe_pause =
                        serde_json::json!({ "command": ["observe_property", 2, "pause"] });
                    let observe_volume =
                        serde_json::json!({ "command": ["observe_property", 3, "volume"] });

                    let _ = writer
                        .write_all(format!("{}\n", observe_percent).as_bytes())
                        .await;
                    let _ = writer
                        .write_all(format!("{}\n", observe_pause).as_bytes())
                        .await;
                    let _ = writer
                        .write_all(format!("{}\n", observe_volume).as_bytes())
                        .await;

                    let mut line = String::new();
                    while let Ok(n) = buf_reader.read_line(&mut line).await {
                        if n == 0 {
                            break; // Socket 关闭
                        }

                        if let Ok(json) = serde_json::from_str::<Value>(&line) {
                            if json["event"] == "property-change" {
                                let mut state = state_clone.lock().await;
                                if json["name"] == "percent-pos" {
                                    if let Some(val) = json["data"].as_f64() {
                                        state.progress = val / 100.0;
                                    }
                                } else if json["name"] == "pause" {
                                    if let Some(val) = json["data"].as_bool() {
                                        state.pause_state = if val {
                                            PauseState::Paused
                                        } else {
                                            PauseState::Playing
                                        };
                                    }
                                } else if json["name"] == "volume" {
                                    if let Some(val) = json["data"].as_f64() {
                                        state.volume = val.clamp(0.0, 130.0) as u8;
                                    }
                                }
                            }
                        }
                        line.clear();
                    }
                }

                // 监听退出或报错后，将状态重置为 Stopped
                let mut state = state_clone.lock().await;
                state.pause_state = PauseState::Stopped;
            });

            *ipc_task_lock = Some(handle);
        }

        Ok(())
    }

    pub async fn get_progress(&self) -> f64 {
        let state = self.playback_state.lock().await;
        state.progress
    }

    pub async fn send_command(&self, args: Vec<&str>) -> Result<()> {
        let cmd = serde_json::json!({ "command": args });
        let mut stream = tokio::net::UnixStream::connect(&self.socket_path)
            .await
            .with_context(|| format!("无法连接 mpv socket: {}", self.socket_path))?;
        stream
            .write_all(format!("{}\n", cmd).as_bytes())
            .await
            .context("发送 mpv IPC 命令失败")?;
        Ok(())
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
        // 正常退出时 quit() 已经清理了 socket，这里是最后兜底
        let _ = std::fs::remove_file(&self.socket_path);
    }
}
