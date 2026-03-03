use crate::app::App;
use crate::net::AudioBackend;
use std::sync::Arc;
use tokio::sync::Mutex;

/// 音量调节：+delta / -delta，读取更新后的实际音量并写日志
pub async fn change_volume_with_log(audio: &Arc<AudioBackend>, app: &Arc<Mutex<App>>, delta: i32) {
    match audio.change_volume(delta).await {
        Ok(_) => {
            // 读取 mpv 实际更新后的音量（稍等一个事件循环让 IPC 刷新）
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            let vol = audio.get_volume().await;
            let mut app_lock = app.lock().await;
            app_lock.volume = vol;
            let direction = if delta > 0 { "🔊" } else { "🔈" };
            app_lock.add_log(format!("{} 音量: {}%", direction, vol));
        }
        Err(e) => {
            let mut app_lock = app.lock().await;
            app_lock.add_log(format!("音量调节失败: {}", e));
        }
    }
}
