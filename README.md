# m3u8DL

> 浏览器抓 m3u8 → 本机 HTTP 服务下载 → 输出 mp4。
> 5MB Rust 二进制 + ffmpeg + Tampermonkey 脚本。零依赖 PowerShell / .NET / Node。

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey.svg)
![Rust](https://img.shields.io/badge/rust-1.87%2B-orange.svg)

## ✨ 特性

- 浏览器一键抓取 → 自动 POST 到本机 `127.0.0.1:7787`
- **并行分片下载**（默认 16 线程）+ 失败重试（指数退避 250→2000ms × 4）
- **AES-128-CBC 解密**（HLS 标准加密；key 自动 fetch + 缓存）
- **master playlist** 自动选最高码率
- **`#EXT-X-MAP` 初始化段** + **HTTP Range** + **PNG 反爬剥皮**
- **ffmpeg 重封装** mpegts → mp4（无重编码）
- **实时进度推送** — SSE 推每段下载完成事件 + 合并百分比；UI 渐变进度条
- **Windows 系统代理** 自动探测（WinINET 注册表）
- 单一 `.exe` + 双击 `.bat` 启动，无需安装运行时

## ❌ 显式不支持

DRM 内容 — Widevine（Netflix / B 站 VIP / 优酷会员高清 / 腾讯视频付费）/ PlayReady（微软系）/ FairPlay（Apple TV+）。
DASH `.mpd` 也不支持。

为什么不能支持 → [docs/SCOPE.md](m3u8dl-rs/docs/SCOPE.md) 详细解释 + 5 秒判断你的目标站是不是 DRM 的方法。

## 🚀 快速开始

### 用预编译 release

1. 从 [Releases](../../releases) 下载对应平台的归档（Windows: `.zip` / Linux & macOS: `.tar.gz`）解压到任意目录
2. 把 `ffmpeg` 二进制放在同一目录：
   - Windows: [`ffmpeg.exe` from gyan.dev](https://www.gyan.dev/ffmpeg/builds/)（选 essentials）
   - Linux: `apt install ffmpeg` / `dnf install ffmpeg` 然后 `ln -s $(which ffmpeg) ./ffmpeg`，或直接放 PATH
   - macOS: `brew install ffmpeg` 然后 `ln -s $(which ffmpeg) ./ffmpeg`
3. 启动 server：
   - Windows: 双击 `start-server.bat`
   - Linux/macOS: `./start-server.sh`
4. 浏览器装 [Tampermonkey](https://www.tampermonkey.net/)
5. 把 `capture.user.js` 拖入浏览器 → Tampermonkey 提示安装 → 确认
6. 打开任意 HLS 视频播放页 → 右上角自动出现"下载 N 段 / X 分"按钮 → 点击下载

> 默认输出目录：Windows `C:\Folder\Download` / Linux & macOS `~/Downloads/m3u8dl`。可用 `M3U8DL_OUT_DIR` 环境变量覆盖。

### 从源码构建

```cmd
git clone https://github.com/<your>/m3u8DL.git
cd m3u8DL\m3u8dl-rs
cargo build --release
copy target\release\m3u8dl-server.exe ..
cd ..
:: 自行下载 ffmpeg.exe 放到本目录
start-server.bat
```

需 Rust 1.87+（edition 2024 + `is_multiple_of`）。

## 🎯 零配置 — 装上即用

v3.0 起 **不需要为每个站点改任何代码**：

- `capture.user.js` 默认 `@match *://*/*` — 注入到所有页面，但只在抓到 `.m3u8` 时显示按钮
- 浏览器自动从播放页读 `location.origin` / `location.href` 推 `Origin` / `Referer`
- POST 时把这两个 headers 一并发给 server，server 直接转发给上游 CDN
- **任何无 DRM 的 HLS 站都能即装即用**

### 可选调整（多数人不需要）

| 改什么 | 改在哪 | 何时需要 |
|-------|-------|---------|
| 缩小 `@match` 范围 | [`capture.user.js`](capture.user.js) 顶部 `@match` 行 | 不希望脚本注入到无关页面（缩小到常用站） |
| 视频标题 DOM 选择器 | [`capture.user.js`](capture.user.js)::`lookupFromDoc` | 你的站不是 [maccms](https://www.maccms.cn) 模板（默认抓 `div.stui-player__detail > h1`） |
| 标题格式化 | [`capture.user.js`](capture.user.js)::`formatTitle` | 想要不同的文件名格式（默认"《剧名》season epNum"） |
| 默认 User-Agent | [`m3u8dl-rs/src/config.rs`](m3u8dl-rs/src/config.rs) `DEFAULT_HEADERS` | 站点严格校验 UA fingerprint（极少见） |

> 改 `capture.user.js` → Tampermonkey 重装脚本即可，不重启 server。
> 改 `config.rs` → `cd m3u8dl-rs && cargo build --release && copy target\release\m3u8dl-server.exe ..`

### 使用流程

1. 启动 server：双击 `start-server.bat`
2. 浏览器装 Tampermonkey + 拖入 `capture.user.js` 安装
3. 打开任意 HLS 视频站播放页 → 右上角自动出现"下载 N 段 / X 分"按钮
4. 点击 → 按钮渐变进度条实时显示分片进度 / 合并百分比 / 完成大小

不工作时排查：
- 按钮不出现 → 该页没触发 m3u8 fetch（可能视频还没播放过 / 用了 mp4 / 用了 DASH）
- 按钮卡"发送中" → server 没启动或端口冲突（设 `M3U8DL_PORT=7799`）
- 控制台 `network: 403` → 站点 CDN 拒绝；浏览器能播但 server 不能 fetch，多半是 Cookie 缺失（涉及 Cookie 的站需要进阶配置）

## 🏗️ 架构

```
浏览器 (Tampermonkey)                       │   m3u8dl-server.exe (Rust)
  ┌──────────────────┐                     │   ┌─────────────────────┐
  │ capture.user.js  │  POST /download     │   │ axum router         │
  │ ├ hook fetch/XHR │ ──────────────────► │   │ ├ /ping             │
  │ ├ 抓 .m3u8 响应  │                     │   │ ├ /status           │
  │ ├ 注入下载按钮   │  GET /events/:id    │   │ ├ /download         │
  │ └ EventSource    │ ◄────── SSE ─────── │   │ ├ /job/:id (poll)   │
  │   实时进度UI     │                     │   │ └ /events/:id (SSE) │
  └──────────────────┘                     │   └──────────┬──────────┘
                                           │              │ tokio::spawn
                                           │   ┌──────────▼──────────┐
                                           │   │ DownloadJob 编排器  │
                                           │   │ ├ PNG 剥皮          │
                                           │   │ ├ m3u8 normalize    │
                                           │   │ ├ parse + master    │
                                           │   │ ├ 并行 fetch (16x)  │
                                           │   │ │  ├ AES-128 解密   │
                                           │   │ │  └ retry 4x       │
                                           │   │ └ ffmpeg concat+mux │
                                           │   └─────────────────────┘
                                           │              │
                                           │              ▼
                                           │      C:\Folder\Download\*.mp4
```

## 📡 HTTP API

| Method | Path | 说明 |
|--------|------|------|
| `GET`  | `/ping`           | 存活探测 |
| `GET`  | `/status`         | 所有 job 列表 |
| `POST` | `/download`       | 入队新 job → `202 {jobId}` |
| `GET`  | `/job/:id`        | 单 job 快照（含 `progress` 字段） |
| `GET`  | `/events/:id`     | **SSE 实时进度推送** |

完整契约 + JSON shape → [docs/API.md](m3u8dl-rs/docs/API.md)。

## 🛠️ 开发

```cmd
cd m3u8dl-rs
cargo test --release           :: 72 集成测试 + 2 ignored (真 ffmpeg)
cargo clippy --release -- -D warnings
cargo build --release
```

测试覆盖（13 suites，运行 < 2s）：

| Suite | 内容 |
|-------|------|
| `parser_master` / `parser_media` | m3u8 master 选码率 / media 段表 / AES key 继承 / EXT-X-MAP / SAMPLE-AES 拒绝 |
| `aes_decrypt` | AES-128-CBC round-trip / 错块对齐 / 错 key |
| `m3u8_normalize` | DevTools 一行 → 多行 / append ENDLIST / 幂等 |
| `png_strip` | IEND 切割 / 非 PNG passthrough |
| `base_url` | URL 推导 + proptest 不变量 |
| `m3u8_input` | Raw / Url / File 分派 |
| `system_proxy` | WinINET 三种格式解析 |
| `reqwest_client` | 重试 / Range / 超时 |
| `orchestrator` | wiremock e2e / AES key 缓存 / 段并行 |
| `http_routes` | /ping /status /download /job CORS |
| `http_sse` | SSE snapshot + progress event |
| `ffmpeg_muxer` | 真 ffmpeg.exe 集成（`#[ignore]`，`-- --ignored` 跑） |

## 📁 项目结构

```
m3u8DL/
├─ m3u8dl-server.exe        Rust 后端二进制 (cargo 产物，不入 git)
├─ ffmpeg.exe               用户提供 (不入 git)
├─ start-server.bat         启动入口
├─ capture.user.js          Tampermonkey 浏览器脚本
├─ README.md / LICENSE / .gitignore
├─ .github/workflows/ci.yml GitHub Actions CI
├─ m3u8dl-rs/               Rust 源码 (~2400 SLOC, 25 文件)
│   ├─ Cargo.toml
│   ├─ src/
│   │   ├─ main.rs                CLI bootstrap
│   │   ├─ config.rs              端口 / 输出目录 / 默认 headers
│   │   ├─ domain/                类型、不变量、错误（无 IO）
│   │   ├─ ports/                 trait 抽象 (HttpClient / Muxer / ProgressSink)
│   │   ├─ adapters/              端口实现 (reqwest / ffmpeg / broadcast)
│   │   ├─ application/           编排 + 解析 + base_url + segment_fetcher
│   │   └─ http/                  axum routes / dto / sse
│   ├─ tests/                 13 集成测试 suite
│   └─ docs/SCOPE.md          支持范围 / DRM 不支持原因
│       + API.md              HTTP 契约
└─ legacy/                  老 PowerShell 原型 (fallback 备用)
```

## ⚙️ 环境变量

| 变量 | 默认 | 说明 |
|------|------|------|
| `M3U8DL_PORT` | `7787` | HTTP server 监听端口 |
| `M3U8DL_PROXY` | _(自动探测 Windows 系统代理)_ | `none` / 空 = 强制不走代理；`http://127.0.0.1:7890` = 显式代理；不设 = 读 WinINET 注册表 |
| `M3U8DL_OUT_DIR` | `C:\Folder\Download` | 输出 mp4 目录（启动时自动 mkdir，失败即退出） |
| `M3U8DL_FFMPEG` | `ffmpeg.exe` | ffmpeg 路径（相对路径会基于 cwd 解析；建议放绝对路径） |
| `M3U8DL_PARALLELISM` | `16` | 单 job 并行下载分片的 worker 数 |
| `M3U8DL_RETRIES` | `3` | 单分片失败重试次数（指数退避 250→2000ms） |

启动示例：
```cmd
set M3U8DL_PORT=7799
set M3U8DL_PROXY=http://127.0.0.1:7890
start-server.bat
```

## ❓ FAQ

**Q：端口 7787 被占用？**
设环境变量后启动：`set M3U8DL_PORT=7799 && start-server.bat`

**Q：源站在墙外，server 拿不到 m3u8？**
默认会自动用 Windows 系统代理（IE/Edge 设置）。若没生效或想换代理：`set M3U8DL_PROXY=http://127.0.0.1:7890 && start-server.bat`。
强制不走代理：`set M3U8DL_PROXY=none`。

**Q：HTTPS 页面 SSE 收不到？**
HTTPS 页面到 HTTP server 跨协议被浏览器 block；脚本会 4s 内自动 fallback 到 1s polling，仍能看到分片进度（只是延迟 1s 而非真实时）。

**Q：下载失败 "network: 503"？**
站点防盗链 — 改 `m3u8dl-rs/src/config.rs` 里的 `DEFAULT_HEADERS`（Origin / Referer）匹配你的源站，重新 build。

**Q：m3u8 解析失败？**
开 server 控制台看 tracing 输出。带 SAMPLE-AES / Widevine 的 playlist 会 `Unsupported`。可参考 [docs/SCOPE.md](m3u8dl-rs/docs/SCOPE.md) 5 秒判别法。

**Q：能下 Netflix / B 站会员视频吗？**
不能。这些是 Widevine DRM，CDM 闭源 + license server 验签，第三方无法解。

**Q：能添加新站点支持吗？**
通常只需改 `capture.user.js` 的 `@match` + `lookupFromDoc` 标题推断。下载逻辑站点无关。

## 🤝 贡献

- 改 Rust：`cargo clippy -- -D warnings` 通过 + 加测试 + 单文件 ≤150 SLOC
- 加新站点：改 `capture.user.js` 的 `@match` 数组 + 测试。Headers 不同的站可加 PR 让 `config.rs` 按域名映射 preset
- 报 bug：附 m3u8 样本（脱敏 token）+ server 控制台日志

## 📜 License

MIT — 详见 [LICENSE](LICENSE)。

## 🙏 Acknowledgments

- [ffmpeg](https://ffmpeg.org/) — 视频重封装
- [m3u8-rs](https://crates.io/crates/m3u8-rs) — m3u8 解析
- [axum](https://github.com/tokio-rs/axum) / [tokio](https://tokio.rs/) / [reqwest](https://github.com/seanmonstar/reqwest) — async HTTP
- [RustCrypto](https://github.com/RustCrypto) — AES + CBC（纯 Rust，免装 OpenSSL）
- 原 PowerShell 原型 → [`legacy/`](legacy/)（保留作 fallback）
