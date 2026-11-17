/*
 * GeTube — YouTube sayfa arayüzü (content script).
 *
 * MP4 / MP3 düğmeleri v1.0.8 davranışını korur; ✂ düğmesi zaman aralığı (kesit)
 * panelini açar. Kesit aralığı GeTubeClipTime ile doğrulanır ve saniye olarak
 * gönderilir; native host ham metin görmez.
 */
(() => {
  'use strict';

  const CT = window.RuviTubeClipTime || window.GeTubeClipTime;
  const UI_ID = 'getube-ui';
  const STYLE_ID = 'getube-style';
  const PANEL_ID = 'getube-clip-host';
  const SETTINGS_KEY = 'ruvitube.settings';

  const DEFAULTS = {
    openDownloads: false,
    openFile: false,
    openFolder: false,
    videoQuality: '720p',
    audioQuality: '320kbps',
    clipLength: 30,
    jobs: 4
  };

  const VALID_JOBS = [1, 2, 4, 8, 16];
  function normalizeJobs(raw) {
    const n = Math.round(Number(raw));
    if (!Number.isFinite(n)) return DEFAULTS.jobs;
    if (VALID_JOBS.includes(n)) return n;
    return Math.min(16, Math.max(1, n));
  }

  const QUALITY_LABELS = {
    '1080p': '1080p (MP4)', '720p': '720p (MP4)', '480p': '480p (MP4)', 'best': 'En iyi (MP4)'
  };
  const AUDIO_LABELS = { '320kbps': '320 kbps', '192kbps': '192 kbps' };
  const JOBS_LABELS = {
    '1': '1 parça (--jobs 1)',
    '2': '2 parça (--jobs 2)',
    '4': '4 parça (--jobs 4)',
    '8': '8 parça (--jobs 8)',
    '16': '16 parça (--jobs 16)'
  };
  const CHIPS = [15, 30, 60, 180, 300, 600];

  const state = window.__RUVITUBE_STATE__ || window.__GETUBE_STATE__ || (window.__RUVITUBE_STATE__ = {
    jobs: new Map(),
    listener: false,
    settings: Object.assign({}, DEFAULTS),
    panel: null
  });

  // Arayüz Türkçe: chrome.i18n tarayıcı diline göre değişir; arayüz her zaman Türkçe.
  const TR = {
    btnClipTitle: 'Zaman aralığı ile indir',
    clipTitle: 'Kesit indir',
    clipSubtitle: 'Video ve ses için başlangıç–bitiş zamanı seçin.',
    clipStart: 'Başlangıç',
    clipEnd: 'Bitiş',
    clipNow: 'Şimdi',
    clipEndNow: 'Şimdi',
    clipClose: 'Kapat',
    clipDownloadVideo: 'MP4 indir',
    clipDownloadAudio: 'MP3 indir',
    clipJobs: 'Paralel (--jobs)',
    clipFull: 'Tam videoyu indir',
    clipPlaceholder: '1:23 veya 01:23:45',
    clipChapters: 'Bölümler',
    clipSelectChapter: '-- Bölüm seçin --',
    clipNoChapters: '-- Bölüm bulunamadı (Manuel girin) --',
    clipManualParse: 'Manuel Ayrıştır',
    clipManualHint: 'Zaman damgalarını buraya yapıştırın:\n00:00 Giriş\n03:15 Konu',
    clipParseBtn: 'Ayrıştır',
    clipDlChapterVideo: 'Bölümü İndir (MP4)',
    clipDlChapterAudio: 'Bölümü İndir (MP3)',
    errFormat: 'Zaman biçimi geçersiz. Örnek: 1:23 veya 01:23:45',
    errOrder: 'Bitiş zamanı başlangıçtan sonra olmalı.',
    errShort: 'Aralık en az 1 saniye olmalı.',
    errLong: 'Aralık video sınırının dışında.',
    errRange: 'Geçerli bir zaman aralığı girin.',
    statusPreparing: 'Hazırlanıyor...',
    statusDownloading: 'İndiriliyor...',
    statusDone: 'Tamamlandı',
    statusError: 'Hata',
    optOpenFolder: 'İndirme klasörünü aç',
    optOpenFile: 'İndirilen dosyayı aç',
    optOpenDownloads: "Chrome İndirilenler'i aç"
  };
  const t = (key, fallback) => TR[key] || fallback || key;

  /* ------------------------------------------------------------------ CSS */

  function addStyle() {
    if (document.getElementById(STYLE_ID)) return;
    const style = document.createElement('style');
    style.id = STYLE_ID;
    style.textContent = `
      .getube-container{display:inline-flex!important;flex-flow:row nowrap!important;align-items:center;justify-content:flex-start;gap:5px;margin-left:8px;position:relative;font-family:Roboto,Arial,sans-serif;z-index:2000;vertical-align:middle;white-space:nowrap!important;flex:0 0 auto!important;min-width:max-content}
      .getube-btn{--dl-progress:0%;position:relative;overflow:hidden;background-color:var(--yt-spec-badge-chip-background,rgba(255,255,255,.1));color:var(--yt-spec-text-primary,#fff);border:0;border-radius:18px;padding:0 7px;height:40px;font-size:12px;font-weight:500;display:inline-flex;align-items:center;justify-content:center;gap:3px;cursor:pointer;box-sizing:border-box;transition:background .2s,color .2s;line-height:normal;white-space:nowrap;width:clamp(58px,6vw,80px);min-width:58px;max-width:80px;flex:0 0 auto!important}
      .getube-btn:hover{background-color:var(--yt-spec-raised-background,rgba(255,255,255,.2))}
      .getube-btn[disabled]{opacity:1;cursor:default}
      .getube-btn.is-active .getube-label{display:none}
      .getube-btn.is-active .getube-content{visibility:visible}
      .getube-progress{position:absolute;inset:0 auto 0 0;width:var(--dl-progress);background:linear-gradient(90deg,rgba(255,0,0,.25),rgba(255,0,0,.48));transition:width .25s ease;pointer-events:none}
      .getube-btn.is-active .getube-progress{animation:getubePulse 1.2s ease-in-out infinite}
      .getube-btn.is-done .getube-progress{width:100%;background:rgba(46,204,113,.42);animation:none}
      .getube-btn.is-error .getube-progress{width:100%;background:rgba(220,53,69,.42);animation:none}
      .getube-btn .getube-content{position:relative;z-index:1;display:inline-flex;align-items:center;gap:5px;text-shadow:0 1px 2px rgba(0,0,0,.3)}
      .getube-btn .getube-percent{font-size:12px;font-weight:600;min-width:30px;text-align:center}
      .getube-clip-btn{width:40px;min-width:40px;max-width:40px;padding:0}
      .getube-status{font-size:11px;color:var(--yt-spec-text-secondary,#aaa);margin-left:4px;max-width:180px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;display:none}
      @keyframes getubePulse{0%,100%{filter:brightness(1)}50%{filter:brightness(1.22)}}
      @media (max-width:700px){
        .getube-container{gap:4px;margin-left:5px}
        .getube-btn{height:36px;padding:0 5px;font-size:11px;width:58px;min-width:58px}
        .getube-clip-btn{width:36px;min-width:36px}
        .getube-btn svg{width:14px;height:14px}
      }
    `;
    document.head.appendChild(style);
  }

  /* --------------------------------------------------------------- helpers */

  function getTarget() {
    return document.querySelector('ytd-watch-metadata #subscribe-button') ||
      document.querySelector('ytd-watch-metadata ytd-subscribe-button-renderer') ||
      document.querySelector('#owner #subscribe-button') ||
      document.querySelector('#owner ytd-subscribe-button-renderer') ||
      document.querySelector('#top-row #actions') ||
      document.querySelector('ytd-watch-metadata #top-row #actions') ||
      document.querySelector('#above-the-fold #top-row #actions') ||
      document.querySelector('ytd-watch-metadata #actions') ||
      document.querySelector('#menu #actions') ||
      document.querySelector('#actions');
  }

  function isSubscribeAnchor(node) {
    return !!node && (node.id === 'subscribe-button' || node.matches('ytd-subscribe-button-renderer'));
  }

  function placeContainer(container, target) {
    if (isSubscribeAnchor(target)) target.insertAdjacentElement('afterend', container);
    else target.appendChild(container);
  }

  function playbackTime() {
    const video = document.querySelector('video');
    const t0 = video && isFinite(video.currentTime) ? video.currentTime : 0;
    return Math.max(0, Math.round(t0 * 10) / 10);
  }

  function videoDuration() {
    const video = document.querySelector('video');
    return video && isFinite(video.duration) && video.duration > 0 ? video.duration : 0;
  }

  /* ------------------------------------------------------------- chapters */

  function buildChapterRanges(list) {
    if (!list || !list.length) return [];
    list.sort((a, b) => a.startSec - b.startSec);
    const unique = [];
    for (const item of list) {
      if (!unique.length || unique[unique.length - 1].startSec !== item.startSec) {
        unique.push(item);
      }
    }
    const duration = videoDuration();
    const chapters = [];
    for (let i = 0; i < unique.length; i++) {
      const startSec = unique[i].startSec;
      let endSec;
      if (i < unique.length - 1) {
        endSec = unique[i + 1].startSec;
      } else {
        endSec = duration > startSec ? duration : startSec + 300;
      }
      if (endSec <= startSec) endSec = startSec + 30;
      chapters.push({
        startSec: Math.round(startSec * 10) / 10,
        endSec: Math.round(endSec * 10) / 10,
        title: unique[i].title,
        label: CT.formatClock(startSec) + ' – ' + CT.formatClock(endSec) + ' ' + unique[i].title
      });
    }
    return chapters;
  }

  function parseManualChapterText(text) {
    if (!text || typeof text !== 'string') return [];
    const lines = text.split(/\r?\n/);
    const rawList = [];
    const lineRegex = /(?:^|\s)(?:\[|\()?((?:\d{1,3}:)?\d{1,2}:\d{2})(?:\]|\))?\s*[-–—:]?\s*(.+)$/;

    for (let rawLine of lines) {
      const line = rawLine.trim();
      if (!line) continue;
      const m = line.match(lineRegex);
      if (m) {
        const timeStr = m[1].trim();
        const titleStr = m[2].trim().replace(/^[-–—:]\s*/, '').trim();
        const sec = CT.parseTime(timeStr);
        if (sec !== null && isFinite(sec) && sec >= 0 && titleStr) {
          rawList.push({ startSec: sec, title: titleStr });
        }
      }
    }
    return buildChapterRanges(rawList);
  }

  function extractYouTubeChapters() {
    // 1. YouTube HTML5 player API
    try {
      const player = document.getElementById('movie_player');
      if (player && typeof player.getChapters === 'function') {
        const apiChapters = player.getChapters();
        if (Array.isArray(apiChapters) && apiChapters.length > 0) {
          const list = [];
          for (let i = 0; i < apiChapters.length; i++) {
            const ch = apiChapters[i];
            const startSec = Number(ch.time || ch.start_time || 0);
            const title = String(ch.title || '').trim() || ('Bölüm ' + (i + 1));
            list.push({ startSec, title });
          }
          if (list.length > 0) return buildChapterRanges(list);
        }
      }
    } catch (_) {}

    // 2. DOM Macro Markers (Engagement Panel Chapters)
    try {
      const markerItems = document.querySelectorAll(
        'ytd-macro-markers-list-item-renderer, ytd-chapter-renderer, macro-markers-list-item-renderer'
      );
      if (markerItems && markerItems.length > 0) {
        const list = [];
        markerItems.forEach(item => {
          const titleEl = item.querySelector('#details h4, h4, #endpoint #title, .macro-markers-title');
          const timeEl = item.querySelector('#time, span.macro-markers-time, #endpoint span');
          if (titleEl && timeEl) {
            const tText = timeEl.textContent.trim();
            const sec = CT.parseTime(tText);
            const title = titleEl.textContent.trim();
            if (sec !== null && title) list.push({ startSec: sec, title });
          }
        });
        if (list.length > 0) return buildChapterRanges(list);
      }
    } catch (_) {}

    // 3. Description Timestamp Links
    try {
      const descLinks = document.querySelectorAll(
        '#description a[href*="t="], #description-inline-expander a[href*="t="], ytd-expandable-video-description-body-renderer a[href*="t="]'
      );
      if (descLinks && descLinks.length > 0) {
        const list = [];
        descLinks.forEach(a => {
          const text = a.textContent.trim();
          const sec = CT.parseTime(text);
          if (sec !== null) {
            let title = '';
            const parent = a.parentElement;
            if (parent) {
              const fullText = parent.textContent;
              const idx = fullText.indexOf(text);
              if (idx >= 0) {
                title = fullText.substring(idx + text.length).trim().replace(/^[-–—:]\s*/, '').trim();
                const newline = title.indexOf('\n');
                if (newline >= 0) title = title.substring(0, newline).trim();
              }
            }
            if (!title) title = a.nextSibling && a.nextSibling.textContent ? a.nextSibling.textContent.trim().replace(/^[-–—:]\s*/, '').trim() : '';
            if (!title) title = 'Bölüm ' + CT.formatClock(sec);
            list.push({ startSec: sec, title });
          }
        });
        if (list.length > 0) return buildChapterRanges(list);
      }
    } catch (_) {}

    // 4. Description raw text with regex
    try {
      const descContainer = document.querySelector(
        '#description-inline-expander, #description, ytd-expandable-video-description-body-renderer'
      );
      if (descContainer && descContainer.textContent) {
        const fromText = parseManualChapterText(descContainer.textContent);
        if (fromText.length > 0) return fromText;
      }
    } catch (_) {}

    return [];
  }

  function loadSettings() {
    chrome.storage.local.get(SETTINGS_KEY, stored => {
      const saved = (stored && stored[SETTINGS_KEY]) || {};
      const merged = Object.assign({}, DEFAULTS, saved);
      merged.jobs = normalizeJobs(saved.jobs !== undefined ? saved.jobs : saved.jobsCount);
      state.settings = merged;
      if (state.panel) state.panel.sync();
    });
  }

  function saveSettings(patch) {
    state.settings = Object.assign({}, state.settings, patch);
    chrome.storage.local.set({ [SETTINGS_KEY]: state.settings });
  }

  chrome.storage.onChanged.addListener((changes, area) => {
    if (area !== 'local' || !changes[SETTINGS_KEY]) return;
    const saved = changes[SETTINGS_KEY].newValue || {};
    const merged = Object.assign({}, DEFAULTS, saved);
    merged.jobs = normalizeJobs(saved.jobs !== undefined ? saved.jobs : saved.jobsCount);
    state.settings = merged;
    if (state.panel) state.panel.sync();
  });

  /* --------------------------------------------------------------- buttons */

  function createButton(label, icon, extraClass) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'getube-btn' + (extraClass ? ' ' + extraClass : '');
    button.dataset.baseLabel = label;

    const progress = document.createElement('span');
    progress.className = 'getube-progress';

    const content = document.createElement('span');
    content.className = 'getube-content';

    const iconWrap = document.createElement('span');
    iconWrap.innerHTML = icon;

    const text = document.createElement('span');
    text.className = 'getube-label';
    text.textContent = label;

    const pct = document.createElement('span');
    pct.className = 'getube-percent';
    pct.style.display = 'none';

    content.append(iconWrap, text, pct);
    button.append(progress, content);
    return button;
  }

  /* ----------------------------------------------------------------- panel */
  /* ----------------------------------------------------------------- panel */

  function buildPanel() {
    if (state.panel) return state.panel;

    const host = document.createElement('div');
    host.id = PANEL_ID;
    host.style.cssText = 'position:fixed;inset:0;z-index:2147483000;display:none';
    const shadow = host.attachShadow({ mode: 'open' });
    shadow.innerHTML = `
      <style>
        :host{all:initial}
        .backdrop{position:fixed;inset:0;background:rgba(0,0,0,.55);display:flex;align-items:center;justify-content:center;font-family:Roboto,Arial,sans-serif}
        .sheet{width:min(420px,92vw);background:#1f1f1f;color:#fff;border:1px solid #3a3a3a;border-radius:16px;box-shadow:0 18px 60px rgba(0,0,0,.55);overflow:hidden}
        .head{display:flex;align-items:center;justify-content:space-between;padding:14px 16px 6px}
        .head h2{font-size:15px;margin:0;font-weight:600;display:flex;align-items:center;gap:8px}
        .sub{font-size:11px;color:#9a9a9a;padding:0 16px 10px}
        .body{padding:0 16px 4px}
        .row{display:flex;gap:8px;align-items:flex-end;margin-bottom:10px}
        .field{flex:1;display:flex;flex-direction:column;gap:4px;min-width:0}
        .field label{font-size:11px;color:#aaa}
        input[type=text]{background:#121212;border:1px solid #3a3a3a;border-radius:8px;color:#fff;font-size:13px;padding:8px 10px;font-family:inherit;width:100%;box-sizing:border-box}
        input[type=text]:focus{outline:none;border-color:#cc0000}
        input.invalid{border-color:#e04b4b}
        select{background:#121212;border:1px solid #3a3a3a;border-radius:8px;color:#fff;font-size:12px;padding:8px 10px;font-family:inherit;flex:1}
        .chip,.mini{background:#2b2b2b;border:1px solid #3a3a3a;border-radius:999px;color:#e6e6e6;font-size:11px;padding:6px 10px;cursor:pointer;font-family:inherit;white-space:nowrap}
        .chip:hover,.mini:hover{background:#3a3a3a}
        .chips{display:flex;gap:6px;flex-wrap:wrap;margin-bottom:10px}
        .summary{font-size:12px;color:#c9c9c9;background:#171717;border:1px solid #2e2e2e;border-radius:8px;padding:8px 10px;margin-bottom:10px;display:flex;justify-content:space-between;gap:8px}
        .summary b{color:#fff;font-weight:600}
        .err{color:#ff8080;font-size:11px;min-height:14px;margin:-2px 0 8px}
        .actions{display:flex;gap:8px;margin-bottom:12px}
        .primary{flex:1;background:#cc0000;border:0;border-radius:10px;color:#fff;font-size:13px;font-weight:600;padding:11px 10px;cursor:pointer;font-family:inherit}
        .primary:hover{background:#e00000}
        .primary:disabled{background:#4a2020;color:#a88;cursor:default}
        .secondary{flex:1;background:#2b2b2b;border:1px solid #3a3a3a;border-radius:10px;color:#e6e6e6;font-size:12px;padding:9px 8px;cursor:pointer;font-family:inherit}
        .secondary:hover{background:#3a3a3a}
        .secondary:disabled{opacity:.5;cursor:default}
        .foot{display:flex;align-items:center;justify-content:space-between;border-top:1px solid #2e2e2e;padding:10px 16px;gap:8px;flex-wrap:wrap}
        .link{background:none;border:0;color:#9fd0ff;font-size:12px;cursor:pointer;font-family:inherit;padding:0}
        .link:hover{text-decoration:underline}
        .toggles{display:flex;gap:6px;flex-wrap:wrap}
        .toggle{font-size:11px;border:1px solid #3a3a3a;background:#232323;border-radius:999px;padding:5px 9px;cursor:pointer;color:#bbb;font-family:inherit}
        .toggle[aria-pressed=true]{background:rgba(204,0,0,.22);border-color:#cc0000;color:#fff}
        .job{margin-top:10px;font-size:11.5px;color:#bbb;display:none;flex-direction:column;gap:6px}
        .job.visible{display:flex}
        .bar{height:4px;background:#2e2e2e;border-radius:999px;overflow:hidden}
        .bar span{display:block;height:100%;width:0;background:#cc0000;transition:width .25s}
        .job.done .bar span{background:#2ecc71}
        .job.error .bar span{background:#e04b4b}
        .jobtext{display:flex;justify-content:space-between;gap:8px}
        .jobtext em{font-style:normal;color:#888;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
        .chapter-box{background:#161616;border:1px solid #2e2e2e;border-radius:10px;padding:10px;margin-bottom:10px}
        .chapter-header{display:flex;justify-content:space-between;align-items:center;margin-bottom:6px}
        .chapter-header label{font-size:11px;color:#aaa}
        textarea{background:#121212;border:1px solid #3a3a3a;border-radius:8px;color:#fff;font-size:12px;padding:6px 8px;font-family:inherit;width:100%;box-sizing:border-box;resize:vertical}
        textarea:focus{outline:none;border-color:#cc0000}
      </style>
      <div class="backdrop" part="backdrop">
        <div class="sheet" role="dialog" aria-modal="true" aria-label="RuviTube kesit">
          <div class="head">
            <h2>✂ <span id="title"></span></h2>
            <button class="link" id="close"></button>
          </div>
          <div class="sub" id="sub"></div>
          <div class="body">
            <div class="row">
              <div class="field">
                <label id="lblStart"></label>
                <input type="text" id="start" inputmode="numeric" autocomplete="off">
              </div>
              <button class="mini" id="now"></button>
              <div class="field">
                <label id="lblEnd"></label>
                <input type="text" id="end" inputmode="numeric" autocomplete="off">
              </div>
              <button class="mini" id="endNow"></button>
            </div>
            <div class="chapter-box" id="chapterBox">
              <div class="chapter-header">
                <label id="lblChapters"></label>
                <button class="link" id="btnToggleManual" type="button" style="font-size:11px"></button>
              </div>
              <div class="row" style="margin-bottom:6px">
                <select id="chapterSelect" style="width:100%"></select>
              </div>
              <div id="manualChapterWrap" style="display:none;margin-bottom:8px">
                <textarea id="manualChapterText" rows="3"></textarea>
                <div style="display:flex;justify-content:flex-end;margin-top:4px">
                  <button class="mini" id="btnParseChapters" type="button"></button>
                </div>
              </div>
              <div class="row" id="chapterDlRow" style="display:none;margin-bottom:0">
                <button class="secondary" id="btnDlChapterVideo" type="button" style="font-size:11px;padding:7px 8px"></button>
                <button class="secondary" id="btnDlChapterAudio" type="button" style="font-size:11px;padding:7px 8px"></button>
              </div>
            </div>
            <div class="chips" id="chips"></div>
            <div class="row">
              <div class="field">
                <label id="lblLen">Süre (dk:sn)</label>
                <input type="text" id="len" inputmode="numeric" autocomplete="off" placeholder="5:00">
              </div>
              <button class="mini" id="applyLen">Uygula</button>
            </div>
            <div class="summary"><span id="range"></span><b id="span"></b></div>
            <div class="err" id="err"></div>
            <div class="row">
              <select id="vq"></select>
              <button class="primary" id="video"></button>
            </div>
            <div class="row">
              <select id="aq"></select>
              <button class="primary" id="audio"></button>
            </div>
            <div class="row" style="align-items:center">
              <label id="lblJobs" for="jobs" style="font-size:11px;color:#aaa;min-width:108px"></label>
              <select id="jobs"></select>
            </div>
            <div class="row">
              <button class="secondary" id="fullV"></button>
              <button class="secondary" id="fullA"></button>
            </div>            <div class="job" id="job">
              <div class="jobtext"><span id="jobStatus"></span><em id="jobFile"></em></div>
              <div class="bar"><span id="jobBar"></span></div>
            </div>
          </div>
          <div class="foot">
            <button class="link" id="full"></button>
            <div class="toggles">
              <button class="toggle" data-setting="openFolder"></button>
              <button class="toggle" data-setting="openFile"></button>
              <button class="toggle" data-setting="openDownloads"></button>
            </div>
          </div>
        </div>
      </div>
    `;

    const $ = id => shadow.getElementById(id);
    $('title').textContent = t('clipTitle', 'Kesit indir');
    $('sub').textContent = t('clipSubtitle', 'Video ve ses için başlangıç–bitiş zamanı seçin.');
    $('lblStart').textContent = t('clipStart', 'Başlangıç');
    $('lblEnd').textContent = t('clipEnd', 'Bitiş');
    $('now').textContent = t('clipNow', 'Şimdi');
    $('endNow').textContent = t('clipEndNow', 'Şimdi');
    $('lblChapters').textContent = t('clipChapters', 'Bölümler');
    $('btnToggleManual').textContent = t('clipManualParse', 'Manuel Ayrıştır') + ' ▼';
    $('btnParseChapters').textContent = t('clipParseBtn', 'Ayrıştır');
    $('manualChapterText').placeholder = t('clipManualHint', 'Zaman damgalarını buraya yapıştırın:\n00:00 Giriş\n03:15 Konu');
    $('btnDlChapterVideo').textContent = t('clipDlChapterVideo', 'Bölümü İndir (MP4)');
    $('btnDlChapterAudio').textContent = t('clipDlChapterAudio', 'Bölümü İndir (MP3)');
    $('lblLen').textContent = 'Süre (dk:sn)';
    $('applyLen').textContent = 'Uygula';
    $('fullV').textContent = 'Tümünü indir (MP4)';
    $('fullA').textContent = 'Tümünü indir (MP3)';
    $('close').textContent = t('clipClose', 'Kapat');
    $('video').textContent = t('clipDownloadVideo', 'MP4 indir');
    $('audio').textContent = t('clipDownloadAudio', 'MP3 indir');
    $('lblJobs').textContent = t('clipJobs', 'Paralel (--jobs)');
    $('full').textContent = t('clipFull', 'Tam videoyu indir');
    $('start').placeholder = t('clipPlaceholder', '1:23 veya 01:23:45');
    $('end').placeholder = t('clipPlaceholder', '1:23 veya 01:23:45');

    let currentChapters = [];
    let selectedChapterTitle = null;

    function populateChapterDropdown(chapters) {
      currentChapters = chapters || [];
      const sel = $('chapterSelect');
      sel.innerHTML = '';
      if (!currentChapters.length) {
        const opt = document.createElement('option');
        opt.value = '';
        opt.textContent = t('clipNoChapters', '-- Bölüm bulunamadı (Manuel girin) --');
        sel.appendChild(opt);
        $('chapterDlRow').style.display = 'none';
        return;
      }
      const defaultOpt = document.createElement('option');
      defaultOpt.value = '';
      defaultOpt.textContent = t('clipSelectChapter', '-- Bölüm seçin --');
      sel.appendChild(defaultOpt);
      currentChapters.forEach((ch, idx) => {
        const opt = document.createElement('option');
        opt.value = String(idx);
        opt.textContent = ch.label;
        sel.appendChild(opt);
      });
      $('chapterDlRow').style.display = 'none';
    }

    function refreshChapters() {
      const detected = extractYouTubeChapters();
      populateChapterDropdown(detected);
    }

    for (const [value, label] of Object.entries(QUALITY_LABELS)) {
      const option = document.createElement('option');
      option.value = value;
      option.textContent = label;
      $('vq').appendChild(option);
    }
    for (const [value, label] of Object.entries(AUDIO_LABELS)) {
      const option = document.createElement('option');
      option.value = value;
      option.textContent = label;
      $('aq').appendChild(option);
    }
    for (const value of VALID_JOBS) {
      const option = document.createElement('option');
      option.value = String(value);
      option.textContent = JOBS_LABELS[String(value)] || String(value);
      $('jobs').appendChild(option);
    }

    CHIPS.forEach(seconds => {
      const chip = document.createElement('button');
      chip.className = 'chip';
      chip.type = 'button';
      chip.dataset.seconds = String(seconds);
      chip.textContent = '+' + (seconds >= 60 ? (seconds / 60) + 'dk' : seconds + 'sn');
      $('chips').appendChild(chip);
    });

    shadow.querySelectorAll('.toggle').forEach(toggle => {
      toggle.addEventListener('click', () => {
        const key = toggle.dataset.setting;
        saveSettings({ [key]: !state.settings[key] });
        sync();
      });
    });

    function readClip() {
      return CT.validateRange($('start').value, $('end').value, state.settings.clipLength);
    }

    function sync() {
      $('vq').value = state.settings.videoQuality || '720p';
      $('aq').value = state.settings.audioQuality || '320kbps';
      const j = normalizeJobs(state.settings.jobs);
      $('jobs').value = String(VALID_JOBS.includes(j) ? j : DEFAULTS.jobs);
      shadow.querySelectorAll('.toggle').forEach(toggle => {
        const on = !!state.settings[toggle.dataset.setting];
        toggle.setAttribute('aria-pressed', on ? 'true' : 'false');
        toggle.textContent = t(
          toggle.dataset.setting === 'openFolder' ? 'optOpenFolder'
            : toggle.dataset.setting === 'openFile' ? 'optOpenFile' : 'optOpenDownloads',
          toggle.dataset.setting
        );
      });
      update();
    }

    function update() {
      const result = readClip();
      const startInvalid = CT.parseTime($('start').value) === null && $('start').value.trim() !== '';
      const endInvalid = $('end').value.trim() !== '' && CT.parseTime($('end').value) === null;
      $('start').classList.toggle('invalid', startInvalid);
      $('end').classList.toggle('invalid', endInvalid);

      if (!result.ok) {
        $('err').textContent = t(result.code, 'Geçerli bir zaman aralığı girin.');
        $('range').textContent = '—';
        $('span').textContent = '';
        $('video').disabled = true;
        $('audio').disabled = true;
        return;
      }

      $('err').textContent = '';
      $('range').textContent = CT.formatClock(result.startSec) + ' – ' + CT.formatClock(result.endSec);
      const duration = videoDuration();
      const overflow = duration > 0 && result.endSec > duration;
      $('span').textContent = overflow
        ? CT.formatClock(duration) + ' (kırpılacak)'
        : CT.formatDuration(result.endSec - result.startSec);
      const busy = !!(state.panel && state.panel.busy);
      $('video').disabled = busy;
      $('audio').disabled = busy;
    }

    $('now').addEventListener('click', () => {
      $('start').value = CT.formatClock(playbackTime());
      if ($('end').value.trim()) {
        const end = CT.parseTime($('end').value);
        const start = CT.parseTime($('start').value) || 0;
        if (end === null || end <= start) $('end').value = '';
      }
      selectedChapterTitle = null;
      $('chapterSelect').value = '';
      $('chapterDlRow').style.display = 'none';
      update();
    });

    $('endNow').addEventListener('click', () => {
      $('end').value = CT.formatClock(playbackTime());
      selectedChapterTitle = null;
      $('chapterSelect').value = '';
      $('chapterDlRow').style.display = 'none';
      update();
    });

    $('btnToggleManual').addEventListener('click', () => {
      const wrap = $('manualChapterWrap');
      const isHidden = wrap.style.display === 'none';
      wrap.style.display = isHidden ? 'block' : 'none';
      $('btnToggleManual').textContent = isHidden
        ? t('clipManualParse', 'Manuel Ayrıştır') + ' ▲'
        : t('clipManualParse', 'Manuel Ayrıştır') + ' ▼';
    });

    $('btnParseChapters').addEventListener('click', () => {
      const text = $('manualChapterText').value;
      const parsed = parseManualChapterText(text);
      if (parsed.length > 0) {
        populateChapterDropdown(parsed);
        $('err').textContent = '';
      } else {
        $('err').textContent = 'Zaman damgası bulunamadı. Örnek: 00:00 Giriş';
      }
    });

    $('chapterSelect').addEventListener('change', () => {
      const val = $('chapterSelect').value;
      if (val === '' || !currentChapters[Number(val)]) {
        selectedChapterTitle = null;
        $('chapterDlRow').style.display = 'none';
        return;
      }
      const ch = currentChapters[Number(val)];
      selectedChapterTitle = ch.title;
      $('start').value = CT.formatClock(ch.startSec);
      $('end').value = CT.formatClock(ch.endSec);
      $('chapterDlRow').style.display = 'flex';
      update();
    });

    $('btnDlChapterVideo').addEventListener('click', () => {
      submit('Video', $('vq').value);
    });

    $('btnDlChapterAudio').addEventListener('click', () => {
      submit('MP3', $('aq').value);
    });

    $('chips').addEventListener('click', event => {
      const chip = event.target.closest('.chip');
      if (!chip) return;
      const start = CT.parseTime($('start').value);
      if (start === null) {
        $('start').value = CT.formatClock(playbackTime());
      }
      const base = CT.parseTime($('start').value) || 0;
      $('end').value = CT.formatClock(base + Number(chip.dataset.seconds));
      selectedChapterTitle = null;
      $('chapterSelect').value = '';
      $('chapterDlRow').style.display = 'none';
      update();
    });

    $('applyLen').addEventListener('click', () => {
      const start = CT.parseTime($('start').value);
      const span = CT.parseTime($('len').value);
      if (start === null || span === null || span < 1) {
        $('err').textContent = 'Süre geçersiz. Örnek: 5:00 veya 90';
        return;
      }
      $('end').value = CT.formatClock(start + span);
      selectedChapterTitle = null;
      $('chapterSelect').value = '';
      $('chapterDlRow').style.display = 'none';
      update();
    });

    ['start', 'end'].forEach(id => $(id).addEventListener('input', () => {
      if (selectedChapterTitle) {
        const s = CT.parseTime($('start').value);
        const e = CT.parseTime($('end').value);
        const curIdx = Number($('chapterSelect').value);
        const ch = currentChapters[curIdx];
        if (!ch || s !== ch.startSec || e !== ch.endSec) {
          selectedChapterTitle = null;
          $('chapterSelect').value = '';
          $('chapterDlRow').style.display = 'none';
        }
      }
      update();
    }));
    $('vq').addEventListener('change', () => saveSettings({ videoQuality: $('vq').value }));
    $('aq').addEventListener('change', () => saveSettings({ audioQuality: $('aq').value }));
    $('jobs').addEventListener('change', () => saveSettings({ jobs: normalizeJobs($('jobs').value) }));

    $('video').addEventListener('click', () => submit('Video', $('vq').value));
    $('audio').addEventListener('click', () => submit('MP3', $('aq').value));

    function submit(type, quality) {
      const result = readClip();
      if (!result.ok) return;

      // Video süresi biliniyorsa bitişi süreye kırp: yt-dlp bunu sessizce yapıyor,
      // kullanıcı dosya süresinin neden kısa olduğunu anlamıyor.
      let startSec = result.startSec;
      let endSec = result.endSec;
      const duration = videoDuration();
      if (duration > 0 && endSec > duration) endSec = Math.floor(duration * 10) / 10;
      if (endSec - startSec < 1) {
        $('err').textContent = t('errShort', 'Aralık en az 1 saniye olmalı.');
        return;
      }

      const clipPayload = { startSec, endSec };
      if (selectedChapterTitle) clipPayload.chapterTitle = selectedChapterTitle;
      start(type, quality, clipPayload);
    }

    // Kesit panelinden tam indirme: aralık yok sayılır, seçili kalite kullanılır.
    $('fullV').addEventListener('click', () => start('Video', $('vq').value, null));
    $('fullA').addEventListener('click', () => start('MP3', $('aq').value, null));

    $('full').addEventListener('click', () => {
      // Tam indirme: kesit olmadan, varsayilan kaliteyle.
      hide();
      start('Video', state.settings.videoQuality || '720p', null);
    });

    $('close').addEventListener('click', hide);
    shadow.querySelector('.backdrop').addEventListener('click', event => {
      if (event.target === event.currentTarget) hide();
    });
    shadow.addEventListener('keydown', event => {
      if (event.key === 'Escape') { event.preventDefault(); hide(); }
    });

    function show() {
      host.style.display = 'block';
      state.panel.busy = false;
      const startValue = CT.formatClock(playbackTime());
      $('start').value = startValue;
      $('end').value = CT.formatClock(playbackTime() + (state.settings.clipLength || 30));
      selectedChapterTitle = null;
      $('manualChapterWrap').style.display = 'none';
      refreshChapters();
      sync();
      $('start').focus();
      $('start').select();
    }

    function hide() {
      host.style.display = 'none';
    }

    function showJob(job) {
      const wrap = $('job');
      wrap.classList.add('visible');
      wrap.classList.toggle('done', job.state === 'done');
      wrap.classList.toggle('error', job.state === 'failed');
      $('jobStatus').textContent = job.status || (job.state === 'done' ? t('statusDone', 'Tamamlandı') : '');
      $('jobFile').textContent = job.fileName || job.error || '';
      $('jobBar').style.width = Math.max(0, Math.min(100, Number(job.percent) || 0)) + '%';
      const busy = job.state === 'running' || job.state === 'downloading' || job.state === 'queued';
      state.panel.busy = busy;
      $('fullV').disabled = busy;
      $('fullA').disabled = busy;
      $('video').disabled = busy;
      $('audio').disabled = busy;
    }

    document.documentElement.appendChild(host);
    state.panel = {
      host, shadow, show, hide, sync, update, showJob,
      busy: false,
      isOpen: () => host.style.display === 'block'
    };
    return state.panel;
  }

  /* -------------------------------------------------------------- download */

  function start(type, quality, clip) {
    const jobs = normalizeJobs(state.settings.jobs);
    const payload = { action: 'download', type, quality, url: location.href, jobs };
    if (clip) {
      payload.clip = { startSec: clip.startSec, endSec: clip.endSec };
      if (clip.chapterTitle) payload.clip.chapterTitle = clip.chapterTitle;
    }

    chrome.runtime.sendMessage(payload, response => {
      const err = chrome.runtime.lastError;
      if (err) {
        showStatus('Hata: ' + err.message);
        return;
      }
      if (!response || !response.ok) {
        showStatus('Hata: ' + ((response && response.error) || 'İndirme başlatılamadı.'));
        return;
      }

      const job = {
        jobId: response.jobId,
        displayLabel: response.label,
        percent: 0,
        status: t('statusPreparing', 'Hazırlanıyor...'),
        clip: !!clip
      };
      state.jobs.set(response.jobId, job);

      showStatus('✂ ' + job.status);
      if (state.panel) state.panel.showJob({ jobId: job.jobId, state: 'running', status: job.status, percent: 0, clip: job.clip });
    });
  }
  function showStatus(text) {
    const status = document.querySelector('.getube-status');
    if (!status) return;
    status.style.display = 'block';
    status.textContent = text;
    status.title = text;
    setTimeout(() => {
      if (status.textContent === text) { status.style.display = 'none'; status.textContent = ''; }
    }, 8000);
  }

  /* ------------------------------------------------------------------- UI */

  function createUI() {
    addStyle();
    const target = getTarget();
    if (!target || !document.documentElement.contains(target)) return;

    const existing = document.getElementById(UI_ID);
    if (existing) {
      const stillPlaced = isSubscribeAnchor(target)
        ? existing.previousElementSibling === target
        : existing.parentElement === target;
      if (stillPlaced) {
        return;
      }
      existing.remove();
    }

    const container = document.createElement('div');
    container.id = UI_ID;
    container.className = 'getube-container';

    const clipWrap = document.createElement('div');
    clipWrap.style.position = 'relative';

    const clipIcon = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="6" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><line x1="20" y1="4" x2="8.12" y2="15.88"/><line x1="14.47" y1="14.48" x2="20" y2="20"/><line x1="8.12" y1="8.12" x2="12" y2="12"/></svg>';

    const clipButton = createButton('', clipIcon, 'getube-clip-btn');
    clipButton.title = t('btnClipTitle', 'Zaman aralığı ile indir');
    clipButton.setAttribute('aria-label', t('btnClipTitle', 'Zaman aralığı ile indir'));
    clipButton.querySelector('.getube-label').textContent = '';

    clipButton.addEventListener('click', event => {
      event.stopPropagation();
      buildPanel().show();
    });

    const status = document.createElement('span');
    status.className = 'getube-status';

    clipWrap.append(clipButton);
    container.append(clipWrap, status);
    placeContainer(container, target);

    container.addEventListener('click', event => event.stopPropagation());
  }

  /* ------------------------------------------------------------- messaging */

  if (!state.listener) {
    state.listener = true;

    chrome.runtime.onMessage.addListener(message => {
      if (!message || message.action !== 'download_progress' || !message.jobId) return;
      const job = state.jobs.get(message.jobId) || { jobId: message.jobId };
      Object.assign(job, message);
      state.jobs.set(message.jobId, job);

      if (state.panel) state.panel.showJob(Object.assign({}, job));

      if (job.state === 'done') {
        showStatus('✓ ' + (job.fileName || t('statusDone', 'Tamamlandı')));
      } else if (job.state === 'failed') {
        showStatus('✕ ' + (job.error || t('statusError', 'Hata')));
      } else {
        const pct = Math.max(0, Math.min(100, Math.round(Number(job.percent) || 0)));
        showStatus('✂ ' + pct + '% · ' + (job.status || t('statusDownloading', 'İndiriliyor...')));
      }
    });
  }
  /* --------------------------------------------------------------- bootstrap */

  loadSettings();

  let timer = 0;
  function schedule() {
    clearTimeout(timer);
    timer = setTimeout(createUI, 180);
  }

  function boot() {
    addStyle();
    createUI();
  }

  const TARGET_HINTS = 'ytd-watch-metadata, #subscribe-button, ytd-subscribe-button-renderer, #owner, #top-row, #actions';
  function hasRelevantAddedNode(records) {
    if (document.getElementById(UI_ID)) return false;
    for (const record of records) {
      for (const node of record.addedNodes) {
        if (node.nodeType !== 1) continue;
        if (node.matches(TARGET_HINTS) || (node.querySelector && node.querySelector(TARGET_HINTS))) return true;
      }
    }
    return false;
  }

  const observer = new MutationObserver(records => {
    if (hasRelevantAddedNode(records)) schedule();
  });

  function observe() {
    if (document.documentElement) observer.observe(document.documentElement, { childList: true, subtree: true });
  }

  ['yt-navigate-finish', 'yt-page-data-updated', 'yt-player-updated'].forEach(name => {
    window.addEventListener(name, () => {
      if (state.panel && state.panel.isOpen()) state.panel.hide();
      schedule();
    }, { passive: true });
  });
  window.addEventListener('popstate', schedule, { passive: true });

  // YouTube SPA ilk yüklemede başlık alanı geç oluşabiliyor: kısa süre ısrarla dene.
  let tries = 0;
  const retry = setInterval(() => {
    tries++;
    if (document.getElementById(UI_ID) || tries > 60) { clearInterval(retry); return; }
    createUI();
  }, 500);

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => { observe(); boot(); }, { once: true });
  } else {
    observe();
    boot();
  }
})();
