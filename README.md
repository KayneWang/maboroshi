# 🌀 Maboroshi (幻) - 终端音乐播放器

一个基于 Rust 和 TUI 的轻量级音乐播放器，通过 YouTube 搜索和播放音乐。

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

- 🔍 **YouTube 音乐搜索** - 通过关键词搜索并播放音乐
- ⭐ **收藏管理** - 收藏喜欢的歌曲，快速访问
- 🔄 **多种播放模式** - 单曲循环、列表循环、顺序播放
- 📋 **实时日志** - 查看播放器运行状态和操作记录
- 🎯 **智能滚动** - 搜索结果和收藏列表支持键盘滚动
- 💾 **URL 缓存** - 缓存音频流 URL，加快播放速度
- 🔧 **错误恢复** - 播放失败时自动跳过，继续播放下一首
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

### 方式 3：通过 Cargo 安装

```bash
cargo install --git https://github.com/KayneWang/maboroshi
```

### 方式 4：从源码编译

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

### 基本操作

| 按键      | 功能                      |
| --------- | ------------------------- |
| `s`       | 进入搜索模式              |
| `Enter`   | 确认搜索 / 播放选中的歌曲 |
| `Esc`     | 取消搜索 / 返回收藏列表   |
| `↑` / `↓` | 选择歌曲（在列表中）      |
| `Space`   | 暂停/继续播放             |
| `f`       | 添加/移除收藏             |
| `m`       | 切换播放模式              |
| `q`       | 退出播放器                |

### 播放模式

- **🔂 单曲循环** - 重复播放当前歌曲
- **🔁 列表循环** - 循环播放收藏列表
- **▶️ 顺序播放** - 顺序播放收藏列表，播完停止

### 使用流程

1. **搜索音乐**
   - 按 `s` 进入搜索模式
   - 输入歌曲名或歌手名
   - 按 `Enter` 确认搜索

2. **选择播放**
   - 使用 `↑` `↓` 键选择搜索结果
   - 按 `Enter` 播放选中的歌曲

3. **收藏管理**
   - 播放时按 `f` 添加到收藏
   - 在收藏列表中按 `f` 移除收藏
   - 收藏会自动保存到 `~/.maboroshi_favorites.json`

4. **列表播放**
   - 在收藏列表中使用 `↑` `↓` 选择歌曲
   - 按 `Enter` 播放
   - 歌曲播放完毕会自动播放下一首（根据播放模式）

## 🗂️ 文件位置

- **收藏列表**: `~/.maboroshi_favorites.json`
- **URL 缓存**: 内存中（重启后清空）
- **mpv IPC Socket**: `/tmp/maboroshi.sock`

## 🎯 界面说明

```
┌─ 🌀 Maboroshi - 幻 | 🔁 ──────────────┐
├─ ▶ 米津玄师 - Lemon ⭐ ────────────────┤
│ [████████████░░░░░░░░] 65%            │
├─ ♥ 收藏列表 (6) ──────────────────────┤
│ ♥ 薛之谦 - 演员                        │
│ ▶ 米津玄师 - Lemon                     │
│ ♥ 周杰伦 - 晴天                        │
├─ 📋 日志 ─────────────────────────────┤
│ 清理旧进程和 socket                    │
│ ✓ 使用缓存的 URL                       │
│ 启动 mpv 播放器                        │
│ socket 就绪 (100ms)                    │
├─ 帮助 ────────────────────────────────┤
│ 'q' 退出 | 's' 搜索 | 'f' 收藏 ...    │
└────────────────────────────────────────┘
```

## 🔧 技术栈

- **Rust** - 系统编程语言
- **Ratatui** - 终端 UI 框架
- **Tokio** - 异步运行时
- **yt-dlp** - YouTube 下载工具
- **mpv** - 媒体播放器

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

| 平台    | 架构                  | 状态      |
| ------- | --------------------- | --------- |
| macOS   | Apple Silicon (ARM64) | ✅ 支持   |
| macOS   | Intel (x86_64)        | ✅ 支持   |
| Windows | -                     | ⏳ 计划中 |

## 📄 开发计划

- [ ] 播放历史记录
- [ ] 快进/快退功能
- [ ] 播放队列
- [ ] 导出/导入收藏列表
- [ ] 配置文件支持
- [ ] Windows 平台支持

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
