use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub network: NetworkConfig,
    #[serde(default)]
    pub playback: PlaybackConfig,
    #[serde(default)]
    pub paths: PathsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    #[serde(default = "default_search_source")]
    pub source: String,
    #[serde(default = "default_max_results")]
    pub max_results: usize,
    #[serde(default = "default_search_timeout")]
    pub timeout: u64,
    #[serde(default = "default_cookies_browser")]
    pub cookies_browser: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    #[serde(default = "default_cache_size")]
    pub url_cache_size: usize,
    #[serde(default = "default_cache_ttl")]
    pub url_cache_ttl: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    #[serde(default = "default_play_timeout")]
    pub play_timeout: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackConfig {
    #[serde(default = "default_play_mode")]
    pub default_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsConfig {
    #[serde(default = "default_socket_path")]
    pub socket_path: String,
    #[serde(default = "default_favorites_file")]
    pub favorites_file: String,
}

// Default values
fn default_search_source() -> String {
    "yt".to_string()
}

fn default_max_results() -> usize {
    15
}

fn default_search_timeout() -> u64 {
    30
}

fn default_cookies_browser() -> String {
    "chrome".to_string()
}

fn default_cache_size() -> usize {
    30
}

fn default_cache_ttl() -> u64 {
    7200 // 2 hours
}

fn default_play_timeout() -> u64 {
    10
}

fn default_play_mode() -> String {
    "list_loop".to_string()
}

fn default_socket_path() -> String {
    "/tmp/maboroshi.sock".to_string()
}

fn default_favorites_file() -> String {
    "~/.maboroshi_favorites.json".to_string()
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            source: default_search_source(),
            max_results: default_max_results(),
            timeout: default_search_timeout(),
            cookies_browser: default_cookies_browser(),
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            url_cache_size: default_cache_size(),
            url_cache_ttl: default_cache_ttl(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            play_timeout: default_play_timeout(),
        }
    }
}

impl Default for PlaybackConfig {
    fn default() -> Self {
        Self {
            default_mode: default_play_mode(),
        }
    }
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            socket_path: default_socket_path(),
            favorites_file: default_favorites_file(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            search: SearchConfig::default(),
            cache: CacheConfig::default(),
            network: NetworkConfig::default(),
            playback: PlaybackConfig::default(),
            paths: PathsConfig::default(),
        }
    }
}

impl Config {
    fn get_config_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".config/maboroshi/config.toml")
    }

    pub fn load() -> Self {
        let config_path = Self::get_config_path();

        if let Ok(content) = fs::read_to_string(&config_path) {
            match toml::from_str::<Config>(&content) {
                Ok(config) => {
                    return config;
                }
                Err(e) => {
                    eprintln!("配置文件解析失败: {}, 使用默认配置", e);
                }
            }
        }

        Config::default()
    }

    pub fn save_example() -> Result<(), Box<dyn std::error::Error>> {
        let config_path = Self::get_config_path();

        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        if config_path.exists() {
            return Ok(());
        }

        let example_config = Config::default();
        let toml_string = toml::to_string_pretty(&example_config)?;
        fs::write(&config_path, toml_string)?;

        Ok(())
    }

    pub fn get_search_prefix(&self) -> String {
        // 如果 source 包含 "search" 后缀，直接使用
        // 否则自动添加 "search" 后缀
        // 例如: "youtube" -> "ytsearch", "bili" -> "bilisearch"
        // 也支持直接指定: "ytsearch", "bilisearch" 等
        let source = self.search.source.as_str();
        if source.ends_with("search") {
            source.to_string()
        } else {
            format!("{}search", source)
        }
    }
}
