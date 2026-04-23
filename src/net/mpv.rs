use anyhow::{Context, Result};
use serde_json::Value;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

// ── 播放状态 ──────────────────────────────────────────────────────────────────

pub struct PlaybackState {
    pub progress: f64,
    pub pause_state: PauseState,
    /// 当前音量 (0–130)，默认 100
    pub volume: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseState {
    Paused,
    Playing,
    Stopped,
}

// ── IPC 连接抽象 ──────────────────────────────────────────────────────────────
//
// Unix 下 mpv IPC 走 Unix Domain Socket，Windows 下走 Named Pipe。
// 两种连接都实现了 AsyncRead + AsyncWrite，因此上层逻辑共用一份。

#[cfg(unix)]
type IpcStream = tokio::net::UnixStream;

#[cfg(windows)]
type IpcStream = tokio::net::windows::named_pipe::NamedPipeClient;

#[cfg(unix)]
async fn connect_ipc(path: &str) -> std::io::Result<IpcStream> {
    tokio::net::UnixStream::connect(path).await
}

#[cfg(windows)]
async fn connect_ipc(path: &str) -> std::io::Result<IpcStream> {
    use tokio::net::windows::named_pipe::ClientOptions;
    use tokio::time::{sleep, Duration};

    // Windows 下如果 named pipe 暂时处于 busy（mpv 还在建立），循环重试一小会儿。
    // ERROR_PIPE_BUSY = 231
    const ERROR_PIPE_BUSY: i32 = 231;
    for _ in 0..50 {
        match ClientOptions::new().open(path) {
            Ok(client) => return Ok(client),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                sleep(Duration::from_millis(50)).await;
            }
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "mpv IPC named pipe 持续处于 busy 状态",
    ))
}

/// 轻量探测 mpv IPC 是否就绪（用于启动阶段的等待循环）。
pub fn ipc_exists(path: &str) -> bool {
    #[cfg(unix)]
    {
        std::path::Path::new(path).exists()
    }
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        ClientOptions::new().open(path).is_ok()
    }
}

/// 清理残留的 IPC 端点。Unix 下删除 socket 文件；Windows named pipe 随进程结束自动回收，无需清理。
pub fn cleanup_ipc_file(path: &str) {
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(path);
    }
    #[cfg(windows)]
    {
        let _ = path;
    }
}

// ── mpv IPC 操作 ──────────────────────────────────────────────────────────────

/// 向 mpv IPC 发送 JSON 命令
pub async fn send_command(socket_path: &str, args: Vec<&str>) -> Result<()> {
    let cmd = serde_json::json!({ "command": args });
    let mut stream = connect_ipc(socket_path)
        .await
        .with_context(|| format!("无法连接 mpv IPC: {}", socket_path))?;
    stream
        .write_all(format!("{}\n", cmd).as_bytes())
        .await
        .context("发送 mpv IPC 命令失败")?;
    Ok(())
}

/// 启动 IPC 监听任务，持续读取 mpv property-change 事件并更新 PlaybackState。
/// 返回任务句柄。
pub fn spawn_ipc_task(socket_path: String, state: Arc<Mutex<PlaybackState>>) -> JoinHandle<()> {
    tokio::spawn(async move {
        if let Ok(stream) = connect_ipc(&socket_path).await {
            let (reader, mut writer) = tokio::io::split(stream);
            let mut buf_reader = BufReader::new(reader);

            // 发送属性观察请求
            let observe_percent =
                serde_json::json!({ "command": ["observe_property", 1, "percent-pos"] });
            let observe_pause = serde_json::json!({ "command": ["observe_property", 2, "pause"] });
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
                    break; // Socket / pipe 关闭
                }

                if let Ok(json) = serde_json::from_str::<Value>(&line) {
                    if json["event"] == "property-change" {
                        let mut state = state.lock().await;
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
        let mut state = state.lock().await;
        state.pause_state = PauseState::Stopped;
    })
}
