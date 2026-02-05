use crate::audio::SearchResult;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub enum PlayerStatus {
    Waiting,
    Searching,
    SearchResults, // 新增：显示搜索结果状态
    Playing,
    Paused,
    Error(String),
}

#[derive(Clone, Copy, PartialEq)]
pub enum PlayMode {
    Single,     // 单曲循环
    ListLoop,   // 列表循环
    Sequential, // 顺序播放（播完停止）
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
    pub logs: Vec<String>,
    pub input_mode: bool,
    pub input_buffer: String,
    pub favorites: Vec<FavoriteItem>,
    pub selected_favorite: usize,
    pub play_mode: PlayMode,
    pub search_results: Vec<SearchResult>,
    pub selected_search_result: usize,
    pub saved_status: Option<PlayerStatus>,
    pub current_source: String,
}

impl App {
    fn get_favorites_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".maboroshi_favorites.json")
    }

    fn load_favorites() -> Vec<FavoriteItem> {
        let path = Self::get_favorites_path();
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(data) = serde_json::from_str::<FavoritesData>(&content) {
                return data.items;
            }
        }
        Vec::new()
    }

    fn save_favorites(favorites: &[FavoriteItem]) {
        let path = Self::get_favorites_path();
        let data = FavoritesData {
            items: favorites.to_vec(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&data) {
            let _ = fs::write(&path, json);
        }
    }

    pub fn new() -> Self {
        let favorites = Self::load_favorites();
        let mut logs = vec!["应用启动".to_string()];
        if !favorites.is_empty() {
            logs.push(format!("加载了 {} 首收藏", favorites.len()));
        }

        Self {
            running: true,
            status: PlayerStatus::Waiting,
            current_song: String::new(),
            progress: 0.0,
            logs,
            input_mode: false,
            input_buffer: String::new(),
            favorites,
            selected_favorite: 0,
            play_mode: PlayMode::ListLoop,
            search_results: Vec::new(),
            selected_search_result: 0,
            saved_status: None,
            current_source: "yt".to_string(),
        }
    }

    pub fn add_log(&mut self, message: String) {
        self.logs.push(message);
        // 只保留最近 50 条日志
        if self.logs.len() > 50 {
            self.logs.remove(0);
        }
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
        Self::save_favorites(&self.favorites);
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

            Self::save_favorites(&self.favorites);
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

    pub fn set_search_results(&mut self, results: Vec<SearchResult>) {
        self.search_results = results;
        self.selected_search_result = 0;
        if !self.search_results.is_empty() {
            self.status = PlayerStatus::SearchResults;
        }
    }

    pub fn clear_search_results(&mut self) {
        self.search_results.clear();
        self.selected_search_result = 0;
    }

    pub fn save_status_before_search(&mut self) {
        if !matches!(
            self.status,
            PlayerStatus::Searching | PlayerStatus::SearchResults
        ) {
            self.saved_status = Some(match &self.status {
                PlayerStatus::Playing => PlayerStatus::Playing,
                PlayerStatus::Paused => PlayerStatus::Paused,
                PlayerStatus::Waiting => PlayerStatus::Waiting,
                PlayerStatus::Error(e) => PlayerStatus::Error(e.clone()),
                _ => PlayerStatus::Waiting,
            });
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
            PlayMode::Single => PlayMode::ListLoop,
            PlayMode::ListLoop => PlayMode::Sequential,
            PlayMode::Sequential => PlayMode::Single,
        };
        let mode_text = match self.play_mode {
            PlayMode::Single => "单曲循环",
            PlayMode::ListLoop => "列表循环",
            PlayMode::Sequential => "顺序播放",
        };
        self.add_log(format!("播放模式: {}", mode_text));
    }

    pub fn get_play_mode_text(&self) -> &str {
        match self.play_mode {
            PlayMode::Single => "🔂",
            PlayMode::ListLoop => "🔁",
            PlayMode::Sequential => "▶️",
        }
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
                    self.add_log(format!("当前歌曲在收藏列表索引: {}", current_idx));
                    let next_idx = current_idx + 1;
                    if next_idx < self.favorites.len() {
                        self.selected_favorite = next_idx;
                        self.add_log(format!("播放下一首，索引: {}", next_idx));
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
