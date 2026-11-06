# JackettRS

Jackett 官方 C# 版本（[Jackett/Jackett](https://github.com/Jackett/Jackett)）的 Rust 重写，
覆盖核心功能：Cardigann 引擎（YAML 定义驱动）、Torznab、聚合搜索、Blackhole、内置 Vue Web UI。

## 移植基线

- **Upstream**: [Jackett/Jackett@`493ad039958425bbd9f0614734728e8a2f8c1700`](https://github.com/Jackett/Jackett/commit/493ad039958425bbd9f0614734728e8a2f8c1700)（2026-11-03, "0dayfiles: new cats"）
- YAML 索引器定义（`Definitions/`，586 个）即取自该基线的 `src/Jackett.Common/Definitions/`，
  上游定义更新时可直接同步覆盖本仓库的同名目录。

## 结构

```
jackett-common    核心库：索引器框架、Cardigann 引擎、模型、服务
jackett-server    HTTP 服务（axum）：/api/v2.0/*、Torznab、all 聚合、Blackhole、内置 UI
jackett-frontend  Vue 3 + Vite 源码，构建产物单文件输出到 jackett-server/wwwroot
jackett-desktop   Tauri 桌面壳：内嵌 server，独立 P2PSearcher 风格纯搜索界面
jackett-service   Windows 服务包装
jackett-tray      Windows 托盘
jackett-updater   自更新器
```

## 构建

```bash
# 前端（产物被 jackett-server 通过 include_str! 编入二进制，必须先构建）
cd jackett-frontend && npm ci && npm run build

# 服务端（三平台，交叉编译见 .cargo/config.toml 与 .github/workflows/build.yml）
cargo build --release -p jackett-server
```

运行时加载 `Definitions/`（可用环境变量 `JACKETT_DEFINITIONS_DIR` 指定）、
数据目录可用 `-d <dir>`（serverconfig.json、索引器配置）。端口默认 9117。

## 桌面版（纯搜索）

```bash
run_desk.bat          # 或 cargo run -p jackett-desktop
```

桌面版与 Web 管理界面完全分离：没有 Add Indexer、Configured Indexers 或任何配置页，
启动后直接并发搜索 `Definitions/` 里全部 Public 类型索引器（当前 89 个），无需配置。
页面在桌面壳内以 `include_str!` 编译并通过 `/desktop` 提供；搜索走 `/desktop/stream`
（服务端并发 + SSE 边搜边出结果），下载交给 `/desktop/download` 由系统默认 BT 客户端打开磁力/种子。
`/desktop?q=关键词` 可在打开时直接开始搜索。

## 部署（Linux / Armbian 实测流程）

以 ARM64 盒子（Armbian, systemd, 专用 `jackett` 用户）为例，
`v0.1.0` 起可直接使用 GitHub Release 的 `jackett-server-aarch64-unknown-linux-gnu.tar.gz`。

### 1. 首次安装

```bash
# 目录与用户（用户已存在则跳过 useradd）
sudo useradd -r -s /usr/sbin/nologin jackett 2>/dev/null || true
sudo mkdir -p /opt/jackett-rust/Data

# 下载并解压 Release 产物（内含二进制 + Definitions/ + 运行脚本）
wget https://github.com/dzw/JackettRS/releases/latest/download/jackett-server-aarch64-unknown-linux-gnu.tar.gz -O /tmp/jackett.tar.gz
sudo tar -xzf /tmp/jackett.tar.gz -C /opt/jackett-rust/
sudo chmod +x /opt/jackett-rust/jackett-server
sudo chown -R jackett:jackett /opt/jackett-rust
```

### 2. systemd 服务

`/etc/systemd/system/jackett.service`：

```ini
[Unit]
Description=Jackett Service (Rust)
After=network.target

[Service]
Type=simple
User=jackett
Group=jackett
WorkingDirectory=/opt/jackett-rust
Environment=JACKETT_DEFINITIONS_DIR=/opt/jackett-rust/Definitions
ExecStart=/opt/jackett-rust/jackett-server -d /opt/jackett-rust/Data
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload && sudo systemctl enable --now jackett
curl -s http://127.0.0.1:9117/health   # 应返回 OK
```

### 3. 必要的初始配置

服务首次启动会在 Data 目录生成配置；对外访问（局域网）需要改绑定并设置 API key：

```bash
# local_bind_address 默认 127.0.0.1，改为 0.0.0.0 供局域网访问
curl -s -X POST -H "Content-Type: application/json" \
  -d '{"local_bind_address":"0.0.0.0","external":true}' \
  "http://127.0.0.1:9117/api/v2.0/server/config"
sudo systemctl restart jackett
```

API key 可通过 Web UI（`http://<主机>:9117` → Settings）查看/设置；
需要兼容旧客户端时也可以 POST `{"api_key":"..."}` 沿用原 key。

### 4. 接入 qBittorrent（Blackhole 自动加种，实测可用）

原理：Jackett 的下载请求把 .torrent 二进制 / .magnet 文本写入监视目录，
qBittorrent（4.5+，支持 .magnet）扫描后自动加种。

```bash
# 1) 建 watch 目录（放在下载盘上）
sudo mkdir -p /mnt/data/watch/jackett && sudo chown jackett:jackett /mnt/data/watch/jackett

# 2) qBittorrent 配置监视目录（WebUI：设置→下载→监视文件夹，
#    或直接写 /root/.config/qBittorrent/watched_folders.json 后重启 qbittorrent-nox）

# 3) Jackett 配置 blackhole 目录（Settings UI 或 API）
curl -s -X POST -H "Content-Type: application/json" \
  -d '{"blackholedir":"/mnt/data/watch/jackett"}' \
  "http://127.0.0.1:9117/api/v2.0/server/config"
```

链路：Web UI / Torznab 客户端点下载 → 服务端转换磁力/种子 → 落盘 watch 目录 →
qB 每 ~60 秒扫描自动加种。qBittorrent 的 nova3 Jackett 搜索插件也兼容
（依赖虚拟 `all` 聚合索引器：`/api/v2.0/indexers/all/results/torznab/api`）。

### 5. 升级

```bash
wget https://github.com/dzw/JackettRS/releases/latest/download/jackett-server-aarch64-unknown-linux-gnu.tar.gz -O /tmp/jackett.tar.gz
sudo systemctl stop jackett
sudo tar -xzf /tmp/jackett.tar.gz -C /opt/jackett-rust/
sudo chmod +x /opt/jackett-rust/jackett-server && sudo chown -R jackett:jackett /opt/jackett-rust
sudo systemctl start jackett
```

`Data/` 不在发布包内——索引器配置、API key、blackhole 设置升级后原样保留。
若 `Definitions/` 有更新，随包覆盖即可（索引器配置在 Data/ 中，不受影响）。

### 6. Windows（本地开发 / 服务）

Release 的 `jackett-server-x86_64-pc-windows-msvc.tar.gz` 解压后直接运行；
`jackett-service` / `jackett-tray`（Windows 服务与托盘）按需用 `cargo build --release -p jackett-service -p jackett-tray` 构建。
