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

## 部署形态（参考：Armbian ARM64 盒子 + qBittorrent）

systemd 服务（`User=jackett`）+ qBittorrent Blackhole 模式：
Jackett 的 `blackholedir` 指向 qB 的监视目录，UI 点下载 → 服务端转换磁力/种子落盘 → qB 自动加种。
