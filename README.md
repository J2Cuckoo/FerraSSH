# FerraSSH

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-2.x-24C8DB.svg)](https://tauri.app/)

跨平台 SSH / SFTP 桌面客户端。终端、文件传输、会话与凭据管理放在同一个本地应用里，用来替代 Xshell + XFTP 这类组合工具。

- 主页：[https://hyrubik.com/#/products/ferrassh](https://hyrubik.com/#/products/ferrassh)
- 仓库：[https://github.com/ferra-ssh/ferrassh](https://github.com/ferra-ssh/ferrassh)
- 维护：贵州力贤网络科技有限公司
- 许可：[MIT](./LICENSE)

## 功能

- **SSH 终端**：PTY（`xterm-256color`）、缩放、IME、括号粘贴、本地回显、UTF-8 / 中文 / emoji
- **SFTP**：浏览、上传下载、删除、重命名、权限、符号链接；流式传输、进度、拖拽上传
- **会话**：分组、跳板机（Jump Host / `direct-tcpip`）
- **主机密钥**：首次连接记录 SHA-256 指纹，变更则拒绝连接
- **算法**：会话级 `modern` / `compatible` / `legacy`，方便连老设备
- **保险库**：主密码 → Argon2id → AES-256-GCM，SQLite 本地存储；锁定后密钥清零
- **监控**：当前会话的主机基础指标
- **集群**：多节点项目管理
- **更新**：`latest.json` 与安装包同级分发

## 架构

| 层 | 实现 |
|---|---|
| 桌面壳 | Tauri 2 + React + TypeScript（`src-tauri` + `src/`，产品入口） |
| 核心库 | `crates/ferra-core`：SSH、SFTP、VT、保险库、传输、监控 |
| SSH / SFTP | [russh](https://crates.io/crates/russh) + [russh-sftp](https://crates.io/crates/russh-sftp)，Tokio 异步，无 OpenSSL |
| 终端仿真 | [alacritty_terminal](https://crates.io/crates/alacritty_terminal) |
| 实验壳 | `crates/ferrassh`（egui，可选） |
| 同步节点 | `crates/ferra-syncd`（只存密文 blob + revision） |

```
src/ + src-tauri/     Tauri 桌面产品
        │
        ▼
   ferra-core         协议与业务核心
      ├── russh / russh-sftp
      └── alacritty_terminal
```

## 环境

- Rust 1.85+
- Node.js 18+
- Windows：MSVC，配置见 `.cargo/config.toml`
- macOS / Linux：在目标系统上本地编译，不做交叉编译

## 开发

```bash
git clone https://github.com/ferra-ssh/ferrassh.git
cd ferrassh
npm install

# 桌面产品
npm run tauri -- dev

# 核心库测试（不需要界面）
cargo test -p ferra-core

# 可选：egui 壳
cargo run -p ferrassh

# 可选：同步服务
cargo run -p ferra-syncd
# FERRASSH_SYNCD_BIND=0.0.0.0:7749
```

## 打包

安装包输出到 `dist/`，与 `latest.json` 同级，不要再套一层 `windows/` 之类的子目录。

```bash
npm run pack              # 当前平台
npm run pack:windows      # dist/FerraSSH-*-x64-Setup.exe + dist/latest.json
npm run pack:macos
npm run pack:linux
```

更新文件上传示例目录：`https://files.hyrubik.com/updates/ferrassh/`  
本机保险库和用户数据不会打进安装包。

## 目录

```
src/              React 前端
src-tauri/        Tauri 后端
crates/ferra-core/
crates/ferrassh/      egui 实验壳
crates/ferra-syncd/   同步服务
scripts/          打包与更新说明
dist/             发布产物
```

## 安全

- 主密码不写盘；解锁后的密钥只在内存里，锁定时 `zeroize`
- 默认用现代算法；`legacy` 会打开弱算法，只给老交换机 / 防火墙用，按会话显式打开
- 主机密钥变了就断开，降低中间人风险
- `ferra-syncd` 只看得到密文，解不开内容
- Issue / PR 里不要贴真实密码、私钥

发现问题请私下联系维护方，别在公开 Issue 里贴可利用细节。

## 贡献

1. Fork，开分支
2. 改动尽量小；协议相关逻辑优先放 `ferra-core`
3. 该补测试就补
4. PR 写清楚改了什么、为什么

大改动建议先开 Issue 商量。

## 版本

版本号见 `package.json` 和 Cargo workspace（当前 **0.2.11**）。  
发版说明在 `scripts/update-notes.txt`，打包时写入 `latest.json`。

## 许可

MIT © 2026 贵州力贤网络科技有限公司，见 [LICENSE](./LICENSE)。

软件按现状提供。上线前请自行评估安全与备份策略；启用 `legacy` 即表示接受弱算法风险。
