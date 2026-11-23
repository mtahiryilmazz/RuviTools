/*
 * RuviTube — YouTube sayfa arayüzü (content script).
 *
 * Özellikler:
 *  - Oynatıcı üzerinde RuviTools buzlu-cam kalite & otomatik MP3 menüsü (Shadow DOM, Trusted-Types güvenli).
 *  - player-probe.js (MAIN world) ile anlık çözünürlük/kodek/boyut okuma + NativeHost probe fallback.
 *  - Abone ol yanında kompakt Kalite/MP3 + Kesit (✂) düğmeleri ve canlı durum göstergesi.
 *  - Kesit aralığı RuviTubeClipTime ile doğrulanır; native host ham metin yerine saniye alır.
 */
(() => {
  'use strict';

  const CT = window.RuviTubeClipTime || window.GeTubeClipTime;
  const UI_ID = 'ruvitube-ui';
  const STYLE_ID = 'ruvitube-style';
  const PANEL_ID = 'ruvitube-clip-host';
  const PILL_HOST_ID = 'ruvitube-player-pill';
  const SETTINGS_KEY = 'ruvitube.settings';

  const DEFAULTS = {
    openDownloads: false,
    openFile: false,
    openFolder: false,
    videoQuality: '1080p',
    audioQuality: '320kbps',
    clipLength: 30,
    jobs: 4,
    showStatus: true,
    showFloatingPill: false,
    favorites: []
  };

  const VALID_JOBS = [1, 2, 4, 8, 16];
  function normalizeJobs(raw) {
    const n = Math.round(Number(raw));
    if (!Number.isFinite(n)) return DEFAULTS.jobs;
    if (VALID_JOBS.includes(n)) return n;
    return Math.min(16, Math.max(1, n));
  }

  const QUALITY_LABELS = {
    'best': 'En iyi (MP4/WEBM)',
    '2160p': '4K · 2160p',
    '1440p': '2K · 1440p',
    '1080p': '1080p Full HD',
    '720p': '720p HD',
    '480p': '480p'
  };
  const AUDIO_LABELS = {
    '320kbps': 'MP3 · 320 kbps',
    '256kbps': 'MP3 · 256 kbps',
    '192kbps': 'MP3 · 192 kbps',
    '128kbps': 'MP3 · 128 kbps',
    'vbr': 'MP3 · VBR',
    'm4a': 'M4A · Orijinal AAC',
    'wav': 'WAV · Kayıpsız PCM'
  };
  const JOBS_LABELS = {
    '1': 'x1 · Normal',
    '2': 'x2 · Hızlı',
    '4': 'x4 · Paralel',
    '8': 'x8 · Turbo',
    '16': 'x16 · Ultra'
  };
  const CHIPS = [15, 30, 60, 180, 300, 600];

  const state = window.__RUVITUBE_STATE__ || (window.__RUVITUBE_STATE__ = {
    jobs: new Map(),
    listener: false,
    settings: Object.assign({}, DEFAULTS),
    panel: null,
    pill: null,
    probe: { url: '', data: null, loading: false, chapters: [] }
  });

  const TR = {
    btnClipTitle: 'Zaman aralığı & bölüm kesiti indir',
    clipTitle: 'Kesit indir',
    clipSubtitle: 'Video ve ses için başlangıç–bitiş zamanı seçin.',
    clipStart: 'Başlangıç',
    clipEnd: 'Bitiş',
    clipNow: 'Şimdi',
    clipEndNow: 'Şimdi',
    clipClose: 'Kapat',
    clipDownloadVideo: 'Video indir',
    clipDownloadAudio: 'Ses / MP3 indir',
    clipJobs: 'İndirme Hızı',
    clipFull: 'Tam videoyu indir',
    clipPlaceholder: '1:23 veya 01:23:45',
    clipChapters: 'Bölümler',
    clipSelectChapter: '-- Bölüm seçin --',
    clipNoChapters: '-- Bölüm bulunamadı (Manuel girin) --',
    clipManualParse: 'Manuel Ayrıştır',
    clipManualHint: 'Zaman damgalarını buraya yapıştırın:\n00:00 Giriş\n03:15 Konu',
    clipParseBtn: 'Ayrıştır',
    clipDlChapterVideo: 'Bölümü İndir (Video)',
    clipDlChapterAudio: 'Bölümü İndir (Ses)',
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

  /* ------------------------------------------------------------- DOM helpers */

  const SVG_NS = 'http://www.w3.org/2000/svg';
  function el(tag, attrs = {}, ...children) {
    const node = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs)) {
      if (v !== undefined && v !== null) node.setAttribute(k, String(v));
    }
    for (const ch of children) {
      if (ch === null || ch === undefined) continue;
      node.appendChild(typeof ch === 'string' ? document.createTextNode(ch) : ch);
    }
    return node;
  }

  function svgEl(attrs, shapes) {
    const s = document.createElementNS(SVG_NS, 'svg');
    for (const [k, v] of Object.entries(attrs)) s.setAttribute(k, String(v));
    for (const [tag, sa] of shapes) {
      const sh = document.createElementNS(SVG_NS, tag);
      for (const [k, v] of Object.entries(sa)) sh.setAttribute(k, String(v));
      s.appendChild(sh);
    }
    return s;
  }

  /* ------------------------------------------------------------------ CSS */

  function addStyle() {
    if (document.getElementById(STYLE_ID)) return;
    const style = document.createElement('style');
    style.id = STYLE_ID;
    style.textContent = `
      .rt-container{display:inline-flex!important;flex-flow:row nowrap!important;align-items:center;justify-content:flex-start;gap:6px;margin-left:8px;position:relative;font-family:"Segoe UI",Roboto,Arial,sans-serif;z-index:2000;vertical-align:middle;white-space:nowrap!important;flex:0 0 auto!important;min-width:max-content}
      .rt-btn{position:relative;overflow:hidden;background:linear-gradient(135deg,#ff2a18 0%,#ff5700 58%,#ff7b00 100%);color:#ffffff;border:1px solid rgba(255,175,130,.48);border-radius:12px;padding:0;width:36px;height:36px;display:inline-flex;align-items:center;justify-content:center;cursor:pointer;box-sizing:border-box;box-shadow:0 4px 14px rgba(255,68,0,.32),inset 0 1px 0 rgba(255,255,255,.28);transition:transform .15s,box-shadow .18s,filter .18s;flex:0 0 auto!important}
      .rt-btn:hover{filter:brightness(1.08);box-shadow:0 6px 18px rgba(255,75,0,.48),inset 0 1px 0 rgba(255,255,255,.35);transform:translateY(-1px)}
      .rt-btn:active{transform:translateY(0)}
      .rt-btn-ico{display:inline-flex;align-items:center;justify-content:center;flex:none}
      .rt-status{font-size:11.5px;color:#ffb499;margin-left:4px;max-width:190px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;display:none}
    `;
    document.head.appendChild(style);
  }

  /* --------------------------------------------------------------- helpers */

  function isWatchLikePage() {
    return /^\/(watch|shorts|live)/.test(location.pathname) || location.hostname === 'youtu.be';
  }

  function currentVideoUrl() {
    try {
      const u = new URL(location.href);
      if (u.hostname === 'youtu.be') return `https://youtu.be${u.pathname}`;
      const v = u.searchParams.get('v');
      if (u.pathname.startsWith('/watch') && v) {
        return `https://${u.hostname}/watch?v=${encodeURIComponent(v)}`;
      }
      if (/^\/(shorts|live|embed)\/[\w-]+/.test(u.pathname)) {
        return `https://www.youtube.com${u.pathname}`;
      }
    } catch (_) {}
    return location.href;
  }

  function getTarget() {
    return document.querySelector('ytd-watch-metadata #subscribe-button') ||
      document.querySelector('ytd-watch-metadata ytd-subscribe-button-renderer') ||
      document.querySelector('#owner #subscribe-button') ||
      document.querySelector('#owner ytd-subscribe-button-renderer') ||
      document.querySelector('#top-row #actions') ||
      document.querySelector('ytd-watch-metadata #top-row #actions') ||
      document.querySelector('#above-the-fold #top-row #actions') ||
      document.querySelector('ytd-watch-metadata #actions');
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
    const t0 = video && Number.isFinite(video.currentTime) ? video.currentTime : 0;
    return Math.max(0, Math.round(t0 * 10) / 10);
  }

  function videoDuration() {
    const pd = Number(state.probe.data && state.probe.data.duration) || 0;
    if (pd > 0) return pd;
    const video = document.querySelector('video');
    return video && Number.isFinite(video.duration) && video.duration > 0 ? video.duration : 0;
  }

  function formatDuration(sec) {
    const total = Math.round(Number(sec) || 0);
    if (total <= 0) return '';
    const h = Math.floor(total / 3600);
    const m = Math.floor((total % 3600) / 60);
    const s = total % 60;
    const pad = n => String(n).padStart(2, '0');
    return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
  }

  function formatSize(bytes, approx = false) {
    const b = Number(bytes) || 0;
    if (b <= 0) return '';
    const pfx = approx ? '~' : '';
    if (b >= 1024 * 1024 * 1024) return `${pfx}${(b / (1024 * 1024 * 1024)).toFixed(2)} GB`;
    if (b >= 1024 * 1024) return `${pfx}${(b / (1024 * 1024)).toFixed(1)} MB`;
    return `${pfx}${Math.max(1, Math.round(b / 1024))} KB`;
  }

  function cleanTitle(raw) {
    return String(raw || '')
      .replace(/^\(\d+\)\s+/, '')
      .replace(/\s+[-–|]\s*YouTube$/i, '')
      .trim();
  }

  function currentTitle() {
    if (state.probe.data && state.probe.data.title) return cleanTitle(state.probe.data.title);
    const h1 = document.querySelector('h1.ytd-watch-metadata yt-formatted-string, #title h1 yt-formatted-string');
    return cleanTitle((h1 && h1.textContent) || document.title || 'YouTube');
  }

  /* ------------------------------------------------------------- player probe */

  let probeSeq = 0;
  const pendingProbeCallbacks = new Map();

  window.addEventListener('message', ev => {
    if (ev.source !== window || !ev.data || ev.data.channel !== 'ruvitube-probe-res-v1') return;
    const { reqId, data } = ev.data;
    if (data && Array.isArray(data.formats) && data.formats.length > 0) {
      state.probe.url = currentVideoUrl();
      state.probe.data = data;
      state.probe.loading = false;
      if (Array.isArray(data.chapters) && data.chapters.length > 0) {
        state.probe.chapters = data.chapters;
      }
      if (state.pill) state.pill.renderMenu();
    }
    const cb = pendingProbeCallbacks.get(reqId);
    if (cb) {
      pendingProbeCallbacks.delete(reqId);
      cb(data || null);
    }
  });

  function requestMainProbe() {
    return new Promise(resolve => {
      const reqId = 'rt-' + (++probeSeq);
      const timer = setTimeout(() => {
        pendingProbeCallbacks.delete(reqId);
        resolve(null);
      }, 600);
      pendingProbeCallbacks.set(reqId, data => {
        clearTimeout(timer);
        resolve(data);
      });
      try {
        window.postMessage({ channel: 'ruvitube-probe-req-v1', reqId }, location.origin === 'null' ? '*' : location.origin);
      } catch (_) {
        clearTimeout(timer);
        pendingProbeCallbacks.delete(reqId);
        resolve(null);
      }
    });
  }

  async function refreshVideoProbe(forceNative = false) {
    if (!isWatchLikePage()) return;
    const url = currentVideoUrl();
    if (!forceNative && state.probe.url === url && state.probe.data && state.probe.data.formats.length) {
      requestMainProbe().then(d => {
        if (d && typeof d.activeAudioLang === 'string') state.probe.data.activeAudioLang = d.activeAudioLang;
      });
      if (state.pill) state.pill.renderMenu();
      return;
    }

    state.probe.url = url;
    state.probe.loading = true;
    if (state.pill) state.pill.renderMenu();

    const mainData = await requestMainProbe();
    if (mainData && Array.isArray(mainData.formats) && mainData.formats.length > 0) {
      state.probe.data = mainData;
      state.probe.loading = false;
      if (state.pill) state.pill.renderMenu();
      return;
    }

    // Pure-Rust NativeHost probe fallback (when MAIN-world player response is unavailable or Firefox)
    chrome.runtime.sendMessage({ action: 'probe_video', url }, resp => {
      if (chrome.runtime.lastError || !resp || !resp.ok || !resp.data) {
        state.probe.loading = false;
        if (state.pill) state.pill.renderMenu();
        return;
      }
      const d = resp.data;
      const dur = videoDuration();
      const formats = Array.isArray(d.streams) ? d.streams.map(s => ({
        id: s.id || '',
        kind: s.kind || 'video',
        container: s.container || 'mp4',
        width: s.width || 0,
        height: s.height || 0,
        fps: Number(s.fps) || 0,
        hdr: false,
        vcodec: s.codecs || s.container || '',
        acodec: s.kind === 'audio' ? (s.codecs || s.container) : (s.kind === 'videoaudio' ? 'mp4a' : 'none'),
        tbr: s.bandwidth_bps ? Math.round(s.bandwidth_bps / 1000) : 0,
        total_bytes: s.total_bytes || (dur > 0 && s.bandwidth_bps ? Math.round((s.bandwidth_bps / 8) * dur) : 0)
      })) : [];
      state.probe.data = {
        id: '',
        title: d.title || currentTitle(),
        channel: '',
        duration: dur,
        isLive: false,
        thumbnail: '',
        formats,
        chapters: d.chapters || []
      };
      state.probe.loading = false;
      if (state.pill) state.pill.renderMenu();
    });
  }

  /* ------------------------------------------------------------- row builder */

  function codecShort(raw) {
    const v = String(raw || '').toLowerCase();
    if (!v || v === 'none') return '';
    if (v.startsWith('av01') || v === 'av1') return 'AV1';
    if (v.startsWith('vp09') || v === 'vp9') return 'VP9';
    if (v.startsWith('vp8')) return 'VP8';
    if (v.startsWith('avc') || v.includes('h264')) return 'H.264';
    if (v.startsWith('hev') || v.startsWith('hvc')) return 'HEVC';
    return '';
  }

  function resBadge(h) {
    if (h >= 4320) return { text: '8K', tier: 'uhd' };
    if (h >= 2880) return { text: '5K', tier: 'uhd' };
    if (h >= 2160) return { text: '4K', tier: 'uhd' };
    if (h >= 1440) return { text: '2K', tier: 'qhd' };
    if (h >= 1080) return { text: '1080p', tier: 'fhd' };
    if (h >= 720) return { text: '720p', tier: 'hd' };
    return { text: h > 0 ? `${h}p` : 'HD', tier: 'sd' };
  }

  function resName(h) {
    if (h >= 4320) return '8K (4320p)';
    if (h >= 2160) return '4K (2160p)';
    if (h >= 1440) return '2K (1440p)';
    return `${h}p`;
  }

  function buildQualityRows() {
    const data = state.probe.data || {};
    const formats = Array.isArray(data.formats) ? data.formats : [];
    const duration = Number(data.duration) || videoDuration() || 0;

    const videos = formats.filter(f => f && (f.kind === 'video' || f.kind === 'videoaudio') && Number(f.height) > 0);
    const audios = formats.filter(f => f && f.kind === 'audio');
    const m4aAudios = audios.filter(f => String(f.container || '').toLowerCase() === 'm4a');
    const bestAudio = audios.reduce((a, b) => (!a || (b.total_bytes || b.tbr || 0) > (a.total_bytes || a.tbr || 0)) ? b : a, null);
    const bestM4a = m4aAudios.reduce((a, b) => (!a || (b.total_bytes || b.tbr || 0) > (a.total_bytes || a.tbr || 0)) ? b : a, null);
    const audioBytes = (bestM4a && bestM4a.total_bytes) || (bestAudio && bestAudio.total_bytes) || 0;

    const videoRows = [];
    if (videos.length > 0) {
      const fpsClass = f => (Number(f.fps) >= 48 ? Math.round(Number(f.fps)) : 0);
      const dedup = new Map();
      for (const f of videos) {
        const cont = String(f.container || 'mp4').toUpperCase();
        const key = `${Number(f.height)}|${fpsClass(f)}|${cont}|${f.hdr ? 'hdr' : ''}`;
        const prev = dedup.get(key);
        if (!prev || (Number(f.tbr) || 0) > (Number(prev.tbr) || 0)) {
          dedup.set(key, f);
        }
      }
      const sorted = [...dedup.values()].sort((a, b) =>
        (Number(b.height) - Number(a.height))
        || ((Number(b.fps) || 0) - (Number(a.fps) || 0))
        || ((Number(b.tbr) || 0) - (Number(a.tbr) || 0))
      );

      const top = sorted[0];
      const topBadge = resBadge(Number(top.height));
      const topTotal = (top.total_bytes || 0) + (top.kind === 'video' ? audioBytes : 0);
      videoRows.push({
        type: 'Video',
        quality: 'best',
        badge: topBadge.text,
        tier: topBadge.tier,
        best: true,
        title: 'En iyi kalite (Otomatik DASH + Ses)',
        sub: [resName(Number(top.height)), String(top.container || 'mp4').toUpperCase(), codecShort(top.vcodec), fpsClass(top) ? `${fpsClass(top)}fps` : '', top.hdr ? 'HDR' : ''].filter(Boolean).join(' · '),
        sizeText: formatSize(topTotal, topTotal > 0 && top.kind === 'video')
      });

      const bestAvc = sorted.find(f => codecShort(f.vcodec) === 'H.264');
      if (bestAvc) {
        const avcH = Number(bestAvc.height) || 720;
        const avcTotal = (bestAvc.total_bytes || 0) + (bestAvc.kind === 'video' ? audioBytes : 0);
        videoRows.push({
          type: 'Video',
          quality: 'h264',
          badge: 'MP4',
          tier: 'fhd',
          title: `MP4 · H.264 + AAC (${resName(avcH)})`,
          sub: 'Windows Media Player ve tüm cihazlarda açılır',
          sizeText: formatSize(avcTotal, avcTotal > 0 && bestAvc.kind === 'video')
        });
      }

      for (const f of sorted) {
        const h = Number(f.height);
        const fps = fpsClass(f);
        const b = resBadge(h);
        const cont = String(f.container || 'mp4').toUpperCase();
        const codec = codecShort(f.vcodec);
        const total = (f.total_bytes || 0) + (f.kind === 'video' ? audioBytes : 0);
        const resSpec = `${h}p${fps >= 48 ? '60' : ''}`;
        const qSpec = f.id && /^itag-\d+$/.test(f.id)
          ? `${f.id}|${resSpec}:${cont.toLowerCase() === 'webm' ? 'webm' : 'mp4'}`
          : resSpec;
        videoRows.push({
          type: 'Video',
          quality: qSpec,
          badge: b.text,
          tier: b.tier,
          hdr: !!f.hdr,
          title: [resName(h), fps ? `${fps}fps` : '', cont].filter(Boolean).join(' · '),
          sub: [codec, f.tbr > 0 ? `${f.tbr} kbps` : '', f.kind === 'video' ? '+ Ses Remux' : 'Tek akış'].filter(Boolean).join(' · '),
          sizeText: formatSize(total, total > 0 && f.kind === 'video')
        });
      }
    } else {
      for (const [q, label, badge, tier] of [
        ['best', 'En iyi kalite (Otomatik)', 'MAX', 'uhd'],
        ['4320p', '8K (4320p)', '8K', 'uhd'],
        ['2160p', '4K (2160p)', '4K', 'uhd'],
        ['1440p', '2K (1440p)', '2K', 'qhd'],
        ['1080p60', '1080p 60fps', '1080p', 'fhd'],
        ['1080p', '1080p Full HD', '1080p', 'fhd'],
        ['720p', '720p HD', '720p', 'hd'],
        ['480p', '480p SD', '480p', 'sd'],
        ['360p', '360p', '360p', 'sd']
      ]) {
        videoRows.push({
          type: 'Video',
          quality: q,
          badge,
          tier,
          best: q === 'best',
          title: label,
          sub: 'Saf Rust paralel indirme',
          sizeText: ''
        });
      }
    }

    const estKbps = kbps => (duration > 0 ? formatSize(Math.round(kbps * 125 * duration), true) : '');
    const m4aSize = bestM4a && bestM4a.total_bytes ? formatSize(bestM4a.total_bytes, false) : estKbps(128);
    const opusAudios = audios.filter(f => String(f.container || '').toLowerCase() === 'webm');
    const bestOpus = opusAudios.reduce((a, b) => (!a || (b.total_bytes || b.tbr || 0) > (a.total_bytes || a.tbr || 0)) ? b : a, null);
    const opusSize = bestOpus && bestOpus.total_bytes ? formatSize(bestOpus.total_bytes, false) : estKbps(160);
    const wavSize = duration > 0 ? formatSize(Math.round(duration * 44100 * 4), true) : '';

    const audioRows = [
      {
        type: 'MP3',
        quality: '320kbps',
        badge: 'MP3',
        tier: 'audio',
        title: 'MP3 · 320 kbps (En Yüksek)',
        sub: 'Saf Rust Symphonia + rusty_mp3',
        sizeText: estKbps(320)
      },
      {
        type: 'MP3',
        quality: '256kbps',
        badge: 'MP3',
        tier: 'audio',
        title: 'MP3 · 256 kbps (Yüksek)',
        sub: 'Saf Rust Symphonia + rusty_mp3',
        sizeText: estKbps(256)
      },
      {
        type: 'MP3',
        quality: '192kbps',
        badge: 'MP3',
        tier: 'audio',
        title: 'MP3 · 192 kbps (Standart)',
        sub: 'Dengeli boyut ve yüksek kalite',
        sizeText: estKbps(192)
      },
      {
        type: 'MP3',
        quality: '128kbps',
        badge: 'MP3',
        tier: 'audio',
        title: 'MP3 · 128 kbps (Kompakt)',
        sub: 'Küçük dosya boyutu',
        sizeText: estKbps(128)
      },
      {
        type: 'MP3',
        quality: 'vbr',
        badge: 'MP3',
        tier: 'audio',
        title: 'MP3 · VBR (Değişken Bit Hızı)',
        sub: 'Akıllı psikoakustik V2 profili',
        sizeText: estKbps(190)
      },
      {
        type: 'M4A',
        quality: 'original',
        badge: 'M4A',
        tier: 'audio',
        title: 'M4A · Orijinal Ses (AAC)',
        sub: 'Kayıpsız konteyner aktarımlı AAC',
        sizeText: m4aSize
      },
      {
        type: 'OPUS',
        quality: 'original',
        badge: 'OPUS',
        tier: 'audio',
        title: 'Opus · Orijinal Ses (WebM)',
        sub: 'Yeniden kodlamasız orijinal akış',
        sizeText: opusSize
      },
      {
        type: 'WAV',
        quality: 'original',
        badge: 'WAV',
        tier: 'audio',
        title: 'WAV · Kayıpsız PCM (16-bit)',
        sub: 'Sıkıştırılmamış stüdyo PCM',
        sizeText: wavSize
      }
    ];

    return { videoRows, audioRows, duration, data };
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
      let endSec = i < unique.length - 1
        ? unique[i + 1].startSec
        : (duration > startSec ? duration : startSec + 300);
      if (endSec <= startSec) endSec = startSec + 30;
      const rawTitle = String(unique[i].title || '').trim() || `Bölüm ${i + 1}`;
      const hasNumPrefix = /^\d+[\s.)\-:]/.test(rawTitle);
      const numberedTitle = hasNumPrefix ? rawTitle : `${i + 1}. ${rawTitle}`;
      const timeRange = CT.formatClock(startSec) + ' – ' + CT.formatClock(endSec);
      chapters.push({
        index: i + 1,
        startSec: Math.round(startSec * 10) / 10,
        endSec: Math.round(endSec * 10) / 10,
        rawTitle,
        title: numberedTitle,
        timeRange,
        label: `${numberedTitle} (${timeRange})`
      });
    }
    return chapters;
  }

  function parseManualChapterText(text) {
    if (!text || typeof text !== 'string') return [];
    const lines = text.split(/\r?\n/);
    const rawList = [];
    const tsPattern = /(?:^|[\s([\-–—|])((?:\d{1,3}:)?\d{1,2}:\d{2})(?=$|[\s)\]\-–—|:.,])/;
    for (const rawLine of lines) {
      const line = rawLine.trim();
      if (!line || line.length > 220) continue;
      const m = tsPattern.exec(line);
      if (m) {
        const tsStr = m[1].trim();
        const sec = CT.parseTime(tsStr);
        if (sec !== null && Number.isFinite(sec) && sec >= 0) {
          const idx = line.indexOf(tsStr);
          const before = line.slice(0, idx).replace(/[\s([\-–—|:]+$/, '').trim();
          const after = line.slice(idx + tsStr.length).replace(/^[\s)\]\-–—|:]+/, '').trim();
          let titleStr = (before && after)
            ? `${before} ${after}`.replace(/\s+/g, ' ').trim()
            : (after || before || '').trim();
          titleStr = titleStr.replace(/^[-–—:|•·]+\s*/, '').replace(/\s*[-–—:|•·]+$/, '').trim();
          if (titleStr) {
            rawList.push({ startSec: sec, title: titleStr });
          }
        }
      }
    }
    return buildChapterRanges(rawList);
  }

  function isGenericTimestampTitle(title) {
    return /^Bölüm\s+\d{1,2}:\d{2}/i.test(String(title || '').trim());
  }

  function extractYouTubeChapters() {
    if (Array.isArray(state.probe.chapters) && state.probe.chapters.length > 0) {
      const hasRealTitles = state.probe.chapters.some(c => c && c.title && !isGenericTimestampTitle(c.title));
      if (hasRealTitles) {
        return buildChapterRanges(state.probe.chapters.map(c => Object.assign({}, c)));
      }
    }
    if (state.probe.data && typeof state.probe.data.description === 'string' && state.probe.data.description.trim()) {
      const fromProbeDesc = parseManualChapterText(state.probe.data.description);
      if (fromProbeDesc.length > 0) return fromProbeDesc;
    }
    try {
      const markerItems = document.querySelectorAll(
        'ytd-macro-markers-list-item-renderer, ytd-chapter-renderer, macro-markers-list-item-renderer'
      );
      if (markerItems && markerItems.length > 0) {
        const list = [];
        markerItems.forEach(item => {
          const titleEl = item.querySelector('h4.macro-markers, #details h4, h4, #endpoint #title, .macro-markers-title');
          const timeEl = item.querySelector('#time, span.macro-markers-time, #endpoint span');
          if (titleEl && timeEl) {
            const sec = CT.parseTime(timeEl.textContent.trim());
            const title = (titleEl.getAttribute('title') || titleEl.textContent || '').trim();
            if (sec !== null && title) list.push({ startSec: sec, title });
          }
        });
        if (list.length > 0) return buildChapterRanges(list);
      }
    } catch (_) {}

    try {
      const descContainers = document.querySelectorAll(
        'ytd-watch-metadata #description-inline-expander, #description-inline-expander, ytd-expandable-video-description-body-renderer, #description'
      );
      for (const dc of descContainers) {
        const rawText = dc && (dc.innerText || dc.textContent || '');
        if (rawText && rawText.trim()) {
          const fromText = parseManualChapterText(rawText);
          if (fromText.length > 0) return fromText;
        }
      }
    } catch (_) {}

    try {
      const descLinks = document.querySelectorAll(
        '#description a[href*="t="], #description-inline-expander a[href*="t="], ytd-expandable-video-description-body-renderer a[href*="t="]'
      );
      if (descLinks && descLinks.length > 0) {
        const list = [];
        descLinks.forEach((a, idx) => {
          const text = a.textContent.trim();
          const sec = CT.parseTime(text);
          if (sec !== null) {
            let title = '';
            const block = a.closest('yt-attributed-string, ytd-text-inline-expander, #description-inline-expander, #description') || a.parentElement;
            if (block) {
              const lines = String(block.innerText || block.textContent || '').split(/\r?\n/);
              for (const ln of lines) {
                if (ln.includes(text)) {
                  const pos = ln.indexOf(text);
                  const before = ln.slice(0, pos).replace(/[\s([\-–—|:]+$/, '').trim();
                  const after = ln.slice(pos + text.length).replace(/^[\s)\]\-–—|:]+/, '').trim();
                  title = (after || before || '').trim();
                  if (title) break;
                }
              }
            }
            if (!title) title = `Bölüm ${idx + 1}`;
            list.push({ startSec: sec, title });
          }
        });
        if (list.length > 0) return buildChapterRanges(list);
      }
    } catch (_) {}

    return [];
  }

  /* ---------------------------------------------------------------- settings */

  function loadSettings() {
    chrome.storage.local.get(SETTINGS_KEY, stored => {
      const saved = (stored && stored[SETTINGS_KEY]) || {};
      const merged = Object.assign({}, DEFAULTS, saved);
      merged.jobs = normalizeJobs(saved.jobs !== undefined ? saved.jobs : saved.jobsCount);
      state.settings = merged;
      if (state.panel) state.panel.sync();
      if (state.pill) state.pill.syncSettings();
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
    if (state.pill) state.pill.syncSettings();
  });

  /* -------------------------------------------------------- floating pill */

  function buildFloatingPill() {
    if (state.pill) return state.pill;

    const host = document.createElement('ruvitube-pill');
    host.id = PILL_HOST_ID;
    for (const [k, v] of Object.entries({
      all: 'initial',
      position: 'fixed',
      'z-index': '2147483600',
      left: '0px',
      top: '0px',
      display: 'none',
      'pointer-events': 'auto'
    })) {
      host.style.setProperty(k, v, 'important');
    }

    const shadow = host.attachShadow({ mode: 'closed' });
    const CSS = `
      :host { all: initial; }
      * { box-sizing: border-box; }
      .wrap {
        position: relative;
        font-family: "Segoe UI Variable Text", "Segoe UI", Roboto, system-ui, sans-serif;
        --bg: rgba(14, 15, 19, 0.96);
        --surface: #181a20;
        --border: rgba(255, 110, 60, 0.22);
        --line: rgba(255, 255, 255, 0.08);
        --text: #f5f6f8;
        --muted: #9aa2b1;
        --accent: #ff4d1a;
        --audio: #3fb950;
      }
      .pill {
        display: inline-flex; align-items: center; gap: 7px; height: 34px;
        padding: 0 12px 0 6px; border-radius: 17px;
        background: linear-gradient(135deg, rgba(255, 42, 24, 0.94), rgba(255, 102, 0, 0.92));
        backdrop-filter: blur(18px) saturate(160%);
        -webkit-backdrop-filter: blur(18px) saturate(160%);
        color: #ffffff; border: 1px solid rgba(255, 175, 130, 0.48);
        box-shadow: 0 10px 28px rgba(255, 58, 0, 0.38), inset 0 1px 0 rgba(255, 255, 255, 0.28);
        font-size: 12px; font-weight: 700; cursor: pointer; user-select: none;
        white-space: nowrap; transition: transform .15s, border-color .15s, box-shadow .15s, filter .15s;
      }
      .pill:hover {
        filter: brightness(1.07);
        border-color: rgba(255, 210, 175, 0.75);
        box-shadow: 0 12px 32px rgba(255, 68, 0, 0.52), 0 0 0 1px rgba(255, 120, 50, 0.35);
      }
      .pill.done { background: linear-gradient(135deg, #1f883d, #2ea043); border-color: rgba(63, 185, 80, 0.75); }
      .pill.err { background: linear-gradient(135deg, #c92a2a, #e03131); border-color: rgba(248, 81, 73, 0.75); }
      .ico-badge {
        width: 22px; height: 22px; border-radius: 7px; flex: none;
        display: flex; align-items: center; justify-content: center;
        background: rgba(15, 13, 19, 0.28); color: #fff;
        box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.22);
      }
      .pill-label { max-width: 190px; overflow: hidden; text-overflow: ellipsis; }
      .dismiss {
        position: absolute; top: -6px; right: -6px; width: 17px; height: 17px; border-radius: 50%;
        border: 1px solid var(--border); background: #0d1117; color: var(--muted);
        display: flex; align-items: center; justify-content: center; cursor: pointer;
        opacity: 0; transition: opacity .14s, background .14s, color .14s; padding: 0;
      }
      .wrap:hover .dismiss { opacity: 1; }
      .dismiss:hover { background: #da3633; color: #fff; }
      .menu {
        position: absolute; top: 40px; right: 0; width: 368px;
        background: var(--bg); color: var(--text);
        backdrop-filter: blur(22px) saturate(180%);
        -webkit-backdrop-filter: blur(22px) saturate(180%);
        border: 1px solid var(--border); border-radius: 14px;
        box-shadow: 0 24px 60px rgba(0, 0, 0, 0.7);
        overflow: hidden; display: none; flex-direction: column;
      }
      .menu.open { display: flex; }
      .meta {
        display: flex; align-items: center; justify-content: space-between; gap: 10px;
        padding: 10px 13px; border-bottom: 1px solid var(--line);
        background: linear-gradient(90deg, rgba(255, 72, 24, 0.08), rgba(255, 255, 255, 0.01));
      }
      .meta-left { min-width: 0; flex: 1; }
      .v-title {
        font-size: 12.5px; font-weight: 700; line-height: 1.25; color: #f5f6f8;
        white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
      }
      .v-meta-sub {
        font-size: 10.5px; color: var(--muted); margin-top: 2px;
        white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
      }
      .dur-pill {
        flex: none; padding: 2px 7px; border-radius: 6px;
        background: rgba(255, 255, 255, 0.06); border: 1px solid var(--line);
        color: #d8dee9; font-size: 10.5px; font-weight: 600;
        font-variant-numeric: tabular-nums; font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
      }
      .tabs-bar {
        display: flex; gap: 5px; padding: 6px 10px;
        border-bottom: 1px solid var(--line); background: rgba(255, 255, 255, 0.015);
      }
      .fav-strip {
        padding: 6px 10px; border-bottom: 1px solid var(--line);
        background: rgba(255, 196, 60, 0.05);
        display: flex; flex-direction: column; gap: 5px;
      }
      .fav-strip[hidden] { display: none; }
      .fav-head {
        display: flex; align-items: center; justify-content: space-between;
        font-size: 10px; font-weight: 700; letter-spacing: .06em; text-transform: uppercase;
        color: #f6c453;
      }
      .fav-chips { display: flex; flex-wrap: wrap; gap: 5px; }
      .fav-chip {
        display: inline-flex; align-items: center; gap: 5px; max-width: 100%;
        padding: 4px 7px 4px 8px; border-radius: 7px;
        background: rgba(255, 255, 255, 0.05); border: 1px solid rgba(246, 196, 83, 0.34);
        color: #f8fafc; font: 600 11px/1.2 inherit; cursor: pointer;
        transition: background .12s, border-color .12s;
      }
      .fav-chip:hover { background: rgba(255, 82, 24, 0.18); border-color: rgba(255, 118, 56, 0.6); }
      .fav-chip .f-lbl { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 190px; }
      .fav-chip .f-sz { font-size: 10px; color: var(--muted); font-variant-numeric: tabular-nums; }
      .fav-chip .f-x {
        display: inline-flex; align-items: center; justify-content: center;
        width: 14px; height: 14px; border-radius: 4px; border: 0; padding: 0;
        background: transparent; color: #f6c453; font-size: 11px; cursor: pointer;
      }
      .fav-chip .f-x:hover { background: rgba(255, 255, 255, 0.12); color: #fff; }
      .list {
        max-height: 290px; overflow-y: auto; padding: 6px 8px;
        scrollbar-width: thin; scrollbar-color: rgba(255, 255, 255, 0.2) transparent;
      }
      .sep {
        padding: 6px 6px 3px; font-size: 10px; font-weight: 700; letter-spacing: .06em;
        text-transform: uppercase; color: var(--muted); display: flex; align-items: center; gap: 6px;
      }
      .sep::after { content: ""; flex: 1; height: 1px; background: rgba(240, 246, 252, 0.08); }
      .row-btn {
        width: 100%; display: grid; grid-template-columns: 3px 1fr auto auto auto;
        align-items: center; gap: 9px;
        padding: 6px 8px; border-radius: 8px; border: 1px solid transparent;
        background: transparent; color: var(--text); cursor: pointer; text-align: left;
        font: inherit; transition: background .12s, border-color .12s;
      }
      .row-btn:hover { background: rgba(255, 255, 255, 0.05); border-color: rgba(255, 110, 60, 0.35); }
      .row-btn.best { background: rgba(255, 72, 24, 0.08); border-color: rgba(255, 102, 36, 0.28); }
      .tier-bar {
        width: 3px; height: 22px; border-radius: 99px;
        background: rgba(154, 162, 177, 0.45);
      }
      .tier-bar[data-tier="uhd"] { background: linear-gradient(180deg, #ff2a18, #ff8a00); }
      .tier-bar[data-tier="qhd"] { background: #fb923c; }
      .tier-bar[data-tier="fhd"] { background: #f8fafc; }
      .tier-bar[data-tier="hd"] { background: #94a3b8; }
      .tier-bar[data-tier="audio"] { background: #3fb950; }
      .r-main { min-width: 0; display: flex; flex-direction: column; gap: 1px; }
      .r-title { font-size: 12px; font-weight: 600; color: #f5f6f8; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
      .r-sub { font-size: 10.5px; color: var(--muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
      .r-size {
        font-size: 11px; font-weight: 500; color: #b6bfce;
        font-variant-numeric: tabular-nums; font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
        white-space: nowrap;
      }
      .r-dl {
        font-size: 12px; font-weight: 700; color: var(--accent);
        opacity: 0; transition: opacity .12s; padding: 0 2px;
      }
      .row-btn:hover .r-dl { opacity: 1; }
      .fav-btn {
        width: 22px; height: 22px; border-radius: 6px; border: 0; padding: 0;
        background: transparent; color: #64748b; font-size: 13px; line-height: 1;
        display: inline-flex; align-items: center; justify-content: center; cursor: pointer;
        opacity: 0.45; transition: opacity .12s, color .12s, background .12s;
      }
      .row-btn:hover .fav-btn, .fav-btn.active { opacity: 1; }
      .fav-btn.active { color: #f6c453; }
      .fav-btn:hover { background: rgba(255, 255, 255, 0.08); color: #f6c453; }
      .foot {
        display: flex; align-items: center; justify-content: space-between; gap: 8px;
        padding: 7px 10px; border-top: 1px solid var(--line); background: rgba(0, 0, 0, 0.28);
      }
      .spd-group { display: inline-flex; align-items: center; gap: 3px; }
      .spd-ico { font-size: 11px; color: #ff8a47; margin-right: 2px; user-select: none; }
      .spd-btn {
        padding: 3px 6px; border-radius: 6px; border: 1px solid var(--line);
        background: rgba(255, 255, 255, 0.03); color: var(--muted);
        font: 600 10.5px/1.2 inherit; cursor: pointer;
        transition: background .12s, color .12s, border-color .12s;
      }
      .spd-btn:hover { color: #fff; background: rgba(255, 255, 255, 0.08); }
      .spd-btn.active {
        background: linear-gradient(135deg, #ff2e18, #ff6b00);
        border-color: rgba(255, 160, 110, 0.55); color: #fff; font-weight: 700;
      }
      .foot-actions { display: inline-flex; align-items: center; gap: 4px; }
      .foot-ico-btn {
        width: 26px; height: 26px; border-radius: 7px; border: 1px solid var(--line);
        background: rgba(255, 255, 255, 0.04); color: #d8dee9;
        display: inline-flex; align-items: center; justify-content: center;
        cursor: pointer; padding: 0; font-size: 13px; transition: background .12s, border-color .12s;
      }
      .foot-ico-btn:hover { background: rgba(255, 82, 24, 0.16); border-color: rgba(255, 110, 60, 0.45); color: #fff; }
    `;

    const styleEl = document.createElement('style');
    styleEl.textContent = CSS;
    shadow.appendChild(styleEl);

    const ico = svgEl(
      { width: '14', height: '14', viewBox: '0 0 256 256', fill: 'none' },
      [
        ['path', { d: 'M68 54H136C166.928 54 190 75.4903 190 103.5C190 128.42 171.62 147.88 145.8 151.9L184 202H147.5L114 156H98C90.268 156 85 161.268 85 169V202H68V158C68 138.118 82.1177 124 102 124H134C147.255 124 157 115.493 157 103.5C157 91.5066 147.255 83 134 83H98V113.5C85.8 115.6 75.4 121.8 68 131V54Z', fill: 'currentColor' }],
        ['path', { d: 'M114 91.5L137 104.5L114 117.5V91.5Z', fill: 'currentColor' }]
      ]
    );
    const pillLabel = el('span', { class: 'pill-label' }, 'RuviTube İndir');
    const caret = svgEl(
      { width: '10', height: '10', viewBox: '0 0 12 12', fill: 'none', stroke: 'currentColor', 'stroke-width': '1.8', 'stroke-linecap': 'round' },
      [['path', { d: 'M3 4.5 6 7.5l3-3' }]]
    );
    const pillBtn = el('button', { class: 'pill', type: 'button' }, el('span', { class: 'ico-badge' }, ico), pillLabel, caret);
    const dismissBtn = el(
      'button',
      { class: 'dismiss', type: 'button', title: 'Bu videoda gizle' },
      svgEl({ width: '9', height: '9', viewBox: '0 0 12 12', fill: 'none', stroke: 'currentColor', 'stroke-width': '2', 'stroke-linecap': 'round' }, [['path', { d: 'M3 3l6 6M9 3L3 9' }]])
    );

    const titleEl = el('div', { class: 'v-title' });
    const metaSubEl = el('div', { class: 'v-meta-sub' });
    const durPill = el('span', { class: 'dur-pill' });
    const metaEl = el('div', { class: 'meta' }, el('div', { class: 'meta-left' }, titleEl, metaSubEl), durPill);
    const tabsEl = el('div', { class: 'tabs-bar' });
    const favStripEl = el('div', { class: 'fav-strip', hidden: 'true' });
    const listEl = el('div', { class: 'list' });

    const spdGroup = el('div', { class: 'spd-group', title: 'Paralel İndirme Hızı' }, el('span', { class: 'spd-ico' }, '⚡'));
    const spdBtns = VALID_JOBS.map(j => {
      const b = el('button', { type: 'button', class: 'spd-btn', 'data-jobs': String(j), title: JOBS_LABELS[String(j)] || `x${j}` }, `x${j}`);
      b.addEventListener('click', ev => {
        ev.stopPropagation();
        saveSettings({ jobs: j });
        syncSettings();
      });
      spdGroup.appendChild(b);
      return b;
    });

    const clipOpenBtn = el('button', { class: 'foot-ico-btn', type: 'button', title: 'Özel Kesit Zaman Seçici (Başlangıç–Bitiş)' }, '✂');
    const folderBtn = el('button', { class: 'foot-ico-btn', type: 'button', title: 'İndirilenler Klasörünü Aç' }, '📁');
    const footEl = el('div', { class: 'foot' }, spdGroup, el('div', { class: 'foot-actions' }, clipOpenBtn, folderBtn));

    const menuEl = el('div', { class: 'menu', role: 'menu' }, metaEl, tabsEl, favStripEl, listEl, footEl);
    const wrapEl = el('div', { class: 'wrap' }, pillBtn, dismissBtn, menuEl);
    shadow.appendChild(wrapEl);
    document.documentElement.appendChild(host);

    let dismissedUrl = '';
    let menuOpen = false;
    let anchorMode = 'player'; // 'player' | 'button'
    let pillPos = { x: 1, y: 0 }; // relative player edge (default top-right)
    let dragState = null;
    try {
      const savedPos = JSON.parse(localStorage.getItem('ruvitube_pill_pos_v1') || 'null');
      if (savedPos && Number.isFinite(savedPos.x) && Number.isFinite(savedPos.y)) {
        pillPos = {
          x: Math.max(0, Math.min(1, Number(savedPos.x))),
          y: Math.max(0, Math.min(1, Number(savedPos.y)))
        };
      }
    } catch (_) {}

    function refreshDefaultPillLabel() {
      if (pillBtn.classList.contains('done') || pillBtn.classList.contains('err')) return;
      const t = currentTitle();
      pillLabel.textContent = t && t !== 'YouTube' ? `İndir: ${t}` : 'RuviTube İndir';
    }

    function positionPill() {
      if (anchorMode === 'button') {
        const btn = document.getElementById(UI_ID);
        if (btn) {
          const r = btn.getBoundingClientRect();
          host.style.setProperty('left', `${Math.max(8, Math.min(window.innerWidth - 368, r.left))}px`, 'important');
          host.style.setProperty('top', `${Math.max(8, r.bottom + 6)}px`, 'important');
          host.style.setProperty('display', 'block', 'important');
          menuEl.style.left = '0px';
          menuEl.style.right = 'auto';
          menuEl.style.top = '0px';
          pillBtn.style.display = 'none';
          dismissBtn.style.display = 'none';
          return;
        }
      }
      pillBtn.style.display = 'inline-flex';
      dismissBtn.style.display = 'flex';
      refreshDefaultPillLabel();

      if (!state.settings.showFloatingPill && !menuOpen) {
        host.style.setProperty('display', 'none', 'important');
        return;
      }
      if (document.fullscreenElement && !menuOpen) {
        host.style.setProperty('display', 'none', 'important');
        return;
      }
      if (!isWatchLikePage() || dismissedUrl === currentVideoUrl()) {
        host.style.setProperty('display', 'none', 'important');
        return;
      }
      const player = document.querySelector('#movie_player video') || document.querySelector('video');
      if (!player) {
        host.style.setProperty('display', 'none', 'important');
        return;
      }
      const r = player.getBoundingClientRect();
      if (r.width < 220 || r.height < 120 || r.bottom < 0 || r.top > window.innerHeight) {
        host.style.setProperty('display', 'none', 'important');
        return;
      }
      host.style.setProperty('display', 'block', 'important');
      const pillW = pillBtn.offsetWidth || 168;
      const pillH = pillBtn.offsetHeight || 34;
      const minX = Math.max(8, r.left + 12);
      const maxX = Math.min(window.innerWidth - pillW - 12, r.right - pillW - 12);
      const minY = Math.max(60, r.top + 12);
      const maxY = Math.min(window.innerHeight - pillH - 12, r.bottom - pillH - 52);
      const spanX = Math.max(0, maxX - minX);
      const spanY = Math.max(0, maxY - minY);
      const left = minX + spanX * pillPos.x;
      const top = minY + spanY * pillPos.y;
      host.style.setProperty('left', `${Math.round(left)}px`, 'important');
      host.style.setProperty('top', `${Math.round(top)}px`, 'important');
      if (pillPos.x < 0.45) {
        menuEl.style.left = '0px';
        menuEl.style.right = 'auto';
      } else {
        menuEl.style.left = 'auto';
        menuEl.style.right = '0px';
      }
      menuEl.style.top = '40px';
    }

    let activeMenuTab = 'all'; // 'all' | 'video' | 'audio' | 'chapters'

    function favId(row) {
      if (!row) return '';
      if (row.type === 'Video' && String(row.quality || '').includes('|')) {
        return `Video:${String(row.quality).split('|')[1]}`;
      }
      return `${row.type}:${row.quality}`;
    }

    function getFavList() {
      return Array.isArray(state.settings.favorites) ? state.settings.favorites : [];
    }

    function isFavRow(row) {
      const fid = favId(row);
      return getFavList().includes(fid);
    }

    function toggleFavRow(row) {
      const fid = favId(row);
      if (!fid) return;
      const cur = getFavList();
      const next = cur.includes(fid) ? cur.filter(x => x !== fid) : [...cur, fid];
      saveSettings({ favorites: next });
      renderMenu();
    }

    function favShortLabel(fid) {
      if (fid.startsWith('MP3:')) return `MP3 ${fid.slice(4)}`;
      if (fid.startsWith('Video:')) {
        const q = fid.slice(6);
        if (q === 'best') return 'Video · En İyi';
        if (q === 'h264') return 'MP4 · H.264';
        return q.replace(':mp4', ' MP4').replace(':webm', ' WEBM');
      }
      return fid.replace(':original', '');
    }

    function renderMenu() {
      const { videoRows, audioRows, duration, data } = buildQualityRows();
      titleEl.textContent = currentTitle();
      const subParts = [];
      if (data && data.channel) subParts.push(data.channel);
      subParts.push('Saf Rust');
      if (state.probe.loading) subParts.push('Taranıyor...');
      metaSubEl.textContent = subParts.join(' · ');

      const durStr = data && data.isLive ? 'CANLI' : formatDuration(duration);
      durPill.textContent = durStr;
      durPill.style.display = durStr ? 'inline-block' : 'none';

      tabsEl.textContent = '';
      favStripEl.textContent = '';
      listEl.textContent = '';

      const detectedChapters = extractYouTubeChapters();
      const makeTabSvg = paths => {
        const ns = 'http://www.w3.org/2000/svg';
        const svg = document.createElementNS(ns, 'svg');
        svg.setAttribute('viewBox', '0 0 24 24');
        svg.setAttribute('width', '15');
        svg.setAttribute('height', '15');
        svg.setAttribute('fill', 'none');
        svg.setAttribute('stroke', 'currentColor');
        svg.setAttribute('stroke-width', '2.1');
        svg.setAttribute('stroke-linecap', 'round');
        svg.setAttribute('stroke-linejoin', 'round');
        for (const d of paths) {
          const p = document.createElementNS(ns, 'path');
          p.setAttribute('d', d);
          svg.appendChild(p);
        }
        return svg;
      };
      const tabDefs = [
        {
          id: 'all',
          title: `Tümü (${videoRows.length + audioRows.length})`,
          badge: '',
          paths: ['M8 6h13', 'M8 12h13', 'M8 18h13', 'M3 6h.01', 'M3 12h.01', 'M3 18h.01']
        },
        {
          id: 'video',
          title: `Video Kaliteleri (${videoRows.length})`,
          badge: `${videoRows.length}`,
          paths: ['M4 6a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6z', 'M16 10l5-3v10l-5-3v-4z']
        },
        {
          id: 'audio',
          title: `Ses Formatları (${audioRows.length})`,
          badge: `${audioRows.length}`,
          paths: ['M9 18V5l12-2v13', 'M6 15a3 3 0 1 0 0 6 3 3 0 0 0 0-6z', 'M18 13a3 3 0 1 0 0 6 3 3 0 0 0 0-6z']
        },
        {
          id: 'chapters',
          title: detectedChapters.length > 0
            ? `Parçalı & Bölümler (${detectedChapters.length} Bölüm)`
            : 'Parçalı / Özel Kesit İndir',
          badge: detectedChapters.length > 0 ? `${detectedChapters.length}` : '',
          paths: [
            'M6 3a3 3 0 1 0 0 6 3 3 0 0 0 0-6z',
            'M6 15a3 3 0 1 0 0 6 3 3 0 0 0 0-6z',
            'M20 4L8.12 15.88',
            'M14.47 14.48L20 20',
            'M8.12 8.12L12 12'
          ]
        }
      ];
      for (const td of tabDefs) {
        const active = activeMenuTab === td.id;
        const tb = el('button', {
          type: 'button',
          title: td.title,
          'aria-label': td.title,
          style: active
            ? 'flex:1;display:inline-flex;align-items:center;justify-content:center;gap:4px;padding:6px 4px;border-radius:8px;border:1px solid rgba(255,130,70,.6);background:linear-gradient(135deg,#ff2e18,#ff6b00);color:#fff;font:700 10.5px inherit;cursor:pointer;box-shadow:0 2px 8px rgba(255,64,24,.28)'
            : 'flex:1;display:inline-flex;align-items:center;justify-content:center;gap:4px;padding:6px 4px;border-radius:8px;border:1px solid rgba(255,255,255,.09);background:rgba(255,255,255,.04);color:#c9d1d9;font:600 10.5px inherit;cursor:pointer'
        }, makeTabSvg(td.paths), td.badge ? el('span', { style: 'font-size:10px;opacity:.92' }, td.badge) : null);
        tb.addEventListener('click', ev => {
          ev.stopPropagation();
          activeMenuTab = td.id;
          renderMenu();
        });
        tabsEl.appendChild(tb);
      }

      // Pinned Favorites strip (always fixed above scroll area)
      const favIds = getFavList();
      const allPool = [...videoRows, ...audioRows];
      if (favIds.length > 0 && activeMenuTab !== 'chapters') {
        favStripEl.removeAttribute('hidden');
        favStripEl.appendChild(el('div', { class: 'fav-head' }, el('span', {}, '★ Sabitlenmiş Favoriler'), el('span', { style: 'font-weight:500;opacity:.8' }, 'Tek tıkla indir')));
        const chipsWrap = el('div', { class: 'fav-chips' });
        for (const fid of favIds) {
          const matched = allPool.find(r => favId(r) === fid);
          const dlType = matched ? matched.type : fid.split(':')[0];
          const dlQual = matched ? matched.quality : fid.split(':').slice(1).join(':');
          const lblText = matched ? (matched.type === 'MP3' ? `MP3 ${matched.quality}` : matched.title.split('·')[0].trim() + (matched.sub.includes('MP4') ? ' MP4' : '')) : favShortLabel(fid);
          const chipBtn = el('div', { class: 'fav-chip', role: 'button', tabindex: '0', title: `${lblText} — Favoriden hemen indir` },
            el('span', { style: 'color:#f6c453;font-size:11px' }, '★'),
            el('span', { class: 'f-lbl' }, lblText),
            matched && matched.sizeText ? el('span', { class: 'f-sz' }, matched.sizeText) : null
          );
          const unpin = el('button', { type: 'button', class: 'f-x', title: 'Favorilerden kaldır' }, '×');
          unpin.addEventListener('click', ev => {
            ev.stopPropagation();
            saveSettings({ favorites: getFavList().filter(x => x !== fid) });
            renderMenu();
          });
          chipBtn.appendChild(unpin);
          chipBtn.addEventListener('click', () => {
            closeMenu();
            start(dlType, dlQual, null);
          });
          chipsWrap.appendChild(chipBtn);
        }
        favStripEl.appendChild(chipsWrap);
      } else {
        favStripEl.setAttribute('hidden', 'true');
      }

      const buildFormatRowEl = row => {
        const starred = isFavRow(row);
        const favBtn = el('button', {
          type: 'button',
          class: starred ? 'fav-btn active' : 'fav-btn',
          title: starred ? 'Favorilerden çıkar' : 'Üste sabitle (Favorilere ekle)'
        }, starred ? '★' : '☆');
        favBtn.addEventListener('click', ev => {
          ev.stopPropagation();
          toggleFavRow(row);
        });
        const btn = el(
          'button',
          { type: 'button', class: row.best ? 'row-btn best' : 'row-btn' },
          el('span', { class: 'tier-bar', 'data-tier': row.tier }),
          el('span', { class: 'r-main' }, el('span', { class: 'r-title' }, row.title), el('span', { class: 'r-sub' }, row.sub)),
          row.sizeText ? el('span', { class: 'r-size' }, row.sizeText) : el('span', { class: 'r-size' }, ''),
          el('span', { class: 'r-dl' }, '↓'),
          favBtn
        );
        btn.addEventListener('click', () => {
          closeMenu();
          start(row.type, row.quality, null);
        });
        return btn;
      };

      if (activeMenuTab === 'chapters') {
        const clipRowBtn = el(
          'button',
          { type: 'button', class: 'row-btn best', style: 'margin-bottom:4px' },
          el('span', { class: 'tier-bar', 'data-tier': 'uhd' }),
          el(
            'span',
            { class: 'r-main' },
            el('span', { class: 'r-title' }, '✂ Özel Zaman Aralığı Seç (Başlangıç – Bitiş)'),
            el('span', { class: 'r-sub' }, 'Saniye hassasiyetinde video veya ses kesiti kırp')
          ),
          el('span', { class: 'r-size' }, 'KESİT'),
          el('span', { class: 'r-dl' }, '→'),
          el('span', {})
        );
        clipRowBtn.addEventListener('click', () => {
          closeMenu();
          buildPanel().show();
        });
        listEl.appendChild(clipRowBtn);
      }

      if (activeMenuTab === 'all' || activeMenuTab === 'video') {
        listEl.appendChild(el('div', { class: 'sep' }, 'Video'));
        for (const row of videoRows) {
          listEl.appendChild(buildFormatRowEl(row));
        }
      }

      if (activeMenuTab === 'all' || activeMenuTab === 'audio') {
        listEl.appendChild(el('div', { class: 'sep' }, 'Ses'));
        for (const row of audioRows) {
          listEl.appendChild(buildFormatRowEl(row));
        }
      }

      if ((activeMenuTab === 'all' || activeMenuTab === 'chapters') && detectedChapters.length > 0) {
        listEl.appendChild(el('div', { class: 'sep' }, `Bölümler (${detectedChapters.length})`));
        for (const ch of detectedChapters) {
          const chClip = { startSec: ch.startSec, endSec: ch.endSec, chapterTitle: ch.title };
          const vq = state.settings.videoQuality || '720p';
          const aq = state.settings.audioQuality || '320kbps';
          const mp4Btn = el('button', {
            type: 'button',
            class: 'f-btn',
            style: 'padding:4px 8px;font-size:10.5px;background:rgba(255,82,24,.16);border:1px solid rgba(255,102,36,.42);color:#ffd2b8;border-radius:6px;cursor:pointer;font-weight:700',
            title: `${ch.title} bölümünü Video (${vq}) olarak indir`
          }, 'MP4');
          mp4Btn.addEventListener('click', ev => {
            ev.stopPropagation();
            closeMenu();
            start('Video', vq, chClip);
          });
          const mp3Btn = el('button', {
            type: 'button',
            class: 'f-btn',
            style: 'padding:4px 8px;font-size:10.5px;background:rgba(63,185,80,.16);border:1px solid rgba(63,185,80,.42);color:#7ee787;border-radius:6px;cursor:pointer;font-weight:700',
            title: `${ch.title} bölümünü MP3 (${aq}) olarak indir`
          }, 'MP3');
          mp3Btn.addEventListener('click', ev => {
            ev.stopPropagation();
            closeMenu();
            start('MP3', aq, chClip);
          });
          const rowEl = el(
            'div',
            { class: 'row-btn', style: 'grid-template-columns:3px 1fr auto;cursor:default' },
            el('span', { class: 'tier-bar', 'data-tier': 'hd' }),
            el(
              'span',
              { class: 'r-main' },
              el('span', { class: 'r-title', title: ch.title }, ch.title),
              el('span', { class: 'r-sub' }, `${ch.timeRange} · ${CT.formatDuration(ch.endSec - ch.startSec)}`)
            ),
            el('span', { style: 'display:inline-flex;gap:4px;flex-shrink:0' }, mp4Btn, mp3Btn)
          );
          listEl.appendChild(rowEl);
        }
      }
    }

    function openMenu(mode = 'player') {
      anchorMode = mode;
      menuOpen = true;
      menuEl.classList.add('open');
      renderMenu();
      positionPill();
      refreshVideoProbe(false);
    }

    function closeMenu() {
      menuOpen = false;
      anchorMode = 'player';
      menuEl.classList.remove('open');
      positionPill();
    }

    pillBtn.addEventListener('pointerdown', ev => {
      if (ev.button !== 0) return;
      dragState = { startX: ev.clientX, startY: ev.clientY, moved: false };
      try { pillBtn.setPointerCapture(ev.pointerId); } catch (_) {}
    });

    pillBtn.addEventListener('pointermove', ev => {
      if (!dragState) return;
      const dx = ev.clientX - dragState.startX;
      const dy = ev.clientY - dragState.startY;
      if (!dragState.moved && Math.hypot(dx, dy) < 5) return;
      dragState.moved = true;
      const player = document.querySelector('#movie_player video') || document.querySelector('video');
      if (!player) return;
      const r = player.getBoundingClientRect();
      const pillW = pillBtn.offsetWidth || 168;
      const pillH = pillBtn.offsetHeight || 34;
      const minX = Math.max(8, r.left + 12);
      const maxX = Math.max(minX + 1, Math.min(window.innerWidth - pillW - 12, r.right - pillW - 12));
      const minY = Math.max(60, r.top + 12);
      const maxY = Math.max(minY + 1, Math.min(window.innerHeight - pillH - 12, r.bottom - pillH - 52));
      pillPos.x = Math.max(0, Math.min(1, (ev.clientX - pillW / 2 - minX) / (maxX - minX)));
      pillPos.y = Math.max(0, Math.min(1, (ev.clientY - pillH / 2 - minY) / (maxY - minY)));
      positionPill();
    });

    pillBtn.addEventListener('pointerup', ev => {
      if (!dragState) return;
      const wasMoved = dragState.moved;
      dragState = null;
      try { pillBtn.releasePointerCapture(ev.pointerId); } catch (_) {}
      if (wasMoved) {
        try { localStorage.setItem('ruvitube_pill_pos_v1', JSON.stringify(pillPos)); } catch (_) {}
      }
    });

    pillBtn.addEventListener('click', ev => {
      ev.stopPropagation();
      if (dragState && dragState.moved) return;
      if (menuOpen) closeMenu();
      else openMenu('player');
    });

    dismissBtn.addEventListener('click', ev => {
      ev.stopPropagation();
      dismissedUrl = currentVideoUrl();
      closeMenu();
      positionPill();
    });

    clipOpenBtn.addEventListener('click', () => {
      closeMenu();
      buildPanel().show();
    });

    folderBtn.addEventListener('click', () => {
      chrome.runtime.sendMessage({ action: 'open_folder' }).catch(() => {});
    });

    document.addEventListener('click', ev => {
      if (!menuOpen) return;
      if (!host.contains(ev.target) && !ev.target.closest?.(`#${UI_ID}`)) {
        closeMenu();
      }
    }, true);

    window.addEventListener('scroll', () => { if (menuOpen || state.settings.showFloatingPill) positionPill(); }, { passive: true });
    window.addEventListener('resize', () => { if (menuOpen || state.settings.showFloatingPill) positionPill(); }, { passive: true });

    function setJobProgress(job) {
      pillBtn.classList.remove('done', 'err');
      if (job.state === 'done') {
        pillBtn.classList.add('done');
        pillLabel.textContent = '✓ Tamamlandı';
        setTimeout(() => {
          pillBtn.classList.remove('done');
          refreshDefaultPillLabel();
        }, 5000);
      } else if (job.state === 'failed') {
        pillBtn.classList.add('err');
        pillLabel.textContent = '✕ Hata';
        setTimeout(() => {
          pillBtn.classList.remove('err');
          refreshDefaultPillLabel();
        }, 5000);
      } else {
        const pct = Math.max(0, Math.min(100, Math.round(Number(job.percent) || 0)));
        pillLabel.textContent = `${pct}% · ${job.label || 'İndiriliyor'}`;
      }
    }

    function syncSettings() {
      const curJobs = String(normalizeJobs(state.settings.jobs));
      for (const b of spdBtns) {
        b.classList.toggle('active', b.dataset.jobs === curJobs);
      }
      positionPill();
    }

    state.pill = {
      host, openMenu, closeMenu, isOpen: () => menuOpen, renderMenu, positionPill, setJobProgress, syncSettings
    };
    syncSettings();
    return state.pill;
  }

  /* ------------------------------------------------------------- clip panel */

  function buildPanel() {
    if (state.panel) return state.panel;

    const host = document.createElement('div');
    host.id = PANEL_ID;
    host.style.cssText = 'position:fixed;inset:0;z-index:2147483620;display:none';
    const shadow = host.attachShadow({ mode: 'open' });
    shadow.innerHTML = `
      <style>
        :host{all:initial}
        .backdrop{position:fixed;inset:0;background:rgba(0,0,0,.62);display:flex;align-items:center;justify-content:center;font-family:"Segoe UI",Roboto,Arial,sans-serif}
        .sheet{width:min(440px,92vw);background:#161b22;color:#f0f6fc;border:1px solid #30363d;border-radius:16px;box-shadow:0 22px 64px rgba(0,0,0,.65);overflow:hidden}
        .head{display:flex;align-items:center;justify-content:space-between;padding:14px 16px 6px}
        .head h2{font-size:15px;margin:0;font-weight:600;display:flex;align-items:center;gap:8px}
        .sub{font-size:11.5px;color:#8b949e;padding:0 16px 10px}
        .body{padding:0 16px 4px}
        .row{display:flex;gap:8px;align-items:flex-end;margin-bottom:10px}
        .field{flex:1;display:flex;flex-direction:column;gap:4px;min-width:0}
        .field label{font-size:11px;color:#8b949e}
        input[type=text]{background:#0d1117;border:1px solid #30363d;border-radius:8px;color:#f0f6fc;font-size:13px;padding:8px 10px;font-family:inherit;width:100%;box-sizing:border-box}
        input[type=text]:focus{outline:none;border-color:#ff521a}
        input.invalid{border-color:#f85149}
        select{background:#0d1117;border:1px solid #30363d;border-radius:8px;color:#f0f6fc;font-size:12px;padding:8px 10px;font-family:inherit;flex:1}
        .chip,.mini{background:#21262d;border:1px solid #30363d;border-radius:999px;color:#e6edf3;font-size:11px;padding:6px 10px;cursor:pointer;font-family:inherit;white-space:nowrap}
        .chip:hover,.mini:hover{background:#30363d}
        .chips{display:flex;gap:6px;flex-wrap:wrap;margin-bottom:10px}
        .summary{font-size:12px;color:#c9d1d9;background:#0d1117;border:1px solid #30363d;border-radius:8px;padding:8px 10px;margin-bottom:10px;display:flex;justify-content:space-between;gap:8px}
        .summary b{color:#fff;font-weight:600}
        .err{color:#ff7b72;font-size:11px;min-height:14px;margin:-2px 0 8px}
        .primary{flex:1;background:linear-gradient(135deg,#ff2e18,#ff6b00);border:0;border-radius:10px;color:#fff;font-size:13px;font-weight:600;padding:10px;cursor:pointer;font-family:inherit}
        .primary:hover{filter:brightness(1.08)}
        .primary:disabled{background:#3d241c;color:#8b949e;cursor:default}
        .secondary{flex:1;background:#21262d;border:1px solid #30363d;border-radius:10px;color:#e6edf3;font-size:12px;padding:9px 8px;cursor:pointer;font-family:inherit}
        .secondary:hover{background:#30363d}
        .secondary:disabled{opacity:.5;cursor:default}
        .foot{display:flex;align-items:center;justify-content:space-between;border-top:1px solid #30363d;padding:10px 16px;gap:8px;flex-wrap:wrap}
        .link{background:none;border:0;color:#58a6ff;font-size:12px;cursor:pointer;font-family:inherit;padding:0}
        .link:hover{text-decoration:underline}
        .toggles{display:flex;gap:6px;flex-wrap:wrap}
        .toggle{font-size:11px;border:1px solid #30363d;background:#0d1117;border-radius:999px;padding:5px 9px;cursor:pointer;color:#8b949e;font-family:inherit}
        .toggle[aria-pressed=true]{background:rgba(47,129,247,.2);border-color:#2f81f7;color:#f0f6fc}
        .job{margin-top:8px;margin-bottom:8px;font-size:11.5px;color:#8b949e;display:none;flex-direction:column;gap:6px}
        .job.visible{display:flex}
        .bar{height:4px;background:#21262d;border-radius:999px;overflow:hidden}
        .bar span{display:block;height:100%;width:0;background:#2f81f7;transition:width .25s}
        .job.done .bar span{background:#3fb950}
        .job.error .bar span{background:#f85149}
        .jobtext{display:flex;justify-content:space-between;gap:8px}
        .jobtext em{font-style:normal;color:#8b949e;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
        .chapter-box{background:#0d1117;border:1px solid #30363d;border-radius:10px;padding:10px;margin-bottom:10px}
        .chapter-header{display:flex;justify-content:space-between;align-items:center;margin-bottom:6px}
        .chapter-header label{font-size:11px;color:#8b949e}
        textarea{background:#161b22;border:1px solid #30363d;border-radius:8px;color:#f0f6fc;font-size:12px;padding:6px 8px;font-family:inherit;width:100%;box-sizing:border-box;resize:vertical}
        textarea:focus{outline:none;border-color:#2f81f7}
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
              <button class="primary" id="audio" style="background:#238636"></button>
            </div>
            <div class="row" style="align-items:center">
              <label id="lblJobs" for="jobs" style="font-size:11px;color:#8b949e;min-width:108px"></label>
              <select id="jobs"></select>
            </div>
            <div class="row">
              <button class="secondary" id="fullV"></button>
              <button class="secondary" id="fullA"></button>
            </div>
            <div class="job" id="job">
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
    $('title').textContent = t('clipTitle');
    $('sub').textContent = t('clipSubtitle');
    $('lblStart').textContent = t('clipStart');
    $('lblEnd').textContent = t('clipEnd');
    $('now').textContent = t('clipNow');
    $('endNow').textContent = t('clipEndNow');
    $('lblChapters').textContent = t('clipChapters');
    $('btnToggleManual').textContent = t('clipManualParse') + ' ▼';
    $('btnParseChapters').textContent = t('clipParseBtn');
    $('manualChapterText').placeholder = t('clipManualHint');
    $('btnDlChapterVideo').textContent = t('clipDlChapterVideo');
    $('btnDlChapterAudio').textContent = t('clipDlChapterAudio');
    $('lblLen').textContent = 'Süre (dk:sn)';
    $('applyLen').textContent = 'Uygula';
    $('fullV').textContent = 'Tümünü indir (Video)';
    $('fullA').textContent = 'Tümünü indir (Ses)';
    $('close').textContent = t('clipClose');
    $('video').textContent = t('clipDownloadVideo');
    $('audio').textContent = t('clipDownloadAudio');
    $('lblJobs').textContent = t('clipJobs');
    $('full').textContent = t('clipFull');
    $('start').placeholder = t('clipPlaceholder');
    $('end').placeholder = t('clipPlaceholder');

    let currentChapters = [];
    let selectedChapterTitle = null;

    function populateChapterDropdown(chapters) {
      currentChapters = chapters || [];
      const sel = $('chapterSelect');
      sel.innerHTML = '';
      if (!currentChapters.length) {
        const opt = document.createElement('option');
        opt.value = '';
        opt.textContent = t('clipNoChapters');
        sel.appendChild(opt);
        $('chapterDlRow').style.display = 'none';
        return;
      }
      const defaultOpt = document.createElement('option');
      defaultOpt.value = '';
      defaultOpt.textContent = t('clipSelectChapter');
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
      populateChapterDropdown(extractYouTubeChapters());
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

    function resolveAudioChoice(val) {
      if (val === 'm4a') return { type: 'M4A', quality: 'original' };
      if (val === 'wav') return { type: 'WAV', quality: 'original' };
      return { type: 'MP3', quality: val || '320kbps' };
    }

    function sync() {
      const vqVal = state.settings.videoQuality || '1080p';
      $('vq').value = QUALITY_LABELS[vqVal] ? vqVal : 'best';
      const aqVal = state.settings.audioQuality || '320kbps';
      $('aq').value = AUDIO_LABELS[aqVal] ? aqVal : '320kbps';
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
        ? t('clipManualParse') + ' ▲'
        : t('clipManualParse') + ' ▼';
    });

    $('btnParseChapters').addEventListener('click', () => {
      const parsed = parseManualChapterText($('manualChapterText').value);
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

    $('btnDlChapterVideo').addEventListener('click', () => submit('Video', $('vq').value));
    $('btnDlChapterAudio').addEventListener('click', () => {
      const a = resolveAudioChoice($('aq').value);
      submit(a.type, a.quality);
    });

    $('chips').addEventListener('click', event => {
      const chip = event.target.closest('.chip');
      if (!chip) return;
      if (CT.parseTime($('start').value) === null) {
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
    $('audio').addEventListener('click', () => {
      const a = resolveAudioChoice($('aq').value);
      submit(a.type, a.quality);
    });

    function submit(type, quality) {
      const result = readClip();
      if (!result.ok) return;
      let startSec = result.startSec;
      let endSec = result.endSec;
      const duration = videoDuration();
      if (duration > 0 && endSec > duration) endSec = Math.floor(duration * 10) / 10;
      if (endSec - startSec < 1) {
        $('err').textContent = t('errShort');
        return;
      }
      const clipPayload = { startSec, endSec };
      if (selectedChapterTitle) clipPayload.chapterTitle = selectedChapterTitle;
      start(type, quality, clipPayload);
    }

    $('fullV').addEventListener('click', () => start('Video', $('vq').value, null));
    $('fullA').addEventListener('click', () => {
      const a = resolveAudioChoice($('aq').value);
      start(a.type, a.quality, null);
    });
    $('full').addEventListener('click', () => {
      hide();
      start('Video', state.settings.videoQuality || '1080p', null);
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
      $('start').value = CT.formatClock(playbackTime());
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
      $('jobStatus').textContent = job.status || (job.state === 'done' ? t('statusDone') : '');
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
    const url = currentVideoUrl();
    const channel = String((state.probe.data && state.probe.data.channel) || '').trim();
    const title = currentTitle();
    const audioLang = String((state.probe.data && state.probe.data.activeAudioLang) || '').trim();
    const payload = { action: 'download', type, quality, url, jobs, channel, title };
    if (audioLang) payload.audioLang = audioLang;
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
        label: response.label,
        percent: 0,
        status: t('statusPreparing'),
        clip: !!clip
      };
      state.jobs.set(response.jobId, job);

      showStatus(job.label + ' · ' + job.status);
      if (state.pill) state.pill.setJobProgress({ state: 'running', percent: 0, label: job.label });
      if (state.panel) state.panel.showJob({ jobId: job.jobId, state: 'running', status: job.status, percent: 0, clip: job.clip });
    });
  }

  function showStatus(text) {
    const status = document.querySelector('.rt-status');
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
    if (!isWatchLikePage()) {
      const stale = document.getElementById(UI_ID);
      if (stale) stale.remove();
      if (state.pill) state.pill.positionPill();
      return;
    }

    addStyle();
    buildFloatingPill().positionPill();

    const target = getTarget();
    if (!target || !document.documentElement.contains(target)) return;

    const existing = document.getElementById(UI_ID);
    if (existing) {
      const stillPlaced = isSubscribeAnchor(target)
        ? existing.previousElementSibling === target
        : existing.parentElement === target;
      if (stillPlaced) return;
      existing.remove();
    }

    const container = document.createElement('div');
    container.id = UI_ID;
    container.className = 'rt-container';

    const dlBtn = el(
      'button',
      { type: 'button', class: 'rt-btn', title: 'RuviTube — Video, MP3 ve Bölüm İndir', 'aria-label': 'RuviTube İndir' },
      el('span', { class: 'rt-btn-ico' }, svgEl(
        { width: '19', height: '19', viewBox: '0 0 256 256', fill: 'none' },
        [
          ['path', { d: 'M68 54H136C166.928 54 190 75.4903 190 103.5C190 128.42 171.62 147.88 145.8 151.9L184 202H147.5L114 156H98C90.268 156 85 161.268 85 169V202H68V158C68 138.118 82.1177 124 102 124H134C147.255 124 157 115.493 157 103.5C157 91.5066 147.255 83 134 83H98V113.5C85.8 115.6 75.4 121.8 68 131V54Z', fill: 'currentColor' }],
          ['path', { d: 'M114 91.5L137 104.5L114 117.5V91.5Z', fill: 'currentColor' }]
        ]
      ))
    );
    dlBtn.addEventListener('click', event => {
      event.stopPropagation();
      const pill = buildFloatingPill();
      if (pill.isOpen()) pill.closeMenu();
      else pill.openMenu('button');
    });

    const status = el('span', { class: 'rt-status' });
    container.append(dlBtn, status);
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
      if (state.pill) state.pill.setJobProgress(job);

      if (job.state === 'done') {
        showStatus('✓ ' + (job.fileName || t('statusDone')));
      } else if (job.state === 'failed') {
        showStatus('✕ ' + (job.error || t('statusError')));
      } else {
        const pct = Math.max(0, Math.min(100, Math.round(Number(job.percent) || 0)));
        showStatus(pct + '% · ' + (job.status || t('statusDownloading')));
      }
    });
  }

  /* --------------------------------------------------------------- bootstrap */

  loadSettings();

  let timer = 0;
  function schedule() {
    clearTimeout(timer);
    timer = setTimeout(() => {
      createUI();
      if (isWatchLikePage()) refreshVideoProbe(false);
    }, 180);
  }

  function boot() {
    addStyle();
    createUI();
    if (isWatchLikePage()) setTimeout(() => refreshVideoProbe(false), 300);
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
      if (state.pill) state.pill.closeMenu();
      schedule();
    }, { passive: true });
  });
  window.addEventListener('popstate', schedule, { passive: true });

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
