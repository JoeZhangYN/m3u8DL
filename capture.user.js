// ==UserScript==
// @name         M3U8 Capture & Auto Download
// @namespace    local-helper
// @version      3.0
// @description  自动捕获任意网站的 m3u8 响应，注入下载按钮 → POST 到本地 server (127.0.0.1:7787)。SSE 实时进度，零配置。
// @match        *://*/*
// @grant        GM_xmlhttpRequest
// @run-at       document-start
// @connect      127.0.0.1
// ==/UserScript==
//
// Zero-config: this script auto-derives anti-hotlink headers (Origin/Referer) from the
// playing page's `location` and forwards them to the server. No site-specific config
// needed. The download button only appears if a .m3u8 response is detected.
//
// Title detection currently targets the maccms template (`div.stui-player__detail > h1`).
// Other site templates fall back to `document.title`. To adapt for a non-maccms site,
// edit `lookupFromDoc()` below.

(function () {
  'use strict';
  const SERVER = 'http://127.0.0.1:7787';
  const seen = new Set();

  function hash(s) {
    let h = 0;
    const n = Math.min(s.length, 2000);
    for (let i = 0; i < n; i++) h = (h * 31 + s.charCodeAt(i)) | 0;
    return h + ':' + s.length;
  }

  function captured(url, text) {
    if (!text || typeof text !== 'string') return;
    if (!text.startsWith('#EXTM3U') && text.indexOf('#EXTM3U') === -1) return;
    const k = hash(text);
    if (seen.has(k)) return;
    seen.add(k);
    showButton(url, text);
  }

  // hook fetch（hls.js 1.x 用 fetch）
  const _fetch = window.fetch;
  window.fetch = function () {
    const args = arguments;
    const p = _fetch.apply(this, args);
    p.then(function (r) {
      try {
        if (!r || !r.ok) return;
        const u = r.url || (typeof args[0] === 'string' ? args[0] : (args[0] && args[0].url));
        if (u && /\.m3u8/i.test(u)) {
          r.clone().text().then(function (t) { captured(u, t); }).catch(function () {});
        }
      } catch (e) {}
    }).catch(function () {});
    return p;
  };

  // hook XHR（老 player 用）
  const _open = XMLHttpRequest.prototype.open;
  const _send = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.open = function (m, u) { this._capUrl = u; return _open.apply(this, arguments); };
  XMLHttpRequest.prototype.send = function () {
    const xhr = this;
    xhr.addEventListener('load', function () {
      try {
        if (xhr._capUrl && /\.m3u8/i.test(String(xhr._capUrl))) {
          captured(xhr._capUrl, xhr.responseText);
        }
      } catch (e) {}
    });
    return _send.apply(this, arguments);
  };

  function lookupFromDoc(doc) {
    try {
      const h1 = doc.querySelector('div.stui-player__detail > h1');
      let name = '';
      if (h1) {
        for (const n of h1.childNodes) {
          if (n.nodeType === Node.TEXT_NODE) name += n.textContent;
        }
        name = name.replace(/ /g, ' ').replace(/\s+/g, ' ').trim();
      }
      let ep = '';
      const data = doc.querySelector('div.stui-player__detail > div.data');
      if (data) {
        const m = data.textContent.match(/当前播放[：:]\s*([^\s ]+)/);
        if (m) ep = m[1].replace(/ /g, '').trim();
      }
      return { name: name, ep: ep };
    } catch (e) {
      return { name: '', ep: '' };
    }
  }

  function safeTopTitle() {
    try { return window.top.document.title; } catch (e) { return ''; }
  }

  function formatTitle(name, ep) {
    let base = name, season = '';
    const m = name.match(/^(.+?)(第[一-龥\d]+季|Season\s*\d+)$/i);
    if (m) { base = m[1].trim(); season = m[2]; }
    let epOut = '';
    if (ep) {
      const em = ep.match(/(\d+)/);
      epOut = em ? em[1] : ep;
    }
    let out = '《' + base + '》' + season;
    if (epOut) out += (season ? ' ' : '') + epOut;
    return out;
  }

  function buildTitle() {
    let r = lookupFromDoc(document);
    if (!r.name) {
      try {
        if (window !== window.top) {
          const r2 = lookupFromDoc(window.top.document);
          if (r2.name) r = r2;
        }
      } catch (e) {}
    }
    let combined;
    if (r.name) {
      combined = formatTitle(r.name, r.ep);
    } else {
      combined = r.ep || safeTopTitle() || document.title || 'video';
    }
    return combined.replace(/[\\/:*?"<>|]/g, '_').trim().slice(0, 100);
  }

  // ─── progress UI ──────────────────────────────────────────────────────────
  // updateButton receives a "progress event" with .phase + variant fields and
  // renders both text and a percentage gradient on the button background.
  function updateButton(btn, ev) {
    if (!ev || !ev.phase) return;
    switch (ev.phase) {
      case 'parsing':
        btn.textContent = '解析 m3u8 ...';
        btn.style.background = '#0078d4';
        break;
      case 'downloading': {
        const pct = ev.total ? Math.round(ev.done / ev.total * 100) : 0;
        const mb  = ev.bytes ? (ev.bytes / 1048576).toFixed(1) : '0.0';
        btn.textContent = '下载 ' + ev.done + '/' + ev.total + ' (' + pct + '% · ' + mb + 'MB)';
        btn.style.background = 'linear-gradient(to right, #107c10 ' + pct + '%, #1f6cb0 ' + pct + '%)';
        break;
      }
      case 'merging': {
        const mpct = ev.pct != null ? Math.round(ev.pct) : 0;
        btn.textContent = '合并中 ' + mpct + '%';
        btn.style.background = 'linear-gradient(to right, #5c2d91 ' + mpct + '%, #107c10 ' + mpct + '%)';
        break;
      }
      case 'done':
        btn.textContent = '✓ 完成 ' + (ev.size_mb != null ? ev.size_mb.toFixed(1) : '?') + ' MB';
        btn.style.background = '#107c10';
        break;
      case 'failed':
        btn.textContent = '✗ ' + (ev.error || '未知错误').slice(0, 40);
        btn.style.background = '#c50f1f';
        break;
    }
  }

  // ─── tracking: try SSE first, fall back to polling on first error ─────────
  function trackJob(btn, jobId) {
    let done = false;
    let pollTimer = null;

    // SSE attempt — works when page is http or server CORS allows. If the page is
    // https and the server is http, browsers may block mixed-content; we then fall back.
    let es;
    let sseGotEvent = false;
    try {
      es = new EventSource(SERVER + '/events/' + jobId);
      es.addEventListener('snapshot', function (e) { sseGotEvent = true; });
      es.addEventListener('progress', function (e) {
        sseGotEvent = true;
        try {
          const ev = JSON.parse(e.data);
          updateButton(btn, ev);
          if (ev.phase === 'done' || ev.phase === 'failed') {
            done = true;
            es.close();
          }
        } catch (err) {}
      });
      es.onerror = function () {
        if (done) return;
        if (!sseGotEvent) {
          // SSE never delivered — switch to polling
          try { es.close(); } catch (e) {}
          startPolling();
        }
        // else: transient SSE blip; EventSource auto-reconnects
      };
    } catch (e) {
      startPolling();
    }

    // Safety: if SSE handshake hangs without error, fall back after 4s
    setTimeout(function () {
      if (!done && !sseGotEvent && !pollTimer) {
        try { es && es.close(); } catch (e) {}
        startPolling();
      }
    }, 4000);

    function startPolling() {
      if (pollTimer || done) return;
      pollTimer = setInterval(function () {
        GM_xmlhttpRequest({
          method: 'GET',
          url: SERVER + '/job/' + jobId,
          timeout: 5000,
          onload: function (r) {
            try {
              const j = JSON.parse(r.responseText);
              if (j.progress) updateButton(btn, j.progress);
              if (j.state === 'Completed') {
                done = true;
                clearInterval(pollTimer);
                if (j.success) {
                  updateButton(btn, { phase: 'done', size_mb: j.sizeMB });
                } else {
                  updateButton(btn, { phase: 'failed', error: j.error || 'unknown' });
                }
              }
            } catch (e) {}
          },
          onerror: function () {},
        });
      }, 1000);
    }
  }

  function showButton(url, text) {
    const segs = text.split(/\r?\n/).filter(function (l) { return l && !l.startsWith('#'); }).length;
    const dur = (text.match(/#EXTINF:([\d.]+)/g) || []).reduce(function (a, m) {
      return a + parseFloat(m.split(':')[1]);
    }, 0);

    const targetDoc = (function () {
      try { return (window !== window.top && window.top.document) ? window.top.document : document; }
      catch (e) { return document; }
    })();

    const btn = targetDoc.createElement('button');
    btn.textContent = '下载 ' + segs + '段 / ' + (dur / 60).toFixed(1) + '分';
    Object.assign(btn.style, {
      position: 'fixed', top: '12px', right: '12px', zIndex: '999999',
      padding: '10px 16px', fontSize: '13px', fontWeight: '600',
      background: '#0078d4', color: '#fff', border: 'none',
      borderRadius: '4px', cursor: 'pointer',
      boxShadow: '0 2px 10px rgba(0,0,0,0.4)',
      fontFamily: 'system-ui, -apple-system, sans-serif',
      minWidth: '220px',
      transition: 'background 0.3s ease',
    });
    const previewTitle = buildTitle();
    btn.title = '保存为: ' + previewTitle + '.mp4\nm3u8: ' + url;
    btn.onclick = function () {
      btn.disabled = true;
      btn.textContent = '发送中...';
      const title = buildTitle();
      // Auto-derive anti-hotlink headers from the playing page — server forwards them
      // to the upstream m3u8/segment fetches, so no per-site server config needed.
      const reqHeaders = {
        'Origin': location.origin,
        'Referer': location.href,
      };
      GM_xmlhttpRequest({
        method: 'POST',
        url: SERVER + '/download',
        data: JSON.stringify({
          url: url,
          m3u8: text,
          title: title,
          page: location.href,
          headers: reqHeaders,
        }),
        headers: { 'Content-Type': 'application/json' },
        timeout: 10000,
        onload: function (r) {
          try {
            const j = JSON.parse(r.responseText);
            btn.textContent = '入队 [' + j.jobId + ']';
            btn.style.background = '#107c10';
            trackJob(btn, j.jobId);
          } catch (e) {
            btn.textContent = '响应异常';
            btn.style.background = '#c50f1f';
          }
        },
        onerror: function () {
          btn.textContent = '没连上 server';
          btn.style.background = '#c50f1f';
          btn.disabled = false;
        },
        ontimeout: function () {
          btn.textContent = '超时（server 没启?）';
          btn.style.background = '#c50f1f';
          btn.disabled = false;
        },
      });
    };
    if (targetDoc.body) targetDoc.body.appendChild(btn);
  }
})();
