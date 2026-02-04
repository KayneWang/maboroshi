# 贡献指南

感谢你对 Maboroshi 的关注！我们欢迎任何形式的贡献。

## 🐛 报告 Bug

如果你发现了 bug，请在 [Issues](https://github.com/KayneWang/maboroshi/issues) 中创建一个新的 issue，并包含：

- 问题的详细描述
- 复现步骤
- 预期行为和实际行为
- 你的系统信息（操作系统、版本等）
- 相关的日志或截图

## 💡 提出新功能

如果你有好的想法，欢迎创建 Feature Request issue，描述：

- 功能的用途和场景
- 预期的使用方式
- 可能的实现思路（可选）

## 🔧 提交代码

### 开发环境设置

1. Fork 本仓库
2. 克隆你的 fork：
   ```bash
   git clone https://github.com/your-username/maboroshi.git
   cd maboroshi
   ```

3. 安装依赖：
   ```bash
   # 系统依赖
   brew install yt-dlp mpv  # macOS
   # sudo apt install yt-dlp mpv  # Linux
   
   # Rust 工具链（如果还没安装）
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

4. 运行项目：
   ```bash
   cargo run
   ```

### 代码规范

- 使用 `cargo fmt` 格式化代码
- 使用 `cargo clippy` 检查代码质量
- 确保代码通过 `cargo test`

### 提交流程

1. 创建新分支：
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. 进行修改并提交：
   ```bash
   git add .
   git commit -m "feat: add your feature description"
   ```

3. 推送到你的 fork：
   ```bash
   git push origin feature/your-feature-name
   ```

4. 在 GitHub 上创建 Pull Request

### Commit 消息规范

使用 [Conventional Commits](https://www.conventionalcommits.org/) 格式：

- `feat:` 新功能
- `fix:` Bug 修复
- `docs:` 文档更新
- `style:` 代码格式调整
- `refactor:` 代码重构
- `test:` 测试相关
- `chore:` 构建/工具相关

示例：
```
feat: add playlist export feature
fix: resolve crash when mpv is not installed
docs: update installation instructions
```

## 📝 文档贡献

文档改进同样重要！如果你发现文档中的错误或不清楚的地方，欢迎提交 PR。

## ❓ 问题讨论

如果你有任何问题或想法，可以：

- 在 [Issues](https://github.com/KayneWang/maboroshi/issues) 中提问
- 在 [Discussions](https://github.com/KayneWang/maboroshi/discussions) 中讨论

## 📜 许可证

提交代码即表示你同意将代码以 MIT 许可证发布。
