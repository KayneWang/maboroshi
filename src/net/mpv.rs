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

// ── mpv IPC 操作 ──────────────────────────────────────────────────────────────

/// 向 mpv Unix socket 发送 JSON 命令
pub async fn send_command(socket_path: &str, args: Vec<&str>) -> Result<()> {
    let cmd = serde_json::json!({ "command": args });
    let mut stream = tokio::net::UnixStream::connect(socket_path)
        .await
        .with_context(|| format!("无法连接 mpv socket: {}", socket_path))?;
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
        if let Ok(mut stream) = tokio::net::UnixStream::connect(&socket_path).await {
            let (reader, mut writer) = stream.split();
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
                    break; // Socket 关闭
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
