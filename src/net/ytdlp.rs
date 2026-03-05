use crate::config::Config;
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
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

/// 判断用户输入的关键字是否已经是一个 URL（而非普通搜索词）
fn is_url(keyword: &str) -> bool {
    keyword.starts_with("http://") || keyword.starts_with("https://")
}

/// 展开 `~` 为 HOME 目录的绝对路径
fn expand_home(path: &str) -> PathBuf {
    if path.starts_with('~') {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(path.replacen('~', &home, 1))
    } else {
        PathBuf::from(path)
    }
}

/// 确保本地缓存目录存在。如果创建失败，返回 None（降级为网络流）。
fn ensure_cache_dir(cache_dir: &str) -> Option<PathBuf> {
    let dir = expand_home(cache_dir);
    if std::fs::create_dir_all(&dir).is_ok() {
        Some(dir)
    } else {
        None
    }
}

/// 执行 yt-dlp 搜索，返回标题列表。
/// - 如果 keyword 已是 URL，直接解析为播放列表/单曲，不使用搜索前缀。
/// - 否则按分页搜索模式执行。
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

    // ── URL 模式：直接解析播放列表或单曲 ─────────────────────────────────────
    if is_url(keyword) {
        log_fn(format!("检测到 URL，直接解析播放列表: {}", keyword));
        let mut yt_cmd = build_ytdlp_command(config, &path);
        yt_cmd.args(["--dump-json", "--flat-playlist", "--yes-playlist", keyword]);
        let search_timeout = config.search.timeout;
        let yt_output = match timeout(Duration::from_secs(search_timeout), yt_cmd.output()).await {
            Ok(Ok(output)) => {
                log_fn(format!("yt-dlp 执行完成，退出码: {}", output.status));
                log_ytdlp_stderr(&output.stderr, &mut log_fn);
                if !output.status.success() {
                    return Err(anyhow::anyhow!("yt-dlp 解析 URL 失败: {}", output.status));
                }
                output
            }
            Ok(Err(e)) => return Err(e.into()),
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
        log_fn(format!("解析到 {} 首歌曲", results.len()));
        return Ok(results);
    }

    // ── 关键词搜索模式 ────────────────────────────────────────────────────────
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

/// 通过 yt-dlp 获取音频流 URL（带内存 URL 缓存 + 本地文件缓存）。
///
/// 优先级：
///   1. 本地磁盘音频文件（离线缓存命中）→ 直接返回本地路径（mpv 支持本地文件）
///   2. 内存 URL 缓存（TTL 内）→ 返回网络流直链
///   3. yt-dlp 解析网络直链：同时触发后台离线下载任务
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

    // ── 1. 先解析 JSON 元数据，拿到视频 ID 和扩展名 ─────────────────────────
    // 需要先做一次 --dump-json 来得到 id 和 ext，以便检测本地缓存。
    // 但我们不想每次都执行两次 yt-dlp，所以顺序是：
    //   a. 先检查内存 URL 缓存（最快）
    //   b. 内存未命中时，用 --dump-json 得到 id/url/ext，一次搞定

    // a. 检查内存 URL 缓存
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
        // 内存缓存命中的 URL 可能已经是一个本地路径（之前被替换过）
        log_fn("✓ 使用内存缓存的 URL".to_string());
        return Ok(cached_url);
    }

    // b. 执行 yt-dlp --dump-json 获取完整元数据（包含 url、id、ext）
    log_fn(format!("开始解析音频信息: {}", keyword));
    let search_prefix = config.get_search_prefix();

    // 如果 keyword 本身是 URL，直接使用；否则加搜索前缀取第一条结果
    let query = if is_url(keyword) {
        keyword.to_string()
    } else {
        format!("{}1:{}", search_prefix, keyword)
    };

    let mut yt_cmd = build_ytdlp_command(config, &path);
    yt_cmd.args([
        "--dump-json".to_string(),
        "-f".to_string(),
        "bestaudio".to_string(),
        query,
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

    // 解析 JSON 元数据
    let json_str = String::from_utf8_lossy(&yt_output.stdout);
    // yt-dlp 可能输出多行，取第一行非空 JSON
    let json_line = json_str
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .unwrap_or("");

    let meta: Value = serde_json::from_str(json_line)
        .map_err(|e| anyhow::anyhow!("解析 yt-dlp JSON 元数据失败: {}", e))?;

    let stream_url = meta["url"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("yt-dlp 未返回有效的音频流 URL"))?
        .to_string();

    let video_id = meta["id"].as_str().unwrap_or("").to_string();
    let ext = meta["ext"].as_str().unwrap_or("m4a").to_string();

    log_fn(format!(
        "获取到 URL: {}...",
        &stream_url.chars().take(50).collect::<String>()
    ));

    // ── 2. 检查本地离线文件缓存 ───────────────────────────────────────────────
    let local_file: Option<PathBuf> = if !video_id.is_empty() {
        ensure_cache_dir(&config.paths.cache_dir).and_then(|dir| {
            let file = dir.join(format!("{}.{}", video_id, ext));
            if file.exists() {
                Some(file)
            } else {
                None
            }
        })
    } else {
        None
    };

    if let Some(ref local_path) = local_file {
        let local_url = local_path.to_string_lossy().to_string();
        log_fn(format!("✓ 使用本地缓存文件: {}", local_url));

        // 将本地路径也记入内存 URL 缓存，避免下次再调用 yt-dlp
        let mut cache_guard = cache.lock().await;
        cache_guard.insert(
            keyword.to_string(),
            CachedSong {
                url: local_url.clone(),
                cached_at: SystemTime::now(),
            },
        );
        return Ok(local_url);
    }

    // ── 3. 触发后台离线音频下载任务 ──────────────────────────────────────────
    if config.cache.offline_audio && !video_id.is_empty() {
        if let Some(cache_dir) = ensure_cache_dir(&config.paths.cache_dir) {
            let video_id_clone = video_id.clone();
            let ext_clone = ext.clone();
            let path_clone = path.clone();
            let config_clone = config.clone();
            let output_path = cache_dir.join(format!("{}.{}", video_id, ext));

            // 只在目标文件不存在时启动后台下载
            if !output_path.exists() {
                let yt_url = format!("https://www.youtube.com/watch?v={}", video_id_clone);
                let output_template = cache_dir
                    .join(format!("{}.{}", video_id_clone, ext_clone))
                    .to_string_lossy()
                    .to_string();

                tokio::spawn(async move {
                    let mut cmd = build_ytdlp_command(&config_clone, &path_clone);
                    cmd.args(["-f", "bestaudio", "-o", &output_template, &yt_url]);
                    let _ = cmd.output().await;
                });
                log_fn(format!("↓ 后台缓存音频: {}.{}", video_id, ext));
            }
        }
    }

    // ── 4. 写入内存 URL 缓存 ──────────────────────────────────────────────────
    {
        let mut cache_guard = cache.lock().await;
        if cache_guard
            .get(keyword)
            .is_none_or(|c| !is_cache_valid(c.cached_at))
        {
            cache_guard.insert(
                keyword.to_string(),
                CachedSong {
                    url: stream_url.clone(),
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
    Ok(stream_url)
}
