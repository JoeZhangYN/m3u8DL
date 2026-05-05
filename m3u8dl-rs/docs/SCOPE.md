# 支持范围 (SCOPE)

> 本文档说明 m3u8dl-server 当前支持哪些视频源、哪些显式不支持，以及为什么。
> 阅读它能帮你在打开 DevTools 看到一个 m3u8 时，5 秒内判断："这能下吗？"

---

## ✅ 支持

### 协议
- **HLS** (RFC 8216) — 唯一支持的流协议

### Playlist 类型
- **Media playlist** — 直接 `#EXTM3U` + `#EXTINF` 段列表
- **Master playlist** — 自动选择最高 `BANDWIDTH` 的 variant，递归取其 media playlist

### 加密
- **明文 ts**（无加密）
- **AES-128-CBC**（`#EXT-X-KEY:METHOD=AES-128`）+ PKCS7 padding
  - Key 通过 `URI=` 字段从 HTTP 拉取（带项目默认 headers）
  - IV 必须显式 `IV=0xHEX`（暂未实现"用 media-sequence 当 IV"的回退）

### 段属性
- **`#EXT-X-MAP`** 初始化段（取第一段 init）
- **`#EXT-X-BYTERANGE`** 通过 HTTP `Range` 头读取
- **PNG-wrapper** 反爬剥皮（部分源站把 m3u8 文本伪装成 PNG 文件后缀）
- **DevTools 复制压一行**自动正常化（在每个 `#EXT` / `https?://` 前补 `\n`）
- **缺 `#EXT-X-ENDLIST`** 自动补全（避免被识别为直播流）

<!-- retry timings (250→500→1000→2000ms exponential) SOT'd in m3u8dl-rs/src/adapters/reqwest_client.rs::ReqwestClient::fetch_bytes; sync this paragraph if those change. -->

### 网络
- HTTP / HTTPS（rustls，免装 OpenSSL）
- **零配置 headers** — `capture.user.js` 自动从播放页 `location` 推 `Origin` / `Referer`，POST 时透传给 server；任何 HLS 站无需改代码
- 失败重试 — 每段独立 4 次 retry（指数退避 250→500→1000→2000ms）
- 超时 — 单 GET 30s + 连接 10s
- **Windows 系统代理探测** — 自动读 `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings\ProxyServer`，三种格式都支持（裸 host:port / `http=...;https=...` / 完整 URL）；socks 代理跳过

### 输出
- mp4 容器（ffmpeg `-f mpegts -c copy -bsf:a aac_adtstoasc` 重封装）
- 默认输出目录：用户 Downloads 文件夹下的 `m3u8dl/` 子目录（详见 [`config.rs::default_out_dir`](../src/config.rs)；可通过 `M3U8DL_OUT_DIR` 覆盖）
- 文件名 sanitize（去掉 `\\/:*?"<>|`）
- 输出 < 1024 字节视为失败

### 进度反馈
- **SSE** (`GET /events/:jobId`) — 每段下载完成立即 push，合并阶段每 1% 推一次
- 老的 polling (`GET /job/:jobId`) 仍工作，兼容现有 capture.user.js

---

## ❌ 显式不支持

### DRM (Digital Rights Management)

| 技术 | 见于 | 为什么不支持 |
|------|------|------------|
| **Widevine** (CENC) | Netflix, Disney+, HBO Max, B 站大会员独家, 优酷会员高清, 爱奇艺会员, 腾讯视频, YouTube Music premium | Google 的 CDM (Content Decryption Module) 是闭源二进制，license server 要求设备签名 + L1/L3 attestation；纯第三方代码无法解密 |
| **PlayReady** | Microsoft Stream, Sky, iView | 微软同 Widevine — 闭源 CDM + license server 验签 |
| **FairPlay** (`#EXT-X-KEY:METHOD=SAMPLE-AES`) | Apple TV+, Apple Music, 部分 iOS-only HLS 站 | 苹果闭源 CDM；解密 key 需要走 SKD URI 跟 Apple license server 交换，第三方无法触达 |

**判别方法**（在浏览器 DevTools 里 5 秒判断）：
1. 看 Network tab 有 **`.mpd`** 请求 → DASH 协议（不支持，见下）
2. 看请求里有 POST 到 `license.<vendor>.com` 返二进制 blob → DRM 保护，不支持
3. m3u8 文本里有 `#EXT-X-KEY:METHOD=SAMPLE-AES,KEYFORMAT="com.apple.streamingkeydelivery"` → FairPlay，不支持
4. 只有 `METHOD=AES-128` + `URI=` 可被你直接 fetch → **能下**，本工具支持

### 协议
- **DASH** (`.mpd` manifest) — 完全不同的清单格式；HLS 已覆盖目标用户群（个人下载站 / 直播录制 / 公开 HLS 源）的 95% 用例
- **MSS** (Microsoft Smooth Streaming) — 同上

### Tier 1 之外的 HLS 高级特性（按需可加，目前未做）
- 字幕轨独立下载（VTT/TTML 转 SRT）
- 音轨多选（如多语言配音）
- AES-128 没有 IV 时回退到 media-sequence 计算 IV
- 直播流连续录制（当前只处理点播 / 已有 `#EXT-X-ENDLIST` 的 VOD）
- discontinuity-aware 多 init 段处理

---

## 替代方案

如果你确实要下 DRM 内容，**本工具帮不上**，参考方向：
- 自购内容的个人备份 → 用 OBS / 录屏软件
- 抓站点付费内容 → 涉及绕过 DRM，本工具不会提供任何技术支持
- DASH manifest → 用 `yt-dlp` 或 `ffmpeg` 直接处理（功能不同）

如果你的源是**公开 HLS** 但本工具没下载成功，欢迎把 m3u8 样本（脱敏后）反馈，可能是 Tier 1 没覆盖的边界情况。
