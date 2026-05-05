<!-- non-authoritative; port (`7787`) SOT'd in m3u8dl-rs/src/config.rs::Config::default; JSON response shapes SOT'd in m3u8dl-rs/src/http/dto.rs. Keep this in sync if those change. -->

# HTTP API

监听 `127.0.0.1:7787`（不绑公网 IP）。CORS `Access-Control-Allow-Origin: *`（仅 localhost 监听，无外部攻击面）。

## Endpoints

### `GET /ping`
存活探测。

```json
{ "ok": true, "port": 7787, "jobs": 3 }
```

### `GET /status`
列出所有 job 的快照。

```json
{
  "jobs": [
    {
      "id": "a1b2c3d4",
      "title": "《某剧》01",
      "state": "Running",
      "startedAt": "2026-05-02T02:23:11Z",
      "success": null,
      "output": null,
      "sizeMB": null,
      "error": null,
      "progress": { "phase": "downloading", "done": 12, "total": 187, "bytes": 4915200 }
    }
  ]
}
```

`state` 取值（兼容老 PowerShell server）：`Queued` / `Running` / `Completed`。
完成态字段：`success: true` + `output: "<out_dir>/xxx.mp4"` + `sizeMB: 412.3`。
失败态：`success: false` + `error: "<message>"`。
`progress` 字段（`Running` 时存在；`Queued`/`Completed` 时省略）：
- `{ "phase": "parsing" }` — m3u8 解析中
- `{ "phase": "downloading", "done": N, "total": M, "bytes": B }` — 分片下载中（注：polling 路径不含 `seg_index`，全量见 SSE wire）
- `{ "phase": "merging", "pct": F }` — ffmpeg 合并中
所有时间戳走 `chrono::Utc::now().to_rfc3339()`，固定 `Z` 后缀（UTC）。

### `POST /download`
入队一个新 job。

请求体：
```json
{
  "url": "https://cdn.example.com/v/index.m3u8?token=xxx",
  "m3u8": "#EXTM3U\n#EXT-X-VERSION:3\n...",
  "title": "《某剧》01",
  "page": "https://www.example.com/play/12345-1-1.html"
}
```

| 字段 | 必需 | 用途 |
|------|------|------|
| `m3u8` | ✓ | inline m3u8 文本（capture.user.js 拦截到的响应体） |
| `url` | 推荐 | 真实 m3u8 URL，用于推导分片的 base URL（**raw 模式无 url 时相对路径段会失败**） |
| `title` | 可选 | 输出文件名；缺省 `video_<jobId>` |
| `page` | 可选 | 来源页（仅记录，不参与下载） |

响应（`202 Accepted`）：
```json
{ "jobId": "a1b2c3d4", "status": "queued", "title": "《某剧》01" }
```

错误：`400` `{ "error": "m3u8 field empty" }`。

### `GET /job/:id`
单 job 快照（同 `/status` 中一项的 shape）。
`404` 若 id 未知。

### `GET /events/:id` (SSE)
**实时进度推送**。`Content-Type: text/event-stream`。

事件流：
```
event: snapshot
data: {"id":"a1b2c3d4","title":"...","state":"Running","startedAt":"..."}

event: progress
data: {"phase":"parsing"}

event: progress
data: {"phase":"downloading","done":12,"total":187,"bytes":4915200,"seg_index":11}

event: progress
data: {"phase":"merging","pct":73.4}

event: progress
data: {"phase":"done","output":"<out_dir>/xxx.mp4","size_mb":412.3}
```

失败：
```
event: progress
data: {"phase":"failed","error":"network: <details>"}
```

KeepAlive ping 每 30s：`: ping\n\n`（不影响业务事件解析）。

`404` 若 id 未知。

### `OPTIONS *`
CORS preflight，返 `204`。

## 日志 / 运维

- 默认 `tracing-subscriber` 文本格式输出到 stderr。设 `M3U8DL_LOG_FORMAT=json` 切换为 JSONL 一行一事件输出，便于 grep / log 聚合。
- `RUST_LOG` 控制级别（默认 `info,tower_http=warn`）。常用：
  - `RUST_LOG=debug` — 含 axum HTTP request 入站/出站 span
  - `RUST_LOG=info,reqwest=warn,m3u8dl_server=debug` — 仅放开本应用 debug
- 关键 `event` 字段（snake_case，可枚举聚合）：
  `server_started` / `bind_failed` / `out_dir_fallback` / `proxy_selected` / `proxy_parse_failed`
  `fetch_retrying` / `fetch_failed` / `ffmpeg_failed` / `ffmpeg_timeout` / `job_failed` / `job_timeout`
  `sse_subscriber_lagged` / `out_dir_unwritable` / `startup_aborted`
- URL 在日志中只保留 `host + path`（`util::url_redact`）— 签名 token / hmac 在 query string 不会进 log。
- 重定向到文件：Windows `m3u8dl-server.exe 2> server.log` / Unix `... 2> server.log`。

## 浏览器端用法（capture.user.js）

```js
// POST 入队
const r = await fetch('http://127.0.0.1:7787/download', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ url, m3u8, title, page })
});
const { jobId } = await r.json();

// 实时进度
const es = new EventSource('http://127.0.0.1:7787/events/' + jobId);
es.addEventListener('snapshot', e => console.log('init:', JSON.parse(e.data)));
es.addEventListener('progress', e => {
  const ev = JSON.parse(e.data);
  if (ev.phase === 'downloading') ui.progress(ev.done, ev.total);
  if (ev.phase === 'merging') ui.merge(ev.pct);
  if (ev.phase === 'done') { es.close(); ui.done(ev.output, ev.size_mb); }
  if (ev.phase === 'failed') { es.close(); ui.fail(ev.error); }
});
```
