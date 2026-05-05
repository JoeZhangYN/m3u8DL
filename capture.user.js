// ==UserScript==
// @name         M3U8 Capture & Auto Download
// @namespace    local-helper
// @version      3.10
// @description  自动捕获任意网站的 m3u8 响应，按视频位置注入下载按钮 → POST 到本地 server (127.0.0.1:7787)。SSE 实时进度，零配置。
// @match        *://*/*
// @grant        GM_xmlhttpRequest
// @run-at       document-start
// @connect      127.0.0.1
// @connect      *
// ==/UserScript==
//
// Zero-config: this script auto-derives anti-hotlink headers (Origin/Referer) from the
// playing page's `location` and forwards them to the server.
//
// v3.1: per-video button (anchored at video's bottom-left). Multi-video pages now
// get one button per stream. Also actively scans <video> tags for inline m3u8 in
// src / <source> / data-* attributes — sites that embed m3u8 directly no longer
// require waiting for a network capture.
//
// Title detection currently targets the maccms template (`div.stui-player__detail > h1`).

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

  function captured(url, text, videoEl) {
    if (!text || typeof text !== 'string') return;
    if (!text.startsWith('#EXTM3U') && text.indexOf('#EXTM3U') === -1) return;
    if (url && seen.has(url)) return;
    const k = hash(text);
    if (seen.has(k)) return;
    if (url) seen.add(url);
    seen.add(k);
    const v = videoEl || findVideoForUrl(url);
    showButton(url, text, v);
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

  // ─── video discovery ──────────────────────────────────────────────────────
  function allVideos() {
    const list = [];
    try { list.push.apply(list, Array.from(document.querySelectorAll('video'))); } catch (e) {}
    try {
      if (window !== window.top) {
        list.push.apply(list, Array.from(window.top.document.querySelectorAll('video')));
      }
    } catch (e) {}
    return list;
  }

  function videoSourceUrls(v) {
    const urls = [];
    if (v.src) urls.push(v.src);
    if (v.currentSrc) urls.push(v.currentSrc);
    try {
      for (const s of v.querySelectorAll('source')) {
        if (s.src) urls.push(s.src);
      }
    } catch (e) {}
    if (v.dataset) {
      for (const k in v.dataset) {
        const val = v.dataset[k];
        if (val && typeof val === 'string') urls.push(val);
      }
    }
    return urls;
  }

  function findVideoForUrl(url) {
    const candidates = allVideos();
    // Fast-path: single video on page → always use it, skip URL-matching dance.
    // hls.js / shaka set <video src=blob:...> so source URL matching never works.
    if (candidates.length === 1) return candidates[0];
    for (const v of candidates) {
      for (const u of videoSourceUrls(v)) {
        if (u && !/^blob:/i.test(u)
            && (u === url || u.indexOf(url) !== -1 || url.indexOf(u) !== -1)) {
          return v;
        }
      }
    }
    // fallback: first video without an attached button
    for (const v of candidates) {
      if (!v.dataset.__m3u8dlAttached) return v;
    }
    return candidates[0] || null;
  }

  function lookupFromDoc(doc) {
    try {
      const h1 = doc.querySelector('div.stui-player__detail > h1');
      let name = '';
      if (h1) {
        for (const n of h1.childNodes) {
          if (n.nodeType === Node.TEXT_NODE) name += n.textContent;
        }
        name = name.replace(/ /g, ' ').replace(/\s+/g, ' ').trim();
      }
      let ep = '';
      const data = doc.querySelector('div.stui-player__detail > div.data');
      if (data) {
        const m = data.textContent.match(/当前播放[：:]\s*([^\s ]+)/);
        if (m) ep = m[1].replace(/ /g, '').trim();
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
    let es;
    let sseGotEvent = false;
    try {
      es = new EventSource(SERVER + '/events/' + jobId);
      es.addEventListener('snapshot', function () { sseGotEvent = true; });
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
          try { es.close(); } catch (e) {}
          startPolling();
        }
      };
    } catch (e) {
      startPolling();
    }

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

  // ─── button placement ─────────────────────────────────────────────────────
  // Resolve the *player wrapper* enclosing a <video>, not just the video itself.
  // Reason: most players (DPlayer/Artplayer/video.js/xgplayer/...) put their
  // control bar BELOW the <video>, inside a wrapper. Anchoring to <video>.bottom
  // lands the button inside the control area; anchoring to wrapper.bottom puts
  // it below the whole player.
  // Strategy:
  //   1. fast path — match well-known player wrapper class via closest()
  //   2. fallback — walk up while parent's size is ~equal to video's
  //      (a parent that grows ≥5px in either axis is the wrapper boundary)
  const KNOWN_PLAYER_SEL = [
    '.dplayer',
    '.art-video-player',
    '.video-js',
    '.xgplayer',
    '.jwplayer',
    '.plyr',
    '.shaka-video-container',
    '.vjs-tech-wrapper',
  ].join(',');

  function getPlayerContainer(v) {
    const known = v.closest(KNOWN_PLAYER_SEL);
    if (known) return known;
    let el = v, parent = el.parentElement;
    const baseW = el.clientWidth || 1, baseH = el.clientHeight || 1;
    const doc = el.ownerDocument || document;
    const stop = doc.body;
    for (let depth = 0; parent && parent !== stop && depth < 8; depth++) {
      if (parent.clientWidth - baseW >= 5 || parent.clientHeight - baseH >= 5) break;
      el = parent;
      parent = el.parentElement;
    }
    return el;
  }

  function anchorToVideo(btn, videoEl) {
    videoEl.dataset.__m3u8dlAttached = '1';
    const container = getPlayerContainer(videoEl);
    const win = (videoEl.ownerDocument && videoEl.ownerDocument.defaultView) || window;

    // The page's DOM may have NO container larger than the video — every
    // ancestor is laid out tight to the video, often with overflow:hidden.
    // Placing the button outside the container (body-anchored absolute)
    // lands it in a clipped / off-flow region and it never paints.
    // Solution: inject button INTO the player container as a child. Then
    // it inherits the container's stacking context, visibility, fullscreen
    // behaviour, and naturally appears overlaid at top-left of the player.
    Object.assign(btn.style, {
      position: 'absolute',
      top: '8px',
      left: '8px',
      right: 'auto',
      bottom: 'auto',
      opacity: '0.35',
    });
    btn.addEventListener('mouseenter', function () { btn.style.opacity = '0.95'; });
    btn.addEventListener('mouseleave', function () { btn.style.opacity = '0.35'; });

    // Container must be a positioned ancestor for absolute child to anchor.
    try {
      const cs = win.getComputedStyle(container);
      if (cs && cs.position === 'static') container.style.position = 'relative';
    } catch (e) {}
    container.appendChild(btn);

    // Per-container button registry for de-dup. The two capture paths
    // (<video src=...> scanner vs fetch hook) may produce non-equal URL
    // strings (auth_key variants, scheme differences) → both pass URL-level
    // dedup and stack two overlapping buttons. Track all buttons per
    // container; whoever ends up with parsed text evicts text-less siblings.
    btn.__m3u8dlContainer = container;
    if (!container.__m3u8dlBtns) container.__m3u8dlBtns = [];
    container.__m3u8dlBtns.push(btn);
    reconcileSiblings(btn);

    // Visibility tracking: if container collapses (player destroyed / hidden),
    // hide the button. No coordinate math needed — layout is automatic.
    function check() {
      if (!btn.isConnected) return;
      const r = container.getBoundingClientRect();
      btn.style.display = (r.width <= 0 || r.height <= 0) ? 'none' : '';
    }
    check();
    setInterval(check, 1000);
  }

  // Container-level dedup with quality tiers. Called from:
  //   1. relabel() success → my score may have risen, knock losers
  //   2. bg fetch fail / 12s timeout → my score is still 1, may need self-evict
  // Tiers (higher wins; ties → keep both, possibly different streams):
  //  99 = active (user clicked → tracking job → must NOT be culled or shadowed)
  //   3 = variant playlist (has #EXTINF segments → real download)
  //   2 = master playlist  (#EXT-X-STREAM-INF, no segments → server may resolve)
  //   1 = textless         (URL only, fetch pending or failed)
  function btnTier(b) {
    if (b.__m3u8dlActive) return 99;
    if (!b.__m3u8dlHasText) return 1;
    return b.__m3u8dlIsMaster ? 2 : 3;
  }

  function reconcileSiblings(arrived) {
    const c = arrived.__m3u8dlContainer;
    if (!c || !c.__m3u8dlBtns) return;
    let top = 0;
    for (const b of c.__m3u8dlBtns) {
      if (b.isConnected) top = Math.max(top, btnTier(b));
    }
    const remaining = [];
    for (const b of c.__m3u8dlBtns) {
      if (!b.isConnected) continue;
      if (btnTier(b) < top) {
        const why = b.__m3u8dlHasText ? 'master shadowed by variant' : 'textless shadowed';
        console.log('[m3u8dl] removed lower-tier button (' + why + ')');
        b.remove();
        continue;
      }
      remaining.push(b);
    }
    c.__m3u8dlBtns = remaining;
  }

  function parseM3u8Stats(text) {
    if (!text || text.indexOf('#EXTM3U') === -1) return null;
    const isMaster = text.indexOf('#EXT-X-STREAM-INF') !== -1;
    const segs = text.split(/\r?\n/).filter(function (l) { return l && !l.startsWith('#'); }).length;
    const dur = (text.match(/#EXTINF:([\d.]+)/g) || []).reduce(function (a, m) {
      return a + parseFloat(m.split(':')[1]);
    }, 0);
    return { segs: segs, dur: dur, isMaster: isMaster };
  }

  function showButton(url, text, videoEl) {
    // Suppress new buttons in containers that already have an active download.
    // Otherwise live-playlist refreshes / extra variant captures keep stacking
    // fresh buttons next to the one the user clicked.
    if (videoEl) {
      try {
        const c = getPlayerContainer(videoEl);
        if (c && c.__m3u8dlActiveBtn && c.__m3u8dlActiveBtn.isConnected) {
          console.log('[m3u8dl] suppressed new button (container has active download): ' + url);
          return;
        }
      } catch (e) {}
    }
    let m3u8Text = text || '';
    const targetDoc = (function () {
      try { return (window !== window.top && window.top.document) ? window.top.document : document; }
      catch (e) { return document; }
    })();

    const btn = targetDoc.createElement('button');
    function relabel() {
      const s = parseM3u8Stats(m3u8Text);
      if (s) {
        btn.__m3u8dlHasText = true;
        btn.__m3u8dlIsMaster = s.isMaster;
        reconcileSiblings(btn);
      }
      if (!s) btn.textContent = '↓ m3u8';
      else if (s.isMaster) btn.textContent = '↓ master(' + s.segs + ')';
      else btn.textContent = '↓ ' + s.segs + '段 ' + Math.round(s.dur / 60) + '分';
    }
    relabel();
    Object.assign(btn.style, {
      zIndex: '2147483647',
      padding: '3px 8px', fontSize: '11px', fontWeight: '500',
      background: '#0078d4', color: '#fff', border: 'none',
      borderRadius: '3px', cursor: 'pointer',
      boxShadow: '0 1px 4px rgba(0,0,0,0.4)',
      fontFamily: 'system-ui, -apple-system, sans-serif',
      transition: 'opacity 0.2s ease, background 0.3s ease',
      lineHeight: '1.4',
    });
    const previewTitle = buildTitle();
    btn.title = '保存为: ' + previewTitle + '.mp4\nm3u8: ' + url;
    function sendToServer() {
      // Mark active: from this point on, reconcileSiblings treats this button
      // as tier 99 (never shadowed), AND showButton will refuse to spawn new
      // siblings in the same container. Otherwise post-click captures (live
      // playlist refreshes / different bitrate variants) could spawn fresh
      // buttons that visually look like "the new progress button".
      btn.__m3u8dlActive = true;
      const c = btn.__m3u8dlContainer;
      if (c) c.__m3u8dlActiveBtn = btn;
      const title = buildTitle();
      const reqHeaders = {
        'Origin': location.origin,
        'Referer': location.href,
      };
      GM_xmlhttpRequest({
        method: 'POST',
        url: SERVER + '/download',
        data: JSON.stringify({
          url: url,
          m3u8: m3u8Text,
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
            console.warn('[m3u8dl] server response parse fail:', r && r.responseText);
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
    }

    btn.onclick = function () {
      btn.disabled = true;
      btn.textContent = '发送中...';
      if (m3u8Text && m3u8Text.indexOf('#EXTM3U') !== -1) {
        sendToServer();
        return;
      }
      // No text yet — fetch on demand, then send
      btn.textContent = '拉取 m3u8 ...';
      fetchM3u8Text(url, function (t, errTag) {
        if (t && t.indexOf('#EXTM3U') !== -1) {
          m3u8Text = t;
          relabel();
          btn.textContent = '发送中...';
          sendToServer();
        } else {
          console.warn('[m3u8dl] click-time fetch failed (' + errTag + '), sending URL only');
          // Server-side may still re-fetch given URL + headers — try anyway
          sendToServer();
        }
      });
    };

    if (videoEl) {
      anchorToVideo(btn, videoEl);
    } else {
      Object.assign(btn.style, { position: 'fixed', top: '12px', right: '12px' });
      if (targetDoc.body) targetDoc.body.appendChild(btn);
    }

    // Lazy-enrich: if no text yet, try fetching in background to upgrade label
    if (!m3u8Text) {
      fetchM3u8Text(url, function (t, errTag) {
        if (t && t.indexOf('#EXTM3U') !== -1) {
          m3u8Text = t;
          relabel();
        } else {
          console.warn('[m3u8dl] background fetch m3u8 fail (' + errTag + ') for ' + url);
          reconcileSiblings(btn);
        }
      });
      // Backup timeout: even if fetch is still pending after 12s, let
      // reconcileSiblings cull us if a higher-tier sibling has appeared.
      setTimeout(function () { if (!btn.__m3u8dlHasText) reconcileSiblings(btn); }, 12000);
    }
  }

  // Two-stage fetch: page-context fetch (works if CORS open), then GM_xmlhttpRequest
  // (works cross-origin if @connect granted). Either succeeding is enough.
  function fetchM3u8Text(url, cb) {
    let done = false;
    function finish(t, tag) { if (done) return; done = true; cb(t, tag); }
    try {
      fetch(url, { credentials: 'include' })
        .then(function (r) {
          if (!r.ok) throw new Error('http ' + r.status);
          return r.text();
        })
        .then(function (t) { finish(t, 'fetch:ok'); })
        .catch(function (e) {
          tryGM();
          if (!done) console.warn('[m3u8dl] fetch() failed, retry via GM:', e && e.message);
        });
    } catch (e) {
      tryGM();
    }
    function tryGM() {
      if (done) return;
      try {
        GM_xmlhttpRequest({
          method: 'GET', url: url, timeout: 10000,
          onload: function (r) { finish(r && r.responseText, 'gm:ok:' + (r && r.status)); },
          onerror: function (r) { finish(null, 'gm:err'); },
          ontimeout: function () { finish(null, 'gm:timeout'); },
        });
      } catch (e) { finish(null, 'gm:throw'); }
    }
    setTimeout(function () { if (!done) finish(null, 'overall:timeout'); }, 12000);
  }

  // ─── direct <video> tag scanner ───────────────────────────────────────────
  // Sites that embed m3u8 directly in <video src> / <source> / data-* won't
  // generate network captures. We show the button as soon as we see the URL;
  // m3u8 text is fetched lazily (background + on-click), so a CORS/anti-hotlink
  // 403 on the manifest doesn't suppress the button.

  // bind once per <video>; react to src/source/dataset changes + media events.
  // DPlayer / video.js / shaka / hls.js wrappers commonly add the <video> first
  // and set src later — single-shot scan misses these. Re-check on each event.
  function bindVideo(v) {
    if (v.__m3u8dlBound) return;
    v.__m3u8dlBound = true;

    function tryCapture() {
      if (v.__m3u8dlCaptured) return;
      for (const u of videoSourceUrls(v)) {
        if (u && /\.m3u8/i.test(u) && !/^blob:/i.test(u)) {
          v.__m3u8dlCaptured = true;
          console.log('[m3u8dl] found m3u8 in <video>:', u);
          if (seen.has(u)) return;
          seen.add(u);
          showButton(u, '', v);
          return;
        }
      }
    }

    tryCapture();
    if (window.MutationObserver) {
      try {
        new MutationObserver(tryCapture).observe(v, {
          attributes: true,
          attributeFilter: ['src'],
          childList: true,
          subtree: true,
        });
      } catch (e) {}
    }
    for (const ev of ['loadstart', 'loadedmetadata', 'canplay', 'play', 'emptied']) {
      try { v.addEventListener(ev, tryCapture); } catch (e) {}
    }
  }

  function bindAll() {
    for (const v of allVideos()) bindVideo(v);
  }

  function startScanner() {
    console.log('[m3u8dl] scanner started');
    bindAll();
    if (window.MutationObserver) {
      try {
        const root = document.documentElement || document.body;
        if (root) new MutationObserver(bindAll).observe(root, { childList: true, subtree: true });
      } catch (e) {}
    }
    setInterval(bindAll, 2000);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', startScanner);
  } else {
    startScanner();
  }
})();
