use crate::config::Config;
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
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

impl AudioBackend {
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

    fn get_cached_url(&self, keyword: &str) -> Option<String> {
        if let Ok(cache) = self.cache.lock() {
            if let Some(cached) = cache.get(keyword) {
                if self.is_cache_valid(cached.cached_at) {
                    return Some(cached.url.clone());
                }
            }
        }
        None
    }

    fn cache_url(&self, keyword: String, url: String) {
        if let Ok(mut cache) = self.cache.lock() {
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

        let yt_task = Command::new("yt-dlp")
            .env("PATH", &path)
            .args([
                "--cookies-from-browser",
                &self.config.search.cookies_browser,
                "--dump-json",
                "--flat-playlist",
                "--playlist-items",
                &format!("{}-{}", start_index, end_index),
                &format!("{}{}:{}", search_prefix, search_count, keyword),
            ])
            .output();

        log_fn("等待 yt-dlp 响应...".to_string());
        let search_timeout = self.config.search.timeout;
        let yt_output = match timeout(Duration::from_secs(search_timeout), yt_task).await {
            Ok(Ok(output)) => {
                log_fn(format!("yt-dlp 执行完成，退出码: {}", output.status));

                // 打印 stderr 中的所有日志
                let stderr = String::from_utf8_lossy(&output.stderr);
                for line in stderr.lines() {
                    log_fn(format!("[yt-dlp] {}", line));
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
        let _ = std::process::Command::new("pkill").arg("mpv").output();
        if Path::new(&self.socket_path).exists() {
            let _ = std::fs::remove_file(&self.socket_path);
        }

        let path = Self::get_extended_path();

        // 1. 检查缓存
        let stream_url = if let Some(cached_url) = self.get_cached_url(keyword) {
            log_fn("✓ 使用缓存的 URL".to_string());
            cached_url
        } else {
            // 2. 缓存未命中，执行搜索
            log_fn(format!("开始搜索: {}", keyword));
            let search_prefix = self.config.get_search_prefix();
            let yt_task = Command::new("yt-dlp")
                .env("PATH", &path)
                .args([
                    "--cookies-from-browser",
                    &self.config.search.cookies_browser,
                    "--get-url",
                    "-f",
                    "bestaudio",
                    &format!("{}1:{}", search_prefix, keyword),
                ])
                .output();

            log_fn("等待 yt-dlp 响应...".to_string());
            let play_timeout = self.config.network.play_timeout;
            let yt_output = match timeout(Duration::from_secs(play_timeout), yt_task).await {
                Ok(Ok(output)) => {
                    log_fn("yt-dlp 执行完成".to_string());
                    if !output.stderr.is_empty() {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        log_fn(format!(
                            "stderr: {}",
                            stderr.lines().take(3).collect::<Vec<_>>().join(" | ")
                        ));
                    }
                    output
                }
                Ok(Err(e)) => {
                    log_fn(format!("yt-dlp 执行失败: {}", e));
                    return Err(e.into());
                }
                Err(_) => {
                    log_fn(format!("yt-dlp 超时（{}秒）", play_timeout));
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
            self.cache_url(keyword.to_string(), url.clone());
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

        // 等待 socket 文件创建（最多等待 3 秒）
        let socket_path = self.socket_path.clone();
        for i in 0..30 {
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
        let cmd = serde_json::json!({ "command": ["get_property", "percent-pos"] });
        let mut stream = tokio::net::UnixStream::connect(&self.socket_path).await?;
        stream.write_all(format!("{}\n", cmd).as_bytes()).await?;

        let mut buf = [0; 1024];
        let n = stream.read(&mut buf).await?;
        let resp: Value = serde_json::from_slice(&buf[..n])?;

        // mpv 返回格式: {"data": 12.34, "request_id": 0, "error": "success"}
        if let Some(percent) = resp["data"].as_f64() {
            Ok(percent / 100.0)
        } else {
            Ok(0.0)
        }
    }

    pub async fn send_command(&self, args: Vec<&str>) -> Result<()> {
        let cmd = serde_json::json!({ "command": args });
        if Path::new(&self.socket_path).exists() {
            let mut stream = tokio::net::UnixStream::connect(&self.socket_path).await?;
            stream.write_all(format!("{}\n", cmd).as_bytes()).await?;
        }
        Ok(())
    }

    pub async fn is_playing(&self) -> Result<bool> {
        if !Path::new(&self.socket_path).exists() {
            return Ok(false);
        }

        let cmd = serde_json::json!({ "command": ["get_property", "pause"] });
        match tokio::net::UnixStream::connect(&self.socket_path).await {
            Ok(mut stream) => {
                if stream
                    .write_all(format!("{}\n", cmd).as_bytes())
                    .await
                    .is_err()
                {
                    return Ok(false);
                }

                let mut buf = [0; 1024];
                match tokio::time::timeout(Duration::from_millis(100), stream.read(&mut buf)).await
                {
                    Ok(Ok(n)) if n > 0 => {
                        if let Ok(resp) = serde_json::from_slice::<Value>(&buf[..n]) {
                            // pause 为 false 表示正在播放
                            return Ok(!resp["data"].as_bool().unwrap_or(true));
                        }
                        Ok(false)
                    }
                    _ => Ok(false),
                }
            }
            Err(_) => Ok(false),
        }
    }

    pub async fn is_paused(&self) -> Result<bool> {
        if !Path::new(&self.socket_path).exists() {
            return Ok(false);
        }

        let cmd = serde_json::json!({ "command": ["get_property", "pause"] });
        match tokio::net::UnixStream::connect(&self.socket_path).await {
            Ok(mut stream) => {
                if stream
                    .write_all(format!("{}\n", cmd).as_bytes())
                    .await
                    .is_err()
                {
                    return Ok(false);
                }

                let mut buf = [0; 1024];
                match tokio::time::timeout(Duration::from_millis(100), stream.read(&mut buf)).await
                {
                    Ok(Ok(n)) if n > 0 => {
                        if let Ok(resp) = serde_json::from_slice::<Value>(&buf[..n]) {
                            // pause 为 true 表示处于暂停状态
                            return Ok(resp["data"].as_bool().unwrap_or(false));
                        }
                        Ok(false)
                    }
                    _ => Ok(false),
                }
            }
            Err(_) => Ok(false),
        }
    }

    pub async fn seek(&self, seconds: i32) -> Result<()> {
        self.send_command(vec!["seek", &seconds.to_string(), "relative"])
            .await
    }
}
