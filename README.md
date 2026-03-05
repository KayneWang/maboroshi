# 🌀 Maboroshi (幻) - 终端音乐播放器

一个基于 Rust 和 TUI 的轻量级音乐播放器，通过 yt-dlp 支持 YouTube、Bilibili 等多平台搜索和播放音乐。

[![Release](https://img.shields.io/github/v/release/KayneWang/maboroshi?style=for-the-badge)](https://github.com/KayneWang/maboroshi/releases)
[![License](https://img.shields.io/badge/license-MIT-blue?style=for-the-badge)](LICENSE)
![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)
![Terminal](https://img.shields.io/badge/Terminal-TUI-blue?style=for-the-badge)

## 🚀 快速开始

```bash
# macOS 一键安装
curl -fsSL https://raw.githubusercontent.com/KayneWang/maboroshi/main/install.sh | sh

# 安装依赖（必需）
brew install yt-dlp mpv

# 运行
maboroshi
```

## ✨ 特性

- 🔍 **多源音乐搜索** - 支持 YouTube、Bilibili 等多个平台搜索并播放音乐
- ⚙️ **配置文件支持** - 自定义搜索源、缓存大小、音量步长等参数
- 📁 **多分组收藏夹** - 按名称创建多个分组（如「薛之谦」、「纯音乐」），支持重命名、删除、移动歌曲、一键批量收藏
- 🔄 **多种播放模式** - 随机播放、单曲循环、列表循环、顺序播放
- 🎯 **智能缓存** - 搜索结果分页缓存 + 音频 URL 缓存，翻页和重播更流畅
- 🔊 **实时音量控制** - `+`/`-` 调节音量，通过 mpv IPC 实时同步
- 📋 **实时日志** - 仅记录关键操作结果和错误，不展示过程噪音
- 🎨 **美观界面** - 简洁的 TUI 界面，状态一目了然

## 📦 依赖

在使用前，请确保系统已安装以下工具：

- **yt-dlp** - 用于搜索和获取音频流
- **mpv** - 音频播放器

### macOS 安装

```bash
brew install yt-dlp mpv
```

## 🚀 安装

### 方式 1：下载预编译二进制（推荐）

从 [Releases 页面](https://github.com/KayneWang/maboroshi/releases) 下载适合你系统的二进制文件：

#### macOS (Apple Silicon)

```bash
# 下载最新版本
curl -L https://github.com/KayneWang/maboroshi/releases/latest/download/maboroshi-macos-aarch64 -o maboroshi

# 添加执行权限
chmod +x maboroshi

# 移动到系统路径（可选）
sudo mv maboroshi /usr/local/bin/
```

#### macOS (Intel)

```bash
curl -L https://github.com/KayneWang/maboroshi/releases/latest/download/maboroshi-macos-x86_64 -o maboroshi
chmod +x maboroshi
sudo mv maboroshi /usr/local/bin/
```

### 方式 2：一键安装脚本

```bash
curl -fsSL https://raw.githubusercontent.com/KayneWang/maboroshi/main/install.sh | sh
```

### 方式 3：从源码编译

```bash
# 克隆仓库
git clone https://github.com/KayneWang/maboroshi.git
cd maboroshi

# 编译并安装
cargo install --path .
```

安装后可以直接运行：

```bash
maboroshi
```

## 🎮 使用方法

### 命令行选项

```bash
maboroshi              # 启动音乐播放器
maboroshi --version    # 显示版本信息
maboroshi --upgrade    # 升级到最新版本
maboroshi --help       # 显示帮助信息
```

### 基本操作

| 按键      | 功能                                            |
| --------- | ----------------------------------------------- |
| `s`       | 进入搜索模式                                    |
| `Enter`   | 确认搜索 / 播放选中的歌曲                       |
| `Esc`     | 取消搜索 / 返回收藏列表                         |
| `↑` / `↓` | 列表选歌 / 搜索模式下浏览历史记录               |
| `←` / `→` | 搜索结果：上一页/下一页 \| 播放：快退/快进      |
| `Space`   | 暂停/继续播放                                   |
| `+` / `-` | 增大/减小音量（步长可配置，默认 ±5%）           |
| `f`       | 收藏/取消收藏（播放中）；移除选中收藏（浏览时） |
| `F`       | 将搜索结果**全部收藏**到当前激活分组            |
| `m`       | 切换播放模式                                    |
| `q`       | 退出播放器                                      |

### 收藏分组管理

| 按键        | 功能                                         |
| ----------- | -------------------------------------------- |
| `Tab`       | 切换到下一个分组                             |
| `Shift+Tab` | 切换到上一个分组                             |
| `g`         | 新建分组（输入名称后 Enter 确认）            |
| `R`         | 重命名当前分组（预填当前名称，可直接修改）   |
| `D`         | 删除当前分组（需按 `y` 二次确认）            |
| `M`         | 将选中歌曲移动到其他分组（浮层选择目标分组） |

### 播放模式

- **随机播放** - 随机播放收藏列表中的歌曲（默认）
- **单曲循环** - 重复播放当前歌曲
- **列表循环** - 循环播放收藏列表
- **顺序播放** - 顺序播放收藏列表，播完停止

### 使用流程

1. **搜索音乐**
   - 按 `s` 进入搜索模式
   - 输入歌曲名或歌手名
   - 按 `Enter` 确认搜索
   - 系统会显示搜索结果（数量由配置的 `max_results` 决定，默认 15 条）

2. **浏览搜索结果**
   - 使用 `↑` `↓` 键在当前页选择歌曲
   - 使用 `←` `→` 键翻页浏览更多结果
   - 支持智能缓存，已访问的页面会瞬间加载
   - 按 `Enter` 播放选中的歌曲

3. **收藏分组管理**
   - 在搜索结果页按 `f` 收藏单首歌曲，按 `F` **一键全部收藏**当前页所有结果
   - 收藏会自动保存到 `~/.maboroshi_favorites.json`
   - 在收藏列表界面：
     - `Tab` / `Shift+Tab` 切换分组
     - `g` 新建分组，`R` 重命名，`D` 删除（二次确认）
     - `M` 将选中歌曲移动到其他分组
     - `f` 直接移除选中歌曲

4. **Playlist 工作流（歌单导入）**
   Maboroshi 支持直接解析 YouTube 或 Bilibili 的歌单（Playlist）链接，这是批量导入歌曲最快的方式：
   - 复制外部歌单的 URL（例如 YouTube Playlist 链接）
   - 按 `s` 进入搜索模式，粘贴链接并按 `Enter`
   - 等待解析完成，结果页会展示歌单内的所有歌曲
   - 按大写 `F`，一键将整张歌单全部保存到当前你所在的分组（自动跳过重复歌曲）
   - 提示：结合"分组管理"功能，你可以先按 `g` 创建一个新分组（比如"日系摇滚"），然后再执行上述导入操作，轻松实现歌单的本地归档。

5. **列表播放**
   - 在收藏列表中使用 `↑` `↓` 选择歌曲
   - 按 `Enter` 播放
   - 歌曲播放完毕会自动播放下一首（根据播放模式）

## 🗂️ 文件位置与缓存清理

- **配置文件**: `~/.config/maboroshi/config.toml`
- **收藏列表**: `~/.maboroshi_favorites.json`（含所有分组数据）
- **离线音频缓存**: `~/.cache/maboroshi/audio/`（用于秒开已播放歌曲）
- **URL 缓存**: 内存中（重启后清空）
- **mpv IPC Socket**: `/tmp/maboroshi.sock`（可配置）

### 🧹 清理音频缓存

为了实现"越用越快"和节省流量，程序会在后台将播放过的音频缓存到本地（受配置项 `offline_audio` 控制）。
注意：**取消收藏不会自动删除对应的缓存文件**。如果你发现硬盘占用过大，可以直接清空缓存目录：

```bash
rm -rf ~/.cache/maboroshi/audio/*
```

## ⚙️ 配置文件

Maboroshi 支持通过配置文件自定义行为。首次运行时会自动在 `~/.config/maboroshi/config.toml` 创建默认配置文件。

### 配置示例

```toml
[search]
# 搜索源：youtube 或 bilibili
source = "youtube"
max_results = 15
timeout = 30
cookies_browser = "chrome"

[cache]
url_cache_size = 30
url_cache_ttl = 7200  # 2 小时

[network]
play_timeout = 10

[playback]
default_mode = "shuffle"  # shuffle, single, list_loop, sequential
seek_seconds = 10         # 快进/快退秒数
volume_step = 5           # 每次按 +/- 调整的音量步长（0–130）

[paths]
socket_path = "/tmp/maboroshi.sock"
favorites_file = "~/.maboroshi_favorites.json"
```

### 支持的搜索源

Maboroshi 支持所有 yt-dlp 兼容的平台，常用选项包括：

- **YouTube** (`source = "yt"` 或 `"youtube"`): 默认搜索源
- **Bilibili** (`source = "bili"`): 哔哩哔哩视频平台
- **SoundCloud** (`source = "soundcloud"`): 音乐分享平台
- **Spotify** (`source = "spotify"`): 需要账号登录
- **Bandcamp** (`source = "bandcamp"`): 独立音乐平台
- **Niconico** (`source = "niconico"`): ニコニコ動画

也可以直接使用 yt-dlp 的搜索前缀格式（如 `"ytsearch"`、`"bilisearch"` 等）。

完整支持列表请查看: [yt-dlp 支持的网站](https://github.com/yt-dlp/yt-dlp/blob/master/supportedsites.md)

更多配置选项请参考 [config.example.toml](config.example.toml)

## 🐛 故障排除

### 搜索失败

- 确保 `yt-dlp` 已正确安装并在 PATH 中
- 检查网络连接
- 尝试更新 yt-dlp: `brew upgrade yt-dlp` 或 `pip install -U yt-dlp`

### 播放失败

- 确保 `mpv` 已正确安装
- 检查 `/tmp/maboroshi.sock` 是否被占用
- 查看日志区域的错误信息

### Chrome Cookie 问题

如果遇到 YouTube 访问限制，yt-dlp 会自动使用 Chrome 的 cookies。确保：

- Chrome 浏览器已安装
- 已登录 YouTube 账号

## 📦 支持的平台

| 平台  | 架构                  | 状态    |
| ----- | --------------------- | ------- |
| macOS | Apple Silicon (ARM64) | ✅ 支持 |
| macOS | Intel (x86_64)        | ✅ 支持 |

## 🤝 贡献

欢迎提交 Issue 和 Pull Request！

查看 [贡献指南](CONTRIBUTING.md) 了解如何参与项目开发。

## 📄 许可证

MIT License

## 🙏 致谢

- [Ratatui](https://github.com/ratatui-org/ratatui) - 优秀的 TUI 框架
- [yt-dlp](https://github.com/yt-dlp/yt-dlp) - 强大的视频下载工具
- [mpv](https://mpv.io/) - 高性能媒体播放器

---

**Maboroshi (幻)** - 在终端中享受音乐 🎵
