use crate::app::App;
use crate::net::AudioBackend;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use super::spawn_log_forwarder;

pub async fn next_page(
    audio: &Arc<AudioBackend>,
    app: &Arc<Mutex<App>>,
    page_size: usize,
    active_task: &Mutex<Option<JoinHandle<()>>>,
) {
    let (keyword, current_page, total_pages) = {
        let app_lock = app.lock().await;
        (
            app_lock.last_search_keyword.clone(),
            app_lock.current_page,
            app_lock.total_pages,
        )
    };

    if keyword.is_empty() || current_page >= total_pages {
        return;
    }

    search_page(
        audio,
        app,
        &keyword,
        current_page + 1,
        page_size,
        active_task,
    )
    .await;
}

pub async fn prev_page(
    audio: &Arc<AudioBackend>,
    app: &Arc<Mutex<App>>,
    page_size: usize,
    active_task: &Mutex<Option<JoinHandle<()>>>,
) {
    let (keyword, current_page) = {
        let app_lock = app.lock().await;
        (app_lock.last_search_keyword.clone(), app_lock.current_page)
    };

    if keyword.is_empty() || current_page <= 1 {
        return;
    }

    search_page(
        audio,
        app,
        &keyword,
        current_page - 1,
        page_size,
        active_task,
    )
    .await;
}

pub async fn search_page(
    audio: &Arc<AudioBackend>,
    app: &Arc<Mutex<App>>,
    keyword: &str,
    page: usize,
    page_size: usize,
    active_task: &Mutex<Option<JoinHandle<()>>>,
) {
    // 先检查缓存
    let mut app_lock = app.lock().await;
    if let Some(cached_results) = app_lock.get_cached_page(page) {
        let cached_results = cached_results.clone();
        app_lock.current_page = page;
        app_lock.set_search_results(cached_results, keyword.to_string());
        return;
    }

    if app_lock.is_loading_page {
        return;
    }

    let request_id = app_lock.begin_async_request();
    app_lock.is_loading_page = true;
    drop(app_lock);

    // 缓存未命中，执行搜索
    let audio_c = Arc::clone(audio);
    let app_c = Arc::clone(app);
    let keyword_clone = keyword.to_string();

    let task = tokio::spawn(async move {
        let log_tx = spawn_log_forwarder(app_c.clone());

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

    // 替换活动任务
    let mut guard = active_task.lock().await;
    if let Some(prev) = guard.take() {
        prev.abort();
    }
    *guard = Some(task);
}
