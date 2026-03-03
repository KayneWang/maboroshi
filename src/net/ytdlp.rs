use crate::config::Config;
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use tokio::process::Command;
use tokio::time::timeout;

#[derive(Clone)]
pub struct CachedSong {
    pub url: String,
    pub cached_at: SystemTime,
}

pub type UrlCache = HashMap<String, CachedSong>;

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub title: String,
}

const YTDLP_STDERR_LOG_MAX_LINES: usize = 6;
/// 在计算分页范围时额外预留的搜索结果数，避免因 yt-dlp 返回少于预期数量而误判为最后一页
const SEARCH_RESULT_BUFFER: usize = 50;

pub fn get_extended_path() -> String {
    let current_path = std::env::var("PATH").unwrap_or_default();
    // 如果 PATH 中已经包含 homebrew 路径，直接返回
    if current_path.contains("/opt/homebrew/bin") || current_path.contains("/usr/local/bin") {
        return current_path;
    }
    // 否则补充常见的 Homebrew 路径（开发环境可能需要）
    format!("/opt/homebrew/bin:/usr/local/bin:{}", current_path)
}

pub fn build_ytdlp_command(config: &Config, path: &str) -> Command {
    let mut cmd = Command::new("yt-dlp");
    // 当超时或上层任务被取消时，确保子进程不会残留。
    cmd.kill_on_drop(true);
    cmd.env("PATH", path)
        .arg("--cookies-from-browser")
        .arg(&config.search.cookies_browser);
    cmd
}

pub fn log_ytdlp_stderr<F>(stderr: &[u8], log_fn: &mut F)
where
    F: FnMut(String),
{
    let stderr = String::from_utf8_lossy(stderr);
    let mut emitted = 0usize;
    let mut total = 0usize;

    for line in stderr.lines() {
        total += 1;
        if emitted < YTDLP_STDERR_LOG_MAX_LINES {
            log_fn(format!("[yt-dlp] {}", line));
            emitted += 1;
        }
    }

    if total > YTDLP_STDERR_LOG_MAX_LINES {
        log_fn(format!(
            "[yt-dlp] ... 其余 {} 行日志已省略",
            total - YTDLP_STDERR_LOG_MAX_LINES
        ));
    }
}

/// 执行 yt-dlp 搜索，返回标题列表
pub async fn search<F>(
    config: &Config,
    keyword: &str,
    page: usize,
    mut log_fn: F,
) -> Result<Vec<SearchResult>>
where
    F: FnMut(String),
{
    let path = get_extended_path();
    log_fn(format!("开始搜索: {} (第 {} 页)", keyword, page));

    let search_prefix = config.get_search_prefix();
    let per_page = config.search.max_results;
    let start_index = (page - 1) * per_page + 1;
    let end_index = page * per_page;

    // 为搜索结果预留 buffer 位置
    let search_count = end_index + SEARCH_RESULT_BUFFER;

    let mut yt_cmd = build_ytdlp_command(config, &path);
    yt_cmd.args([
        "--dump-json".to_string(),
        "--flat-playlist".to_string(),
        "--playlist-items".to_string(),
        format!("{}-{}", start_index, end_index),
        format!("{}{}:{}", search_prefix, search_count, keyword),
    ]);
    let yt_task = yt_cmd.output();

    log_fn("等待 yt-dlp 响应...".to_string());
    let search_timeout = config.search.timeout;
    let yt_output = match timeout(Duration::from_secs(search_timeout), yt_task).await {
        Ok(Ok(output)) => {
            log_fn(format!("yt-dlp 执行完成，退出码: {}", output.status));
            log_ytdlp_stderr(&output.stderr, &mut log_fn);
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

/// 通过 yt-dlp 获取音频流 URL（带缓存）
pub async fn fetch_stream_url<F>(
    config: &Config,
    cache: &tokio::sync::Mutex<UrlCache>,
    keyword: &str,
    is_cache_valid: impl Fn(SystemTime) -> bool,
    mut log_fn: F,
) -> Result<String>
where
    F: FnMut(String),
{
    let path = get_extended_path();

    // 1. 检查缓存
    if let Some(cached_url) = {
        let cache_guard = cache.lock().await;
        cache_guard.get(keyword).and_then(|c| {
            if is_cache_valid(c.cached_at) {
                Some(c.url.clone())
            } else {
                None
            }
        })
    } {
        log_fn("✓ 使用缓存的 URL".to_string());
        return Ok(cached_url);
    }

    // 2. 缓存未命中，执行搜索
    log_fn(format!("开始搜索: {}", keyword));
    let search_prefix = config.get_search_prefix();
    let mut yt_cmd = build_ytdlp_command(config, &path);
    yt_cmd.args([
        "--get-url".to_string(),
        "-f".to_string(),
        "bestaudio".to_string(),
        format!("{}1:{}", search_prefix, keyword),
    ]);
    let yt_task = yt_cmd.output();

    log_fn("等待 yt-dlp 响应...".to_string());
    let search_timeout = config.search.timeout;
    let yt_output = match timeout(Duration::from_secs(search_timeout), yt_task).await {
        Ok(Ok(output)) => {
            log_fn("yt-dlp 执行完成".to_string());
            log_ytdlp_stderr(&output.stderr, &mut log_fn);
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
        let mut cache_guard = cache.lock().await;
        if cache_guard
            .get(keyword)
            .is_none_or(|c| !is_cache_valid(c.cached_at))
        {
            cache_guard.insert(
                keyword.to_string(),
                CachedSong {
                    url: url.clone(),
                    cached_at: SystemTime::now(),
                },
            );
            if cache_guard.len() > config.cache.url_cache_size {
                if let Some(oldest_key) = cache_guard
                    .iter()
                    .min_by_key(|(_, v)| v.cached_at)
                    .map(|(k, _)| k.clone())
                {
                    cache_guard.remove(&oldest_key);
                }
            }
        }
    }
    log_fn("✓ 已缓存 URL".to_string());
    Ok(url)
}
