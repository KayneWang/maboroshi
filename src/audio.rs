use crate::config::Config;
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::Mutex;
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

#[derive(Debug)]
pub enum PauseStateError {
    Io(std::io::Error),
    Timeout,
    InvalidResponse,
}

impl std::fmt::Display for PauseStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PauseStateError::Io(e) => write!(f, "io error: {}", e),
            PauseStateError::Timeout => write!(f, "timeout while querying pause state"),
            PauseStateError::InvalidResponse => write!(f, "invalid pause state response"),
        }
    }
}

impl std::error::Error for PauseStateError {}

impl AudioBackend {
    const YTDLP_STDERR_LOG_MAX_LINES: usize = 6;

    pub fn new(config: Config) -> Self {
        Self {
            socket_path: config.paths.socket_path.clone(),
            cache: Mutex::new(HashMap::new()),
            config,
        }
    }

    fn is_cache_valid(&self, cached_at: SystemTime) -> bool {
        if let Ok(elapsed) = SystemTime::now().duration_since(cached_at) {
            elapsed.as_secs() < self.config.cache.url_cache_ttl
        } else {
            false
        }
    }

    async fn get_cached_url(&self, keyword: &str) -> Option<String> {
        let cache = self.cache.lock().await;
        if let Some(cached) = cache.get(keyword) {
            if self.is_cache_valid(cached.cached_at) {
                return Some(cached.url.clone());
            }
        }
        None
    }

    async fn cache_url(&self, keyword: String, url: String) {
        let mut cache = self.cache.lock().await;
        cache.insert(
            keyword,
            CachedSong {
                url,
                cached_at: SystemTime::now(),
            },
        );

        // 限制缓存大小
        if cache.len() > self.config.cache.url_cache_size {
            // 找到最旧的条目并删除
            if let Some(oldest_key) = cache
                .iter()
                .min_by_key(|(_, v)| v.cached_at)
                .map(|(k, _)| k.clone())
            {
                cache.remove(&oldest_key);
            }
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

        // 为搜索结果预留 50 个位置
        let search_count = end_index + 50;

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
        let stream_url = if let Some(cached_url) = self.get_cached_url(keyword).await {
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

            // 3. 缓存 URL
            self.cache_url(keyword.to_string(), url.clone()).await;
            log_fn("✓ 已缓存 URL".to_string());

            url
        };

        // 4. 启动 mpv
        log_fn("启动 mpv 播放器".to_string());
        Command::new("mpv")
            .env("PATH", &path)
            .args([
                "--no-video",
                &format!("--input-ipc-server={}", self.socket_path),
                "--cache=yes",
                &stream_url,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        log_fn("mpv 已启动，等待 socket 就绪...".to_string());

        // 等待 socket 文件创建（最多等待 network.play_timeout 秒）
        let socket_path = self.socket_path.clone();
        let wait_timeout_secs = self.config.network.play_timeout.max(1);
        let max_attempts = (wait_timeout_secs * 10) as usize;
        for i in 0..max_attempts {
            if Path::new(&socket_path).exists() {
                log_fn(format!("socket 就绪 ({}ms)", i * 100));
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        if !Path::new(&socket_path).exists() {
            log_fn("警告: socket 文件未创建，但继续播放".to_string());
        }

        Ok(())
    }

    pub async fn get_progress(&self) -> Result<f64> {
        let io_timeout = Duration::from_millis(100);
        let cmd = serde_json::json!({ "command": ["get_property", "percent-pos"] });
        let mut stream = timeout(
            io_timeout,
            tokio::net::UnixStream::connect(&self.socket_path),
        )
        .await
        .map_err(|_| anyhow::anyhow!("连接 mpv socket 超时"))?
        .with_context(|| format!("无法连接 mpv socket: {}", self.socket_path))?;
        timeout(
            io_timeout,
            stream.write_all(format!("{}\n", cmd).as_bytes()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("发送进度查询命令超时"))?
        .context("发送进度查询命令失败")?;

        let mut buf = [0; 1024];
        let n = timeout(io_timeout, stream.read(&mut buf))
            .await
            .map_err(|_| anyhow::anyhow!("读取进度响应超时"))?
            .context("读取进度响应失败")?;
        let resp: Value = serde_json::from_slice(&buf[..n]).context("解析进度响应失败")?;

        // mpv 返回格式: {"data": 12.34, "request_id": 0, "error": "success"}
        if let Some(percent) = resp["data"].as_f64() {
            Ok(percent / 100.0)
        } else {
            Ok(0.0)
        }
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
    /// - Ok(PauseState::Paused): mpv 正在暂停
    /// - Ok(PauseState::Playing): mpv 正在播放
    /// - Ok(PauseState::Stopped): 播放器已停止（socket 不存在或连接已断开）
    /// - Err(...): 临时错误（超时/无效响应等）
    pub async fn get_pause_state(&self) -> std::result::Result<PauseState, PauseStateError> {
        if !Path::new(&self.socket_path).exists() {
            return Ok(PauseState::Stopped);
        }

        let cmd = serde_json::json!({ "command": ["get_property", "pause"] });
        let mut stream = match tokio::net::UnixStream::connect(&self.socket_path).await {
            Ok(s) => s,
            Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::ConnectionRefused) => {
                return Ok(PauseState::Stopped);
            }
            Err(e) => return Err(PauseStateError::Io(e)),
        };

        stream
            .write_all(format!("{}\n", cmd).as_bytes())
            .await
            .map_err(PauseStateError::Io)?;

        let mut buf = [0; 1024];
        let n = match tokio::time::timeout(Duration::from_millis(100), stream.read(&mut buf)).await
        {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(PauseStateError::Io(e)),
            Err(_) => return Err(PauseStateError::Timeout),
        };

        if n == 0 {
            return Ok(PauseState::Stopped);
        }

        let resp: Value =
            serde_json::from_slice(&buf[..n]).map_err(|_| PauseStateError::InvalidResponse)?;

        if let Some(paused) = resp["data"].as_bool() {
            if paused {
                Ok(PauseState::Paused)
            } else {
                Ok(PauseState::Playing)
            }
        } else if resp["error"].as_str() == Some("property unavailable") {
            Ok(PauseState::Stopped)
        } else {
            Err(PauseStateError::InvalidResponse)
        }
    }

    pub async fn seek(&self, seconds: i32) -> Result<()> {
        self.send_command(vec!["seek", &seconds.to_string(), "relative"])
            .await
    }

    pub async fn quit(&self) {
        // 优先通过 IPC socket 优雅退出 mpv
        let _ = self.send_command(vec!["quit"]).await;
        // 清理 socket 文件
        let _ = std::fs::remove_file(&self.socket_path);
    }
}
