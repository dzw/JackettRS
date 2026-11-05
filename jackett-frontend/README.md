# jackett-frontend

Jackett Rust 版内置 Web 界面的 Vue 3 源码工程。构建产物为**单文件** `index.html`，直接输出到
`../jackett-server/wwwroot/`，由 Rust 服务通过 `include_str!` 编译进二进制，也可通过
`JACKETT_WWWROOT` 外部覆盖。

## 命令

```bash
npm install        # 安装依赖
npm run dev        # 开发服务器（http://localhost:5173，API 代理到 127.0.0.1:9117）
npm run build      # 构建单文件产物到 ../jackett-server/wwwroot/index.html
```

开发时先在本机跑起 `jackett-server`（默认端口 9117），`npm run dev` 的页面即可联调全部 API。

## 结构

```
index.html              Vite 入口
vite.config.js          vue + vite-plugin-singlefile；outDir 指向 jackett-server/wwwroot
src/
  main.js               应用挂载
  App.vue               顶层状态（配置/健康/索引器列表）与弹窗调度
  api.js                /api/v2.0 客户端（config、indexers、available、cache、manualsearch）
  format.js             大小/日期格式化
  styles.css            全局暗色主题（与原内置界面一致的视觉）
  components/
    AppHeader.vue       标题、版本、健康状态、地址徽章
    Modal.vue           通用弹窗（遮罩点击/Esc 关闭）
    ManualSearchModal.vue  手动搜索：跨索引器并行搜索、按做种数排序、部分失败提示
    CacheModal.vue         缓存 releases：客户端过滤
    AddIndexerModal.vue    添加索引器：搜索 + kind 过滤、批量勾选、必填字段展开输入
```

## 对应的服务端 API

| 端点 | 用途 |
| --- | --- |
| `GET /api/v2.0/server/config` | 版本号、api_key |
| `GET /health` | 健康徽章 |
| `GET /api/v2.0/indexers` | 已加载索引器列表 |
| `DELETE /api/v2.0/indexers/:id` | 删除索引器 |
| `GET /api/v2.0/indexers/available` | 可添加索引器定义（id/name/kind/required_fields） |
| `POST /api/v2.0/indexers` | 批量添加 `{ id, fields }` |
| `GET /api/v2.0/indexers/:id/results/manualsearch?q=&cat=` | 手动搜索 |
| `GET /api/v2.0/cache` | 缓存 releases |
