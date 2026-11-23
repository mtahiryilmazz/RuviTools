/*
 * RuviTube — MV3 Service Worker.
 *
 * Manages Native Messaging ports (com.ruvitube.nativehost), download queue,
 * format/quality normalization, and optional probe fallback.
 */
'use strict';

const HOST = 'com.ruvitube.nativehost';
const ALL_IN_ONE_URL = 'https://raw.githubusercontent.com/mtahiryilmazz/RuviTools/main/ruvitube/installer/RuviTube-AllInOne.cmd';
const JOBS_KEY = 'ruvitube.jobs';
const SETTINGS_KEY = 'ruvitube.settings';
const MAX_ACTIVE = 2;
const JOB_TTL_MS = 6 * 60 * 60 * 1000;

chrome.storage.session.set({ 'ruvitube.bgVersion': 'sw-2.2.0-ruvi' }).catch(() => {});

const VIDEO_QUALITIES = [
  'best', 'h264', 'mp4-h264', '4320p60', '4320p', '2160p60', '2160p',
  '1440p60', '1440p', '1080p60', '1080p', '720p60', '720p', '480p', '360p', '240p', '144p'
];
const AUDIO_QUALITIES = ['320kbps', '256kbps', '192kbps', '128kbps', 'vbr', 'original'];
const MEDIA_TYPES = ['Video', 'MP3', 'M4A', 'OPUS', 'WAV', 'AAC'];

let jobsCache = null;
let lock = Promise.resolve();
const nativePorts = new Map();

function withLock(fn) {
  const run = lock.then(fn, fn);
  lock = run.then(() => undefined, () => undefined);
  return run;
}

async function getJobs() {
  if (jobsCache) return jobsCache;
  const stored = await chrome.storage.session.get(JOBS_KEY);
  jobsCache = (stored && stored[JOBS_KEY]) || {};
  return jobsCache;
}

async function persist(jobs) {
  jobsCache = jobs;
  const now = Date.now();
  for (const [id, job] of Object.entries(jobs)) {
    if (job.updatedAt && now - job.updatedAt > JOB_TTL_MS) delete jobs[id];
  }
  await chrome.storage.session.set({ [JOBS_KEY]: jobs });
}

const DEFAULT_JOBS = 4;
const MIN_JOBS = 1;
const MAX_JOBS = 16;

function normalizeJobs(raw) {
  if (raw === undefined || raw === null || raw === '') return DEFAULT_JOBS;
  const n = Math.round(Number(raw));
  if (!Number.isFinite(n)) return DEFAULT_JOBS;
  return Math.min(MAX_JOBS, Math.max(MIN_JOBS, n));
}

async function getSettings() {
  const stored = await chrome.storage.local.get(SETTINGS_KEY);
  const s = (stored && stored[SETTINGS_KEY]) || {};
  const merged = Object.assign({
    openDownloads: false,
    openFile: false,
    openFolder: false,
    videoQuality: '1080p',
    audioQuality: '320kbps',
    clipLength: 30,
    jobs: DEFAULT_JOBS,
    rememberClip: true,
    showStatus: true,
    showFloatingPill: false
  }, s);
  merged.jobs = normalizeJobs(s.jobs !== undefined ? s.jobs : s.jobsCount);
  return merged;
}

function sendToTab(tabId, message) {
  if (tabId === null || tabId === undefined) return;
  chrome.tabs.sendMessage(tabId, message).catch(() => {});
}

function isFirefoxBrowser() {
  return (typeof browser !== 'undefined' && typeof browser.runtime?.getBrowserInfo === 'function')
    || (typeof navigator !== 'undefined' && typeof navigator.userAgent === 'string' && navigator.userAgent.includes('Firefox'));
}

function openDownloadsPage() {
  const url = isFirefoxBrowser() ? 'about:downloads' : 'chrome://downloads/';
  try {
    Promise.resolve(chrome.tabs.create({ url })).catch(() => {
      try { chrome.downloads.showDefaultFolder(); } catch (_) {}
    });
  } catch (_) {
    try { chrome.downloads.showDefaultFolder(); } catch (_) {}
  }
}

function isYoutubeUrl(url) {
  if (typeof url !== 'string' || url.length > 2048) return false;
  return /^https:\/\/(?:www\.|m\.|music\.)?youtube\.com\/(?:watch\?(?:[^#]*&)?v=[\w-]{6,20}|shorts\/[\w-]{6,20}|live\/[\w-]{6,20}|embed\/[\w-]{6,20})(?:[?&#].*)?$/.test(url)
    || /^https:\/\/youtu\.be\/[\w-]{6,20}(?:[?&#].*)?$/.test(url);
}

function isFiniteNumber(v) {
  return typeof v === 'number' && Number.isFinite(v);
}

function normalizeClip(clip) {
  if (clip === undefined || clip === null) return null;
  if (typeof clip !== 'object') return false;
  const start = Number(clip.startSec);
  const end = Number(clip.endSec);
  if (!isFiniteNumber(start) || !isFiniteNumber(end)) return false;
  if (start < 0 || end <= start) return false;
  const out = {
    startSec: Math.round(start * 1000) / 1000,
    endSec: Math.round(end * 1000) / 1000
  };
  if (typeof clip.chapterTitle === 'string' && clip.chapterTitle.trim()) {
    out.chapterTitle = clip.chapterTitle.trim().slice(0, 100);
  }
  return out;
}

function normalizeMediaType(raw) {
  const found = MEDIA_TYPES.find(t => t.toLowerCase() === String(raw || '').toLowerCase());
  return found || 'Video';
}

function normalizeQuality(type, quality) {
  const q = String(quality || '').trim();
  if (type === 'Video') {
    if (/^itag-\d{1,6}(?:-[a-z0-9]{1,8})?(?:\|\d{3,4}p(?:60)?(?::(?:mp4|webm))?)?$/i.test(q)) {
      return q.toLowerCase();
    }
    return VIDEO_QUALITIES.includes(q.toLowerCase()) ? q.toLowerCase() : 'best';
  }
  if (type === 'MP3') {
    return AUDIO_QUALITIES.includes(q.toLowerCase()) ? q.toLowerCase() : '320kbps';
  }
  return 'original';
}

function stampShort(seconds) {
  const total = Math.floor(seconds);
  const pad = n => (n < 10 ? '0' : '') + n;
  return pad(Math.floor(total / 3600)) + '-' + pad(Math.floor((total % 3600) / 60)) + '-' + pad(total % 60);
}

function labelFor(type, quality, clip) {
  let base;
  if (type === 'MP3') {
    const q = quality === 'vbr' ? 'VBR' : quality.replace('kbps', 'k');
    base = 'MP3 ' + q;
  } else if (type === 'M4A' || type === 'OPUS' || type === 'WAV' || type === 'AAC') {
    base = type;
  } else {
    const q = quality === 'best' ? 'En iyi' : (quality.includes('|') ? quality.split('|')[1].split(':')[0] : quality);
    base = 'Video ' + q;
  }
  if (!clip) return base;
  const range = stampShort(clip.startSec) + '–' + stampShort(clip.endSec);
  if (clip.chapterTitle) {
    return clip.chapterTitle + ' · ' + base + ' (' + range + ')';
  }
  return base + ' ✂ ' + range;
}

function activeCount(jobs) {
  return Object.values(jobs).filter(j => j.state === 'running' || j.state === 'downloading').length;
}

function updateBadge(jobs) {
  const n = activeCount(jobs);
  chrome.action.setBadgeText({ text: n > 0 ? String(n) : '' }).catch(() => {});
  chrome.action.setBadgeBackgroundColor({ color: '#2f81f7' }).catch(() => {});
}

function publicJob(job) {
  return {
    jobId: job.jobId,
    label: job.label,
    type: job.type,
    quality: job.quality,
    jobs: normalizeJobs(job.jobs),
    clip: job.clip,
    state: job.state,
    percent: job.percent || 0,
    status: job.status || '',
    speed: job.speed || '',
    ok: job.ok,
    error: job.error || '',
    fileName: job.fileName || '',
    filePath: job.filePath || ''
  };
}

async function broadcast(job) {
  sendToTab(job.tabId, Object.assign({ action: 'download_progress' }, publicJob(job)));
  const jobs = await getJobs();
  updateBadge(jobs);
}

async function patchJob(jobId, patch) {
  return withLock(async () => {
    const jobs = await getJobs();
    const job = jobs[jobId];
    if (!job) return null;
    Object.assign(job, patch, { updatedAt: Date.now() });
    await persist(jobs);
    return job;
  });
}

function disposePort(jobId) {
  const port = nativePorts.get(jobId);
  if (!port) return;
  nativePorts.delete(jobId);
  try { port.disconnect(); } catch (_) {}
}

async function finishJob(jobId, patch) {
  const before = (await getJobs())[jobId];
  if (before && (before.state === 'done' || before.state === 'failed')) return;

  let job = await patchJob(jobId, Object.assign({ state: patch.ok === false ? 'failed' : 'done' }, patch));
  if (!job) return;

  if (job.ok !== false && job.downloadId !== undefined) {
    try {
      const items = await chrome.downloads.search({ id: job.downloadId });
      if (items && items[0] && items[0].filename) {
        job = (await patchJob(jobId, { filePath: items[0].filename })) || job;
      }
    } catch (_) {}
  }

  disposePort(jobId);
  await broadcast(job);
  await promoteQueued();
  if (job.ok !== false && job.filePath) await afterDownload(job);
}

async function afterDownload(job) {
  const settings = await getSettings();
  if (settings.openFile) {
    if (job.downloadId !== undefined) {
      try { await chrome.downloads.open(job.downloadId); } catch (_) {}
    } else if (job.filePath) {
      try { await shellNative('open_file', job.filePath); } catch (_) {}
    }
  }
  if (settings.openFolder) {
    if (job.downloadId !== undefined) {
      try { await chrome.downloads.show(job.downloadId); }
      catch (_) { try { chrome.downloads.showDefaultFolder(); } catch (_) {} }
    } else {
      try { await shellNative('open_folder', ''); } catch (_) {}
    }
  }
  if (settings.openDownloads) openDownloadsPage();
}

async function promoteQueued() {
  await withLock(async () => {
    const jobs = await getJobs();
    if (activeCount(jobs) >= MAX_ACTIVE) return;
    const next = Object.values(jobs)
      .filter(j => j.state === 'queued')
      .sort((a, b) => a.createdAt - b.createdAt)[0];
    if (!next) return;
    next.state = 'running';
    next.status = chrome.i18n.getMessage('statusPreparing') || 'Hazırlanıyor...';
    next.updatedAt = Date.now();
    await persist(jobs);
    setTimeout(() => startNativeJob(next), 0);
  });
}

function connectHost(jobId) {
  const port = chrome.runtime.connectNative(HOST);
  nativePorts.set(jobId, port);

  port.onMessage.addListener(msg => {
    if (!msg) return;
    onNativeEvent(Object.assign({ jobId }, msg)).catch(() => {});
  });

  port.onDisconnect.addListener(() => {
    const err = chrome.runtime.lastError;
    const detail = (err && err.message) || 'Native Host bağlantısı kesildi.';
    nativePorts.delete(jobId);
    getJobs().then(jobs => {
      const job = jobs[jobId];
      if (!job || job.state === 'done' || job.state === 'failed') return;
      finishJob(jobId, { ok: false, status: 'Bağlantı kesildi', error: detail });
    }).catch(() => {});
  });

  return port;
}

async function startNativeJob(job) {
  try {
    const settings = await getSettings();
    const rawJobs = (job.jobs !== undefined && job.jobs !== null) ? job.jobs : settings.jobs;
    const jobs = normalizeJobs(rawJobs);
    const port = connectHost(job.jobId);
    port.postMessage({
      action: 'download',
      jobId: job.jobId,
      type: job.type,
      quality: job.quality,
      url: job.url,
      jobs,
      clip: job.clip || null,
      channel: job.channel || '',
      title: job.title || '',
      audioLang: job.audioLang || ''
    });
  } catch (e) {
    await finishJob(job.jobId, {
      ok: false,
      state: 'failed',
      status: 'Hata',
      error: (e && (e.stack || e.message)) || 'Native Host başlatılamadı.'
    });
  }
}

function probeNative(url) {
  return new Promise((resolve, reject) => {
    const jobId = 'probe-' + Date.now().toString(36) + '-' + Math.random().toString(36).slice(2, 8);
    let port;
    try { port = chrome.runtime.connectNative(HOST); }
    catch (e) { reject(new Error((e && e.message) || 'Native Host başlatılamadı.')); return; }

    let settled = false;
    const timer = setTimeout(() => {
      finish(false, { ok: false, error: 'Akış çözümleme zaman aşımı.' });
    }, 20000);

    const finish = (ok, data) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try { port.disconnect(); } catch (_) {}
      if (ok) resolve(data);
      else reject(new Error((data && data.error) || 'Çözümleme başarısız.'));
    };

    port.onMessage.addListener(msg => {
      if (!msg) return;
      if (msg.ok) finish(true, msg);
      else finish(false, msg);
    });

    port.onDisconnect.addListener(() => {
      const err = chrome.runtime.lastError;
      finish(false, { error: (err && err.message) || 'Native Host kapandı.' });
    });

    try {
      port.postMessage({ action: 'probe', jobId, url });
    } catch (e) {
      finish(false, { error: (e && e.message) || 'Probe isteği gönderilemedi.' });
    }
  });
}

const downloadIndex = new Map();

async function startChromeDownload(job, fileUrl, fileName) {
  const safeName = String(fileName || 'ruvitube').replace(/[\\/:*?"<>|]+/g, '_').slice(0, 180);
  const id = await chrome.downloads.download({
    url: fileUrl,
    filename: 'RuviTube/' + safeName,
    conflictAction: 'uniquify',
    saveAs: false
  });
  downloadIndex.set(id, job.jobId);
  const updated = await patchJob(job.jobId, {
    downloadId: id,
    fileName: safeName,
    state: 'downloading',
    percent: 99,
    status: chrome.i18n.getMessage('statusHandoff') || "Chrome İndirilenler'e aktarılıyor..."
  });
  if (updated) broadcast(updated);
}

chrome.downloads.onChanged.addListener(delta => {
  if (!delta || delta.id === undefined || !delta.state) return;
  const state = delta.state.current;
  if (state !== 'complete' && state !== 'interrupted') return;
  const jobId = downloadIndex.get(delta.id);
  if (!jobId) return;
  downloadIndex.delete(delta.id);
  if (state === 'complete') {
    finishJob(jobId, { ok: true, percent: 100, status: 'Tamamlandı' });
  } else {
    const reason = (delta.error && delta.error.current) || 'indirme kesildi';
    finishJob(jobId, { ok: false, percent: 0, status: 'Hata', error: 'Chrome indirme hatası: ' + reason });
  }
});

async function onNativeEvent(msg) {
  const jobId = msg.jobId;
  if (!jobId) return;
  const jobs = await getJobs();
  const job = jobs[jobId];
  if (!job) return;

  if (msg.ready && msg.fileUrl && msg.fileName) {
    try {
      await startChromeDownload(job, msg.fileUrl, msg.fileName);
    } catch (e) {
      await finishJob(jobId, { ok: false, status: 'Hata', error: (e && e.message) || 'Chrome indirmesi başlatılamadı.' });
    }
    return;
  }

  if (msg.done) {
    await finishJob(jobId, {
      ok: msg.ok !== false,
      percent: msg.ok === false ? 0 : (typeof msg.percent === 'number' ? msg.percent : (job.percent || 100)),
      status: msg.status || (msg.ok === false ? 'Hata' : 'Tamamlandı'),
      error: msg.error || '',
      fileName: msg.fileName || job.fileName || '',
      filePath: msg.filePath || job.filePath || ''
    });
    return;
  }

  if (msg.percent !== undefined || msg.status) {
    const updated = await patchJob(jobId, {
      percent: typeof msg.percent === 'number' ? msg.percent : job.percent,
      status: msg.status || job.status,
      speed: msg.speed || '',
      eta: msg.eta || ''
    });
    if (updated) broadcast(updated);
  }
}

function shellNative(op, filePath) {
  return new Promise((resolve, reject) => {
    let port;
    try { port = chrome.runtime.connectNative(HOST); }
    catch (e) { reject(new Error((e && e.message) || 'Native Host bağlantısı kurulamadı.')); return; }
    let settled = false;
    const done = (ok, error) => {
      if (settled) return;
      settled = true;
      try { port.disconnect(); } catch (_) {}
      ok ? resolve() : reject(new Error(error || 'İşlem başarısız.'));
    };
    port.onMessage.addListener(msg => {
      if (msg && msg.ok === false) done(false, msg.error);
      else done(true);
    });
    port.onDisconnect.addListener(() => {
      const err = chrome.runtime.lastError;
      done(false, (err && err.message) || 'Native Host bağlantısı kesildi.');
    });
    try { port.postMessage({ action: op, filePath: filePath || '' }); }
    catch (e) { done(false, (e && e.message) || 'Mesaj gönderilemedi.'); }
  });
}

function checkNativeHost() {
  return new Promise(resolve => {
    let settled = false;
    let port = null;
    const finish = res => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try { if (port) port.disconnect(); } catch (_) {}
      resolve(res);
    };
    const timer = setTimeout(() => finish({ ok: false, error: 'Native Host yanıt vermedi.' }), 2500);
    try {
      port = chrome.runtime.connectNative(HOST);
    } catch (e) {
      finish({ ok: false, error: (e && e.message) || 'Native Host başlatılamadı.' });
      return;
    }
    port.onMessage.addListener(() => {
      finish({ ok: true });
    });
    port.onDisconnect.addListener(() => {
      const err = chrome.runtime.lastError;
      finish({ ok: false, error: (err && err.message) || 'Native Host kurulu değil.' });
    });
    try {
      port.postMessage({ action: 'ping' });
    } catch (e) {
      finish({ ok: false, error: (e && e.message) || 'Native Host bağlantısı kurulamadı.' });
    }
  });
}

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (!message) return;

  if (message.target === 'getube-bg' || message.target === 'ruvitube-bg') {
    if (message.action === 'keepalive') { sendResponse({ ok: true }); return; }
    onNativeEvent(message).catch(() => {});
    sendResponse({ ok: true });
    return;
  }
  if (message.target) return;

  if (message.action === 'check_native_host') {
    checkNativeHost()
      .then(res => sendResponse(res))
      .catch(e => sendResponse({ ok: false, error: (e && e.message) || 'Native Host kontrolü başarısız.' }));
    return true;
  }

  if (message.action === 'download_all_in_one') {
    chrome.downloads.download({
      url: ALL_IN_ONE_URL,
      filename: 'RuviTube-AllInOne.cmd',
      saveAs: false
    }, downloadId => {
      const err = chrome.runtime.lastError;
      if (err) {
        sendResponse({ ok: false, error: err.message });
      } else {
        sendResponse({ ok: true, downloadId });
      }
    });
    return true;
  }

  if (message.action === 'open_downloads') {
    openDownloadsPage();
    sendResponse({ ok: true });
    return;
  }

  if (message.action === 'open_file' || message.action === 'open_folder') {
    const op = message.action;
    const filePath = String(message.filePath || '');
    shellNative(op, filePath)
      .then(() => sendResponse({ ok: true }))
      .catch(e => sendResponse({ ok: false, error: (e && e.message) || 'İşlem başarısız.' }));
    return true;
  }

  if (message.action === 'probe_video') {
    const url = String(message.url || '');
    if (!isYoutubeUrl(url)) {
      sendResponse({ ok: false, error: 'Geçerli bir YouTube adresi değil.' });
      return;
    }
    probeNative(url)
      .then(data => sendResponse({ ok: true, data }))
      .catch(e => sendResponse({ ok: false, error: (e && e.message) || 'Çözümleme hatası.' }));
    return true;
  }

  if (message.action === 'get_jobs') {
    getJobs().then(async jobs => {
      const now = Date.now();
      for (const job of Object.values(jobs)) {
        const stale = (job.state === 'running' || job.state === 'downloading' || job.state === 'queued')
          && job.updatedAt && now - job.updatedAt > 40 * 60 * 1000;
        if (!stale) continue;
        job.state = 'failed';
        job.ok = false;
        job.status = 'Zaman aşımı';
        job.error = 'İş 40 dakikadır güncellenmedi.';
        job.updatedAt = now;
      }
      await persist(jobs).catch(() => {});
      sendResponse({
        ok: true,
        jobs: Object.values(jobs)
          .sort((a, b) => b.createdAt - a.createdAt)
          .slice(0, 10)
          .map(publicJob)
      });
    });
    return true;
  }

  if (message.action === 'cancel_job') {
    const jobId = String(message.jobId || '');
    disposePort(jobId);
    finishJob(jobId, { ok: false, status: 'İptal edildi', error: 'Kullanıcı iptal etti.' })
      .then(() => sendResponse({ ok: true }))
      .catch(() => sendResponse({ ok: false }));
    return true;
  }

  if (message.action !== 'download' && message.action !== 'start_download') return;

  const type = normalizeMediaType(message.type);
  const quality = normalizeQuality(type, String(message.quality || ''));
  const url = String(message.url || '');
  const clip = normalizeClip(message.clip);
  const channel = typeof message.channel === 'string' ? message.channel.trim().slice(0, 120) : '';
  const title = typeof message.title === 'string' ? message.title.trim().slice(0, 200) : '';
  const audioLang = typeof message.audioLang === 'string' ? message.audioLang.trim().slice(0, 24) : '';

  if (!isYoutubeUrl(url)) {
    sendResponse({ ok: false, error: 'Geçerli bir YouTube video adresi bulunamadı.' });
    return;
  }
  if (clip === false) {
    sendResponse({ ok: false, error: 'Geçersiz zaman aralığı.' });
    return;
  }

  const jobId = (self.crypto && crypto.randomUUID) ? crypto.randomUUID()
    : Date.now().toString(36) + '-' + Math.random().toString(36).slice(2, 10);
  const tabId = sender && sender.tab ? sender.tab.id : null;

  (async () => {
    const settings = await getSettings();
    const rawJobs = (message.jobs !== undefined && message.jobs !== null)
      ? message.jobs
      : (message.jobsCount !== undefined && message.jobsCount !== null ? message.jobsCount : settings.jobs);
    const jobsCount = normalizeJobs(rawJobs);
    const job = {
      jobId,
      tabId,
      type,
      quality,
      jobs: jobsCount,
      url,
      clip,
      channel,
      title,
      audioLang,
      label: message.label ? String(message.label).slice(0, 80) : labelFor(type, quality, clip),
      state: 'queued',
      percent: 0,
      status: 'Hazırlanıyor...',
      createdAt: Date.now(),
      updatedAt: Date.now()
    };

    const canStartNow = await withLock(async () => {
      const jobs = await getJobs();
      jobs[jobId] = job;
      await persist(jobs);
      updateBadge(jobs);
      return activeCount(jobs) <= MAX_ACTIVE;
    });
    sendResponse({ ok: true, jobId, label: job.label, jobs: jobsCount });
    if (canStartNow) promoteQueued();
  })().catch(e => sendResponse({ ok: false, error: (e && e.message) || 'İş oluşturulamadı.' }));

  return true;
});

function refreshToolbarActionIcon() {
  if (!chrome.action || typeof chrome.action.setIcon !== 'function') return;
  try {
    chrome.action.setIcon({
      path: {
        16: 'icon16.png',
        32: 'icon32.png',
        48: 'icon48.png',
        128: 'icon128.png'
      }
    }).catch(() => {});
  } catch (_) {}
}

refreshToolbarActionIcon();

if (chrome.runtime && chrome.runtime.onStartup) {
  chrome.runtime.onStartup.addListener(() => {
    refreshToolbarActionIcon();
  });
}

chrome.runtime.onInstalled.addListener(details => {
  refreshToolbarActionIcon();
  if (details && details.reason === 'install') {
    checkNativeHost().then(r => {
      if (!r || !r.ok) {
        chrome.tabs.create({ url: chrome.runtime.getURL('options.html?setup=1') }).catch(() => {});
      }
    }).catch(() => {});
  }
  withLock(async () => {
    const jobs = await getJobs();
    for (const job of Object.values(jobs)) {
      if (job.state === 'running' || job.state === 'downloading') {
        job.state = 'failed';
        job.ok = false;
        job.status = 'Kesildi';
        job.error = 'Uzantı yeniden başlatıldı.';
        job.updatedAt = Date.now();
      }
    }
    await persist(jobs);
  }).catch(() => {});
});
