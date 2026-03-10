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
    #[serde(default)]
    pub local_path: Option<String>,
}

/// 收藏分组：一个命名的歌曲集合
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FavoriteGroup {
    pub name: String,
    pub items: Vec<FavoriteItem>,
}

impl FavoriteGroup {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            items: Vec::new(),
        }
    }
}

// ── 持久化格式 ─────────────────────────────────────────────────────────────────

/// 当前格式（多分组）
#[derive(Serialize, Deserialize)]
struct FavoritesData {
    groups: Vec<FavoriteGroup>,
}

/// 旧格式（单列表），用于向后兼容迁移
#[derive(Deserialize)]
struct LegacyFavoritesData {
    items: Vec<FavoriteItem>,
}

// ── App ────────────────────────────────────────────────────────────────────────

pub struct App {
    pub running: bool,
    pub status: PlayerStatus,
    pub current_song: String,
    pub current_local_path: Option<String>,
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
    /// 多分组收藏夹
    pub groups: Vec<FavoriteGroup>,
    /// 当前激活的分组索引
    pub selected_group: usize,
    /// 当前激活分组内选中的歌曲索引
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
    /// 是否处于新建分组的输入模式
    pub group_input_mode: bool,
    /// 是否处于移动歌曲的分组选择模式
    pub move_mode: bool,
    /// 移动模式下当前高亮的目标分组索引
    pub move_target_group: usize,
    /// 是否处于删除分组的二次确认模式
    pub delete_confirm_mode: bool,
    /// 是否处于修改分组名称的输入模式
    pub rename_mode: bool,
    pub help_mode: bool,
    pub playing_from_search: bool,
    request_seq: u64,
    active_request_id: u64,
    favorites_path: PathBuf,
}

impl App {
    // ── 路径工具 ───────────────────────────────────────────────────────────────

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

    // ── 持久化 ────────────────────────────────────────────────────────────────

    fn load_favorites(path: &Path) -> (Vec<FavoriteGroup>, Option<String>) {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return (vec![FavoriteGroup::new("默认")], None);
            }
            Err(e) => {
                return (
                    vec![FavoriteGroup::new("默认")],
                    Some(format!("读取收藏文件失败 ({}): {}", path.display(), e)),
                );
            }
        };

        // 尝试新格式（groups）
        if let Ok(data) = serde_json::from_str::<FavoritesData>(&content) {
            let groups = if data.groups.is_empty() {
                vec![FavoriteGroup::new("默认")]
            } else {
                data.groups
            };
            return (groups, None);
        }

        // 尝试旧格式（items）,自动迁移
        if let Ok(legacy) = serde_json::from_str::<LegacyFavoritesData>(&content) {
            let mut default_group = FavoriteGroup::new("默认");
            default_group.items = legacy.items;
            return (
                vec![default_group],
                Some("已自动将旧版收藏格式迁移到「默认」分组".to_string()),
            );
        }

        // 两种格式都解析失败，备份并返回空
        match Self::backup_corrupted_favorites(path) {
            Ok(backup_path) => (
                vec![FavoriteGroup::new("默认")],
                Some(format!(
                    "收藏文件已损坏并自动备份到: {}",
                    backup_path.display()
                )),
            ),
            Err(backup_err) => (
                vec![FavoriteGroup::new("默认")],
                Some(format!("收藏文件解析失败；{}", backup_err)),
            ),
        }
    }

    fn save_favorites(groups: &[FavoriteGroup], path: &Path) -> Result<(), String> {
        let data = FavoritesData {
            groups: groups.to_vec(),
        };
        let json =
            serde_json::to_string_pretty(&data).map_err(|e| format!("序列化收藏失败: {}", e))?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("创建收藏目录失败 ({}): {}", parent.display(), e))?;
        }
        fs::write(path, json).map_err(|e| format!("保存收藏失败 ({}): {}", path.display(), e))
    }

    // ── 构建 ──────────────────────────────────────────────────────────────────

    pub fn new(favorites_file: &str) -> Self {
        let favorites_path = Self::resolve_favorites_path(favorites_file);
        let (groups, load_warning) = Self::load_favorites(&favorites_path);
        let mut logs = VecDeque::from(vec!["应用启动".to_string()]);
        let total: usize = groups.iter().map(|g| g.items.len()).sum();
        if total > 0 {
            logs.push_back(format!(
                "加载了 {} 首收藏（{} 个分组）",
                total,
                groups.len()
            ));
        }
        if let Some(warning) = load_warning {
            logs.push_back(warning);
        }

        Self {
            running: true,
            status: PlayerStatus::Waiting,
            current_song: String::new(),
            current_local_path: None,
            progress: 0.0,
            volume: 100,
            logs,
            input_mode: false,
            input_buffer: String::new(),
            search_history: VecDeque::new(),
            history_cursor: None,
            input_draft: String::new(),
            groups,
            selected_group: 0,
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
            group_input_mode: false,
            move_mode: false,
            move_target_group: 0,
            delete_confirm_mode: false,
            rename_mode: false,
            help_mode: false,
            playing_from_search: false,
            request_seq: 0,
            active_request_id: 0,
            favorites_path,
        }
    }

    // ── 分组访问 ──────────────────────────────────────────────────────────────

    /// 确保 selected_group 在合法范围内，返回当前激活分组的不可变引用
    pub fn active_group(&self) -> &FavoriteGroup {
        let idx = self.selected_group.min(self.groups.len().saturating_sub(1));
        &self.groups[idx]
    }

    fn active_group_mut(&mut self) -> &mut FavoriteGroup {
        let idx = self.selected_group.min(self.groups.len().saturating_sub(1));
        &mut self.groups[idx]
    }

    /// 返回当前激活分组的歌曲切片
    pub fn active_items(&self) -> &[FavoriteItem] {
        &self.active_group().items
    }

    // ── 分组管理 ──────────────────────────────────────────────────────────────

    /// 新建分组并立即切换到该分组
    pub fn create_group(&mut self, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        self.groups.push(FavoriteGroup::new(&name));
        self.selected_group = self.groups.len() - 1;
        self.selected_favorite = 0;
        self.add_log(format!("已新建分组: {}", name));
        if let Err(e) = Self::save_favorites(&self.groups, &self.favorites_path) {
            self.add_log(e);
        }
    }

    /// 将当前分组重命名为 new_name
    pub fn rename_group(&mut self, new_name: String) {
        let new_name = new_name.trim().to_string();
        if new_name.is_empty() {
            return;
        }
        let idx = self.selected_group.min(self.groups.len().saturating_sub(1));
        let old_name = self.groups[idx].name.clone();
        self.groups[idx].name = new_name.clone();
        self.add_log(format!("已将分组「{}」重命名为「{}」", old_name, new_name));
        if let Err(e) = Self::save_favorites(&self.groups, &self.favorites_path) {
            self.add_log(e);
        }
    }

    /// 删除当前分组（至少保留一个）
    pub fn delete_current_group(&mut self) {
        if self.groups.len() <= 1 {
            self.add_log("至少保留一个分组".to_string());
            return;
        }
        let name = self.active_group().name.clone();
        self.groups.remove(self.selected_group);
        if self.selected_group >= self.groups.len() {
            self.selected_group = self.groups.len() - 1;
        }
        self.selected_favorite = 0;
        self.add_log(format!("已删除分组: {}", name));
        if let Err(e) = Self::save_favorites(&self.groups, &self.favorites_path) {
            self.add_log(e);
        }
    }

    /// 切换到下一个分组
    pub fn select_next_group(&mut self) {
        if self.groups.len() > 1 {
            self.selected_group = (self.selected_group + 1) % self.groups.len();
            self.selected_favorite = 0;
        }
    }

    /// 切换到上一个分组
    pub fn select_prev_group(&mut self) {
        if self.groups.len() > 1 {
            if self.selected_group == 0 {
                self.selected_group = self.groups.len() - 1;
            } else {
                self.selected_group -= 1;
            }
            self.selected_favorite = 0;
        }
    }

    // ── 移动歌曲 ──────────────────────────────────────────────────────────────

    /// 进入移动模式，默认目标分组为当前分组的下一个
    pub fn enter_move_mode(&mut self) {
        if self.active_items().is_empty() {
            self.add_log("当前分组为空，无法移动".to_string());
            return;
        }
        if self.groups.len() <= 1 {
            self.add_log("只有一个分组，请先新建分组再移动".to_string());
            return;
        }
        // 默认目标：下一个分组（跳过当前分组）
        self.move_target_group = (self.selected_group + 1) % self.groups.len();
        if self.move_target_group == self.selected_group {
            self.move_target_group = (self.move_target_group + 1) % self.groups.len();
        }
        self.move_mode = true;
    }

    /// 移动模式：向下切换目标分组（跳过当前分组）
    pub fn move_mode_next(&mut self) {
        let len = self.groups.len();
        let mut next = (self.move_target_group + 1) % len;
        if next == self.selected_group {
            next = (next + 1) % len;
        }
        self.move_target_group = next;
    }

    /// 移动模式：向上切换目标分组（跳过当前分组）
    pub fn move_mode_prev(&mut self) {
        let len = self.groups.len();
        let mut prev = if self.move_target_group == 0 {
            len - 1
        } else {
            self.move_target_group - 1
        };
        if prev == self.selected_group {
            prev = if prev == 0 { len - 1 } else { prev - 1 };
        }
        self.move_target_group = prev;
    }

    /// 确认移动：将 selected_favorite 从当前分组剪切到 move_target_group
    pub fn confirm_move_song(&mut self) {
        if self.active_items().is_empty() {
            self.move_mode = false;
            return;
        }
        let src = self.selected_group.min(self.groups.len().saturating_sub(1));
        let dst = self
            .move_target_group
            .min(self.groups.len().saturating_sub(1));
        if src == dst {
            self.move_mode = false;
            return;
        }
        let item_idx = self
            .selected_favorite
            .min(self.groups[src].items.len().saturating_sub(1));
        let item = self.groups[src].items.remove(item_idx);
        let title = item.title.clone();
        let dst_name = self.groups[dst].name.clone();
        self.groups[dst].items.push(item);
        // 调整 selected_favorite 防止越界
        if !self.groups[src].items.is_empty() {
            self.selected_favorite = self.selected_favorite.min(self.groups[src].items.len() - 1);
        } else {
            self.selected_favorite = 0;
        }
        self.move_mode = false;
        self.add_log(format!("已将「{}」移动到「{}」", title, dst_name));
        if let Err(e) = Self::save_favorites(&self.groups, &self.favorites_path) {
            self.add_log(e);
        }
    }

    // ── 日志 ──────────────────────────────────────────────────────────────────

    pub fn add_log(&mut self, message: String) {
        if self.logs.back().is_some_and(|last| last == &message) {
            return;
        }
        self.logs.push_back(message);
        if self.logs.len() > 50 {
            self.logs.pop_front();
        }
    }

    // ── 搜索历史 ──────────────────────────────────────────────────────────────

    pub fn add_to_search_history(&mut self, keyword: &str) {
        let keyword = keyword.trim().to_string();
        if keyword.is_empty() {
            return;
        }
        self.search_history.retain(|k| k != &keyword);
        self.search_history.push_front(keyword);
        if self.search_history.len() > 50 {
            self.search_history.pop_back();
        }
    }

    pub fn history_prev(&mut self) {
        if self.search_history.is_empty() {
            return;
        }
        let next_cursor = match self.history_cursor {
            None => {
                self.input_draft = self.input_buffer.clone();
                0
            }
            Some(i) => (i + 1).min(self.search_history.len() - 1),
        };
        self.history_cursor = Some(next_cursor);
        self.input_buffer = self.search_history[next_cursor].clone();
    }

    pub fn history_next(&mut self) {
        match self.history_cursor {
            None => {}
            Some(0) => {
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

    pub fn history_reset(&mut self) {
        self.history_cursor = None;
        self.input_draft.clear();
    }

    // ── 收藏管理 ──────────────────────────────────────────────────────────────

    /// 播放中按 f：在当前激活分组中切换当前播放歌曲的收藏状态
    pub fn toggle_favorite(&mut self) {
        if self.current_song.is_empty() {
            return;
        }
        let song = self.current_song.clone();
        let source = self.current_source.clone();

        let idx = self.selected_group.min(self.groups.len().saturating_sub(1));
        // 用块作用域限制 mutable borrow 的生命周期
        let (removed, group_name) = {
            let group = &mut self.groups[idx];
            if let Some(pos) = group.items.iter().position(|item| item.title == song) {
                group.items.remove(pos);
                (true, String::new())
            } else {
                let name = group.name.clone();
                group.items.push(FavoriteItem {
                    title: song.clone(),
                    source,
                    local_path: self.current_local_path.clone(),
                });
                (false, name)
            }
        };
        if removed {
            self.add_log(format!("取消收藏: {}", song));
        } else {
            self.add_log(format!("已收藏到「{}」: {}", group_name, song));
        }

        if let Err(e) = Self::save_favorites(&self.groups, &self.favorites_path) {
            self.add_log(e);
        }
    }

    /// 浏览收藏时按 f：从当前分组移除当前高亮选中的歌曲
    pub fn remove_selected_favorite(&mut self) {
        if self.active_items().is_empty() {
            return;
        }
        let idx = self.selected_favorite.min(self.active_items().len() - 1);
        let title = self.active_group().items[idx].title.clone();
        self.active_group_mut().items.remove(idx);
        if self.selected_favorite >= self.active_items().len() && !self.active_items().is_empty() {
            self.selected_favorite = self.active_items().len() - 1;
        }
        self.add_log(format!("取消收藏: {}", title));
        if let Err(e) = Self::save_favorites(&self.groups, &self.favorites_path) {
            self.add_log(e);
        }
    }

    /// 搜索结果界面按 f：在当前分组中切换选中结果的收藏状态
    pub fn toggle_favorite_from_search_result(&mut self) {
        if let Some(result) = self.get_selected_search_result() {
            let title = result.title.clone();
            let source = self.current_source.clone();

            let idx = self.selected_group.min(self.groups.len().saturating_sub(1));
            let (removed, group_name) = {
                let group = &mut self.groups[idx];
                if let Some(pos) = group.items.iter().position(|item| item.title == title) {
                    group.items.remove(pos);
                    (true, group.name.clone())
                } else {
                    let name = group.name.clone();
                    group.items.push(FavoriteItem {
                        title: title.clone(),
                        source,
                        local_path: None,
                    });
                    (false, name)
                }
            };
            if removed {
                self.add_log(format!("取消收藏「{}」: {}", group_name, title));
            } else {
                self.add_log(format!("已收藏到「{}」: {}", group_name, title));
            }

            if let Err(e) = Self::save_favorites(&self.groups, &self.favorites_path) {
                self.add_log(e);
            }
        }
    }

    /// 将当前搜索结果全部收藏到激活分组，跳过已存在的条目
    pub fn favorite_all_results(&mut self) {
        if self.search_results.is_empty() {
            self.add_log("当前没有搜索结果".to_string());
            return;
        }
        let source = self.current_source.clone();
        let idx = self.selected_group.min(self.groups.len().saturating_sub(1));
        let group = &mut self.groups[idx];
        let group_name = group.name.clone();
        let mut added = 0usize;
        let mut skipped = 0usize;
        for result in &self.search_results {
            if group.items.iter().any(|item| item.title == result.title) {
                skipped += 1;
            } else {
                group.items.push(FavoriteItem {
                    title: result.title.clone(),
                    source: source.clone(),
                    local_path: None,
                });
                added += 1;
            }
        }
        let msg = if skipped > 0 {
            format!(
                "已将 {} 首添加到「{}」（跳过 {} 首重复）",
                added, group_name, skipped
            )
        } else {
            format!("已将 {} 首全部添加到「{}」", added, group_name)
        };
        self.add_log(msg);
        if let Err(e) = Self::save_favorites(&self.groups, &self.favorites_path) {
            self.add_log(e);
        }
    }

    pub fn is_favorite(&self) -> bool {
        self.active_items()
            .iter()
            .any(|item| item.title == self.current_song)
    }

    pub fn update_favorite_local_path(&mut self, song: &str, local_path: String) {
        let mut save_needed = false;
        for group in &mut self.groups {
            for item in &mut group.items {
                if item.title == song && item.local_path != Some(local_path.clone()) {
                    item.local_path = Some(local_path.clone());
                    save_needed = true;
                }
            }
        }
        if save_needed {
            if let Err(e) = Self::save_favorites(&self.groups, &self.favorites_path) {
                self.add_log(format!("回写 local_path 失败: {}", e));
            }
        }
    }

    // ── 收藏列表导航 ──────────────────────────────────────────────────────────

    pub fn select_next_favorite(&mut self) {
        let len = self.active_items().len();
        if len > 0 {
            self.selected_favorite = (self.selected_favorite + 1) % len;
        }
    }

    pub fn select_prev_favorite(&mut self) {
        let len = self.active_items().len();
        if len > 0 {
            if self.selected_favorite == 0 {
                self.selected_favorite = len - 1;
            } else {
                self.selected_favorite -= 1;
            }
        }
    }

    pub fn get_selected_favorite(&self) -> Option<&FavoriteItem> {
        self.active_items().get(self.selected_favorite)
    }

    pub fn sync_selected_favorite(&mut self) {
        if let Some(idx) = self
            .active_items()
            .iter()
            .position(|item| item.title == self.current_song)
        {
            self.selected_favorite = idx;
            self.add_log(format!("同步收藏索引到: {}", idx));
        } else {
            self.add_log(format!("当前歌曲 '{}' 不在当前分组中", self.current_song));
        }
    }

    // ── 搜索结果导航 ──────────────────────────────────────────────────────────

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

    // ── 异步请求追踪 ──────────────────────────────────────────────────────────

    pub fn begin_async_request(&mut self) -> u64 {
        self.request_seq = self.request_seq.saturating_add(1);
        self.active_request_id = self.request_seq;
        self.is_loading_page = false;
        self.active_request_id
    }

    pub fn is_active_request(&self, request_id: u64) -> bool {
        self.active_request_id == request_id
    }

    // ── 翻页缓存 ──────────────────────────────────────────────────────────────

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

    // ── 搜索状态保存/恢复 ─────────────────────────────────────────────────────

    pub fn save_status_before_search(&mut self) {
        if !matches!(
            self.status,
            PlayerStatus::Searching | PlayerStatus::SearchResults
        ) {
            self.saved_status = Some(self.status.clone());
        }
    }

    pub fn restore_status_after_search(&mut self) {
        if matches!(
            self.status,
            PlayerStatus::Playing | PlayerStatus::Paused | PlayerStatus::Error(_)
        ) {
            self.saved_status = None;
            return;
        }

        if let Some(saved) = self.saved_status.take() {
            self.status = saved;
        } else {
            self.status = PlayerStatus::Waiting;
        }
    }

    // ── 播放模式 ──────────────────────────────────────────────────────────────

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

    // ── 随机数 ────────────────────────────────────────────────────────────────

    fn simple_random(&self, max: usize) -> usize {
        use std::cell::Cell;
        use std::time::UNIX_EPOCH;

        thread_local! {
            static RNG_STATE: Cell<u64> = const { Cell::new(0) };
        }

        RNG_STATE.with(|state| {
            let mut s = state.get();
            if s == 0 {
                s = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .subsec_nanos() as u64
                    | 1;
            }
            let next_state = |x: u64| -> u64 {
                let mut x = x;
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x
            };
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

    // ── 自动播放下一首 ────────────────────────────────────────────────────────

    pub fn get_next_song(&mut self) -> Option<(String, Option<String>)> {
        if self.playing_from_search {
            return self.get_next_search_result();
        }

        let items = self.active_items();
        match self.play_mode {
            PlayMode::Single => {
                if !self.current_song.is_empty() {
                    Some((self.current_song.clone(), self.current_local_path.clone()))
                } else {
                    None
                }
            }
            PlayMode::Shuffle => {
                let len = items.len();
                if len == 0 {
                    return None;
                }
                if len == 1 {
                    self.selected_favorite = 0;
                    return Some((
                        self.active_items()[0].title.clone(),
                        self.active_items()[0].local_path.clone(),
                    ));
                }
                let current_song = self.current_song.clone();
                let mut idx = self.simple_random(len);
                if let Some(current_idx) = self
                    .active_items()
                    .iter()
                    .position(|item| item.title == current_song)
                {
                    idx = self.simple_random(len - 1);
                    if idx >= current_idx {
                        idx += 1;
                    }
                }
                self.selected_favorite = idx;
                Some((
                    self.active_items()[idx].title.clone(),
                    self.active_items()[idx].local_path.clone(),
                ))
            }
            PlayMode::ListLoop | PlayMode::Sequential => {
                let len = self.active_items().len();
                if len == 0 {
                    return None;
                }
                let current_song = self.current_song.clone();
                if let Some(current_idx) = self
                    .active_items()
                    .iter()
                    .position(|item| item.title == current_song)
                {
                    let next_idx = current_idx + 1;
                    if next_idx < self.active_items().len() {
                        self.selected_favorite = next_idx;
                        return Some((
                            self.active_items()[next_idx].title.clone(),
                            self.active_items()[next_idx].local_path.clone(),
                        ));
                    } else if self.play_mode == PlayMode::ListLoop {
                        self.selected_favorite = 0;
                        self.add_log("列表循环，回到第一首".to_string());
                        return Some((
                            self.active_items()[0].title.clone(),
                            self.active_items()[0].local_path.clone(),
                        ));
                    }
                } else {
                    self.add_log(format!("当前歌曲 '{}' 不在当前分组中", self.current_song));
                }
                None
            }
        }
    }

    fn get_next_search_result(&mut self) -> Option<(String, Option<String>)> {
        let len = self.search_results.len();
        if len == 0 {
            return None;
        }

        match self.play_mode {
            PlayMode::Single => {
                if !self.current_song.is_empty() {
                    Some((self.current_song.clone(), self.current_local_path.clone()))
                } else {
                    None
                }
            }
            PlayMode::Shuffle => {
                let mut idx = self.simple_random(len);
                if let Some(current_idx) = self
                    .search_results
                    .iter()
                    .position(|item| item.title == self.current_song)
                {
                    if len > 1 {
                        idx = self.simple_random(len - 1);
                        if idx >= current_idx {
                            idx += 1;
                        }
                    }
                }
                self.selected_search_result = idx;
                Some((self.search_results[idx].title.clone(), None))
            }
            PlayMode::ListLoop | PlayMode::Sequential => {
                let current_song = self.current_song.clone();
                if let Some(current_idx) = self
                    .search_results
                    .iter()
                    .position(|item| item.title == current_song)
                {
                    let next_idx = current_idx + 1;
                    if next_idx < len {
                        self.selected_search_result = next_idx;
                        Some((self.search_results[next_idx].title.clone(), None))
                    } else if self.play_mode == PlayMode::ListLoop {
                        self.selected_search_result = 0;
                        self.add_log("列表循环，回到第一首 (搜索结果)".to_string());
                        Some((self.search_results[0].title.clone(), None))
                    } else {
                        None
                    }
                } else {
                    self.add_log(format!("当前歌曲 '{}' 不在当前搜索结果中", current_song));
                    None
                }
            }
        }
    }
}
