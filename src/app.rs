use crate::net::SearchResult;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone)]
pub enum PlayerStatus {
    Waiting,
    Searching,
    SearchResults,
    Playing,
    Paused,
    Error(String),
}

#[derive(Clone, Copy, PartialEq)]
pub enum PlayMode {
    Single,     // 单曲循环
    ListLoop,   // 列表循环
    Sequential, // 顺序播放（播完停止）
    Shuffle,    // 随机播放
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FavoriteItem {
    pub title: String,
    pub source: String,
}

#[derive(Serialize, Deserialize)]
struct FavoritesData {
    items: Vec<FavoriteItem>,
}

pub struct App {
    pub running: bool,
    pub status: PlayerStatus,
    pub current_song: String,
    pub progress: f64,
    pub volume: u8,
    pub logs: VecDeque<String>,
    pub input_mode: bool,
    pub input_buffer: String,
    /// 搜索历史，最新的在前（index 0 = 最近一条）
    pub search_history: VecDeque<String>,
    /// None = 在草稿位置；Some(i) = 当前浏览到的历史条目
    history_cursor: Option<usize>,
    /// 开始历史导航时保存的未提交输入
    input_draft: String,
    pub favorites: Vec<FavoriteItem>,
    pub selected_favorite: usize,
    pub play_mode: PlayMode,
    pub search_results: Vec<SearchResult>,
    pub selected_search_result: usize,
    pub saved_status: Option<PlayerStatus>,
    pub current_source: String,
    pub last_search_keyword: String,
    pub current_page: usize,
    pub total_pages: usize,
    pub search_cache: HashMap<usize, Vec<SearchResult>>,
    pub is_loading_page: bool,
    request_seq: u64,
    active_request_id: u64,
    favorites_path: PathBuf,
}

impl App {
    fn resolve_favorites_path(configured_path: &str) -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        if configured_path.starts_with('~') {
            PathBuf::from(configured_path.replacen('~', &home, 1))
        } else {
            PathBuf::from(configured_path)
        }
    }

    fn backup_corrupted_favorites(path: &Path) -> Result<PathBuf, String> {
        let ts = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("favorites.json");
        let mut backup_path = path.to_path_buf();
        backup_path.set_file_name(format!("{}.corrupt.{}", file_name, ts));
        fs::rename(path, &backup_path).map_err(|e| {
            format!(
                "收藏文件解析失败，且备份失败 ({} -> {}): {}",
                path.display(),
                backup_path.display(),
                e
            )
        })?;
        Ok(backup_path)
    }

    fn load_favorites(path: &Path) -> (Vec<FavoriteItem>, Option<String>) {
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Vec::new(), None),
            Err(e) => {
                return (
                    Vec::new(),
                    Some(format!("读取收藏文件失败 ({}): {}", path.display(), e)),
                );
            }
        };

        match serde_json::from_str::<FavoritesData>(&content) {
            Ok(data) => (data.items, None),
            Err(e) => match Self::backup_corrupted_favorites(path) {
                Ok(backup_path) => (
                    Vec::new(),
                    Some(format!(
                        "收藏文件已损坏并自动备份到: {}（原因: {}）",
                        backup_path.display(),
                        e
                    )),
                ),
                Err(backup_err) => (
                    Vec::new(),
                    Some(format!("收藏文件解析失败: {}；{}", e, backup_err)),
                ),
            },
        }
    }

    fn save_favorites(favorites: &[FavoriteItem], path: &Path) -> Result<(), String> {
        let data = FavoritesData {
            items: favorites.to_vec(),
        };
        let json =
            serde_json::to_string_pretty(&data).map_err(|e| format!("序列化收藏失败: {}", e))?;
        fs::write(path, json).map_err(|e| format!("保存收藏失败 ({}): {}", path.display(), e))
    }

    pub fn new(favorites_file: &str) -> Self {
        let favorites_path = Self::resolve_favorites_path(favorites_file);
        let (favorites, load_warning) = Self::load_favorites(&favorites_path);
        let mut logs = VecDeque::from(vec!["应用启动".to_string()]);
        if !favorites.is_empty() {
            logs.push_back(format!("加载了 {} 首收藏", favorites.len()));
        }
        if let Some(warning) = load_warning {
            logs.push_back(warning);
        }

        Self {
            running: true,
            status: PlayerStatus::Waiting,
            current_song: String::new(),
            progress: 0.0,
            volume: 100,
            logs,
            input_mode: false,
            input_buffer: String::new(),
            search_history: VecDeque::new(),
            history_cursor: None,
            input_draft: String::new(),
            favorites,
            selected_favorite: 0,
            play_mode: PlayMode::Shuffle,
            search_results: Vec::new(),
            selected_search_result: 0,
            saved_status: None,
            current_source: "yt".to_string(),
            last_search_keyword: String::new(),
            current_page: 1,
            total_pages: 1,
            search_cache: HashMap::new(),
            is_loading_page: false,
            request_seq: 0,
            active_request_id: 0,
            favorites_path,
        }
    }

    pub fn add_log(&mut self, message: String) {
        if self.logs.back().is_some_and(|last| last == &message) {
            return;
        }
        self.logs.push_back(message);
        // 只保留最近 50 条日志
        if self.logs.len() > 50 {
            self.logs.pop_front();
        }
    }

    /// 搜索成功后将关键词写入历史。
    /// 相同内容先删除再插入头部（去重 + 移至最新），最多保留 50 条。
    pub fn add_to_search_history(&mut self, keyword: &str) {
        let keyword = keyword.trim().to_string();
        if keyword.is_empty() {
            return;
        }
        // 去重
        self.search_history.retain(|k| k != &keyword);
        // 插入头部（最新的在前）
        self.search_history.push_front(keyword);
        if self.search_history.len() > 50 {
            self.search_history.pop_back();
        }
    }

    /// 在输入模式中按 ↑：向更早的历史条目导航。
    /// 首次按下时保存当前输入为草稿。
    pub fn history_prev(&mut self) {
        if self.search_history.is_empty() {
            return;
        }
        let next_cursor = match self.history_cursor {
            None => {
                // 首次按下，保存草稿
                self.input_draft = self.input_buffer.clone();
                0
            }
            Some(i) => (i + 1).min(self.search_history.len() - 1),
        };
        self.history_cursor = Some(next_cursor);
        self.input_buffer = self.search_history[next_cursor].clone();
    }

    /// 在输入模式中按 ↓：向更新的历史条目导航，到底恢复草稿。
    pub fn history_next(&mut self) {
        match self.history_cursor {
            None => {} // 已在草稿位置，无操作
            Some(0) => {
                // 回到草稿
                self.history_cursor = None;
                self.input_buffer = self.input_draft.clone();
            }
            Some(i) => {
                let prev = i - 1;
                self.history_cursor = Some(prev);
                self.input_buffer = self.search_history[prev].clone();
            }
        }
    }

    /// 历史导航状态重置（Enter 或 Esc 时调用）。
    pub fn history_reset(&mut self) {
        self.history_cursor = None;
        self.input_draft.clear();
    }

    pub fn toggle_favorite(&mut self) {
        if self.current_song.is_empty() {
            return;
        }

        self.add_log(format!("当前歌曲: '{}'", self.current_song));

        if let Some(pos) = self
            .favorites
            .iter()
            .position(|item| item.title == self.current_song)
        {
            self.favorites.remove(pos);
            self.add_log(format!("取消收藏: {}", self.current_song));
        } else {
            self.favorites.push(FavoriteItem {
                title: self.current_song.clone(),
                source: self.current_source.clone(),
            });
            self.add_log(format!(
                "已收藏: {} ({})",
                self.current_song, self.current_source
            ));
        }

        // 自动保存收藏列表
        if let Err(e) = Self::save_favorites(&self.favorites, &self.favorites_path) {
            self.add_log(e);
        }
    }

    /// 在收藏列表界面，直接移除当前高亮选中的收藏条目（不依赖 current_song）。
    pub fn remove_selected_favorite(&mut self) {
        if self.favorites.is_empty() {
            return;
        }
        let idx = self.selected_favorite.min(self.favorites.len() - 1);
        let title = self.favorites[idx].title.clone();
        self.favorites.remove(idx);
        // 选中行在删除后保持合法范围
        if self.selected_favorite >= self.favorites.len() && !self.favorites.is_empty() {
            self.selected_favorite = self.favorites.len() - 1;
        }
        self.add_log(format!("取消收藏: {}", title));
        if let Err(e) = Self::save_favorites(&self.favorites, &self.favorites_path) {
            self.add_log(e);
        }
    }

    pub fn is_favorite(&self) -> bool {
        self.favorites
            .iter()
            .any(|item| item.title == self.current_song)
    }

    pub fn toggle_favorite_from_search_result(&mut self) {
        if let Some(result) = self.get_selected_search_result() {
            let title = result.title.clone();

            if let Some(pos) = self.favorites.iter().position(|item| item.title == title) {
                self.favorites.remove(pos);
                self.add_log(format!("取消收藏: {}", title));
            } else {
                self.favorites.push(FavoriteItem {
                    title: title.clone(),
                    source: self.current_source.clone(),
                });
                self.add_log(format!("已收藏: {} ({})", title, self.current_source));
            }

            if let Err(e) = Self::save_favorites(&self.favorites, &self.favorites_path) {
                self.add_log(e);
            }
        }
    }

    pub fn select_next_favorite(&mut self) {
        if !self.favorites.is_empty() {
            self.selected_favorite = (self.selected_favorite + 1) % self.favorites.len();
        }
    }

    pub fn select_prev_favorite(&mut self) {
        if !self.favorites.is_empty() {
            if self.selected_favorite == 0 {
                self.selected_favorite = self.favorites.len() - 1;
            } else {
                self.selected_favorite -= 1;
            }
        }
    }

    pub fn get_selected_favorite(&self) -> Option<&FavoriteItem> {
        self.favorites.get(self.selected_favorite)
    }

    pub fn sync_selected_favorite(&mut self) {
        // 同步 selected_favorite 索引到当前播放的歌曲
        if let Some(idx) = self
            .favorites
            .iter()
            .position(|item| item.title == self.current_song)
        {
            self.selected_favorite = idx;
            self.add_log(format!("同步收藏索引到: {}", idx));
        } else {
            self.add_log(format!(
                "当前歌曲 '{}' 不在收藏列表中，无法同步索引",
                self.current_song
            ));
        }
    }

    pub fn select_next_search_result(&mut self) {
        if !self.search_results.is_empty() {
            self.selected_search_result =
                (self.selected_search_result + 1) % self.search_results.len();
        }
    }

    pub fn select_prev_search_result(&mut self) {
        if !self.search_results.is_empty() {
            if self.selected_search_result == 0 {
                self.selected_search_result = self.search_results.len() - 1;
            } else {
                self.selected_search_result -= 1;
            }
        }
    }

    pub fn get_selected_search_result(&self) -> Option<&SearchResult> {
        self.search_results.get(self.selected_search_result)
    }

    pub fn set_search_results(&mut self, results: Vec<SearchResult>, keyword: String) {
        self.search_results = results;
        self.selected_search_result = 0;
        self.last_search_keyword = keyword;
        if !self.search_results.is_empty() {
            self.status = PlayerStatus::SearchResults;
        }
    }

    pub fn clear_search_results(&mut self) {
        self.search_results.clear();
        self.selected_search_result = 0;
        self.last_search_keyword.clear();
        self.search_cache.clear();
        self.is_loading_page = false;
    }

    pub fn begin_async_request(&mut self) -> u64 {
        self.request_seq = self.request_seq.saturating_add(1);
        self.active_request_id = self.request_seq;
        self.is_loading_page = false;
        self.active_request_id
    }

    pub fn is_active_request(&self, request_id: u64) -> bool {
        self.active_request_id == request_id
    }

    pub fn get_cached_page(&self, page: usize) -> Option<&Vec<SearchResult>> {
        self.search_cache.get(&page)
    }

    pub fn cache_page(&mut self, page: usize, results: Vec<SearchResult>) {
        const MAX_CACHE_SIZE: usize = 10;

        self.search_cache.insert(page, results);

        if self.search_cache.len() > MAX_CACHE_SIZE {
            if let Some(&oldest_page) = self.search_cache.keys().min() {
                self.search_cache.remove(&oldest_page);
            }
        }
    }

    pub fn save_status_before_search(&mut self) {
        if !matches!(
            self.status,
            PlayerStatus::Searching | PlayerStatus::SearchResults
        ) {
            self.saved_status = Some(self.status.clone());
        }
    }

    pub fn restore_status_after_search(&mut self) {
        if let Some(saved) = self.saved_status.take() {
            self.status = saved;
        } else {
            self.status = PlayerStatus::Waiting;
        }
    }

    pub fn toggle_play_mode(&mut self) {
        self.play_mode = match self.play_mode {
            PlayMode::Shuffle => PlayMode::Single,
            PlayMode::Single => PlayMode::ListLoop,
            PlayMode::ListLoop => PlayMode::Sequential,
            PlayMode::Sequential => PlayMode::Shuffle,
        };
        let mode_text = match self.play_mode {
            PlayMode::Single => "单曲循环",
            PlayMode::ListLoop => "列表循环",
            PlayMode::Sequential => "顺序播放",
            PlayMode::Shuffle => "随机播放",
        };
        self.add_log(format!("播放模式: {}", mode_text));
    }

    pub fn set_play_mode_from_config(&mut self, mode: &str) -> bool {
        let normalized = mode.trim().to_lowercase();
        let parsed = match normalized.as_str() {
            "single" | "single_loop" | "single-loop" => Some(PlayMode::Single),
            "list_loop" | "list-loop" | "loop" | "list" => Some(PlayMode::ListLoop),
            "sequential" | "sequence" | "seq" => Some(PlayMode::Sequential),
            "shuffle" | "random" => Some(PlayMode::Shuffle),
            _ => None,
        };

        if let Some(play_mode) = parsed {
            self.play_mode = play_mode;
            true
        } else {
            self.play_mode = PlayMode::Shuffle;
            false
        }
    }

    pub fn get_play_mode_text(&self) -> &str {
        match self.play_mode {
            PlayMode::Single => "🔂",
            PlayMode::ListLoop => "🔁",
            PlayMode::Sequential => "▶️",
            PlayMode::Shuffle => "🔀",
        }
    }

    /// 返回 [0, max) 区间内均匀分布的伪随机数。
    /// 使用 xorshift64 算法，每个线程工作，通过拒绝采样消除取模偏差。
    fn simple_random(&self, max: usize) -> usize {
        use std::cell::Cell;
        use std::time::UNIX_EPOCH;

        thread_local! {
            static RNG_STATE: Cell<u64> = const { Cell::new(0) };
        }

        RNG_STATE.with(|state| {
            let mut s = state.get();
            if s == 0 {
                // 首次使用用系统时间作为种子，确保非零
                s = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .subsec_nanos() as u64
                    | 1; // 确保非零
            }

            // xorshift64
            let next_state = |x: u64| -> u64 {
                let mut x = x;
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x
            };

            // 拒绝采样消除取模偏差
            let threshold = u64::MAX - (u64::MAX % max as u64);
            loop {
                s = next_state(s);
                if s < threshold {
                    state.set(s);
                    return (s % max as u64) as usize;
                }
            }
        })
    }

    pub fn get_next_song(&mut self) -> Option<String> {
        match self.play_mode {
            PlayMode::Single => {
                // 单曲循环：返回当前歌曲
                if !self.current_song.is_empty() {
                    Some(self.current_song.clone())
                } else {
                    None
                }
            }
            PlayMode::Shuffle => {
                // 随机播放：从收藏列表中随机选一首（避开当前歌曲）
                if self.favorites.is_empty() {
                    return None;
                }
                if self.favorites.len() == 1 {
                    self.selected_favorite = 0;
                    return Some(self.favorites[0].title.clone());
                }
                // 避免连续播放同一首（O(1) 选取，无需重试循环）
                let mut idx = self.simple_random(self.favorites.len());
                if let Some(current_idx) = self
                    .favorites
                    .iter()
                    .position(|item| item.title == self.current_song)
                {
                    idx = self.simple_random(self.favorites.len() - 1);
                    if idx >= current_idx {
                        idx += 1;
                    }
                }
                self.selected_favorite = idx;
                Some(self.favorites[idx].title.clone())
            }
            PlayMode::ListLoop | PlayMode::Sequential => {
                // 列表循环或顺序播放：播放下一首收藏
                if self.favorites.is_empty() {
                    return None;
                }

                // 找到当前歌曲在收藏列表中的位置
                if let Some(current_idx) = self
                    .favorites
                    .iter()
                    .position(|item| item.title == self.current_song)
                {
                    let next_idx = current_idx + 1;
                    if next_idx < self.favorites.len() {
                        self.selected_favorite = next_idx;
                        return Some(self.favorites[next_idx].title.clone());
                    } else if self.play_mode == PlayMode::ListLoop {
                        // 列表循环：回到第一首
                        self.selected_favorite = 0;
                        self.add_log("列表循环，回到第一首".to_string());
                        return Some(self.favorites[0].title.clone());
                    }
                } else {
                    self.add_log(format!("当前歌曲 '{}' 不在收藏列表中", self.current_song));
                }

                // 如果当前歌曲不在收藏列表中，或者是顺序播放到最后
                None
            }
        }
    }
}
