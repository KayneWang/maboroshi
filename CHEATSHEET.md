# 🎮 Maboroshi 快速参考

## 安装

```bash
# 一键安装
curl -fsSL https://raw.githubusercontent.com/KayneWang/maboroshi/main/install.sh | sh

# 安装依赖
brew install yt-dlp mpv  # macOS
sudo apt install yt-dlp mpv  # Linux
```

## 键盘快捷键

| 按键 | 功能 |
|------|------|
| `s` | 搜索音乐 |
| `Enter` | 确认/播放 |
| `Esc` | 取消/返回 |
| `↑` / `↓` | 上下选择 |
| `Space` | 暂停/继续 |
| `f` | 收藏/取消收藏 |
| `m` | 切换播放模式 |
| `q` | 退出 |

## 播放模式

- 🔂 **单曲循环** - 重复当前歌曲
- 🔁 **列表循环** - 循环播放收藏列表
- ▶️ **顺序播放** - 顺序播放后停止

## 快速使用

1. 运行 `maboroshi`
2. 按 `s` 搜索歌曲
3. 输入歌名，按 `Enter`
4. 用 `↑↓` 选择，按 `Enter` 播放
5. 按 `f` 收藏喜欢的歌曲

## 文件位置

- 收藏列表: `~/.maboroshi_favorites.json`
- mpv Socket: `/tmp/maboroshi.sock`

## 常见问题

**搜索失败？**
```bash
# 更新 yt-dlp
brew upgrade yt-dlp  # macOS
pip install -U yt-dlp  # Linux
```

**播放失败？**
- 检查 mpv 是否安装: `mpv --version`
- 清理旧进程: `pkill mpv`
- 删除旧 socket: `rm /tmp/maboroshi.sock`

## 更多信息

查看完整文档: [README.md](README.md)
