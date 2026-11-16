/*
 * GeTube — service worker.
 *
 * Sorumluluklar:
 *  - Content script isteklerini doğrulamak (url, kalite, kesit aralığı).
 *  - Native Host portunu YÖNETMEK. Not: chrome.runtime.connectNative yalnızca
 *    service worker'da tutulur (v1.0.8 ile aynı yöntem).
 *  - chrome.downloads çağrılarını yapmak.
 *  - İş kayıtlarını chrome.storage.session üzerinde tutmak.
 *
 * MV3 notu: service worker 30 sn boşta sonlandırılabilir; v1.0.8'deki gibi port SW'de
 * tutulur, indirme sürerken yt-dlp çıktısı portu canlı tutar.
 */
'use strict';

const HOST = 'com.ruvitube.nativehost';
const JOBS_KEY = 'ruvitube.jobs';
const SETTINGS_KEY = 'ruvitube.settings';
const MAX_ACTIVE = 2;
const JOB_TTL_MS = 6 * 60 * 60 * 1000;

// Tanı: çalışan service worker sürümü (destek için).
chrome.storage.session.set({ 'ruvitube.bgVersion': 'sw-2.0.0-ruvi' }).catch(() => {});

const VIDEO_QUALITIES = ['1080p', '720p', '480p', 'best'];
const AUDIO_QUALITIES = ['320kbps', '192kbps'];

let jobsCache = null;
let lock = Promise.resolve();
const nativePorts = new Map(); // jobId -> port

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

async function getSettings() {
  const stored = await chrome.storage.local.get(SETTINGS_KEY);
  const s = (stored && stored[SETTINGS_KEY]) || {};
  return Object.assign({
    openDownloads: false,
    openFile: false,
    openFolder: false,
    videoQuality: '720p',
    audioQuality: '320kbps',
    clipLength: 30,
    rememberClip: true,
    showStatus: true
  }, s);
}

/* ---------------------------------------------------------------- helpers */

function sendToTab(tabId, message) {
  if (tabId === null || tabId === undefined) return;
  chrome.tabs.sendMessage(tabId, message).catch(() => {});
}

function isYoutubeUrl(url) {
  return /^https:\/\/www\.youtube\.com\/watch\?(?:[^#]*&)?v=[\w-]{6,20}(?:[&#].*)?$/.test(url || '');
}

function isFiniteNumber(v) {
  return typeof v === 'number' && isFinite(v);
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

function normalizeQuality(type, quality) {
  const list = type === 'MP3' ? AUDIO_QUALITIES : VIDEO_QUALITIES;
  return list.indexOf(quality) >= 0 ? quality : list[list.length - 1];
}

function stampShort(seconds) {
  const total = Math.floor(seconds);
  const pad = n => (n < 10 ? '0' : '') + n;
  return pad(Math.floor(total / 3600)) + '-' + pad(Math.floor((total % 3600) / 60)) + '-' + pad(total % 60);
}

function labelFor(type, quality, clip) {
  const q = type === 'MP3'
    ? quality.replace('kbps', 'k')
    : (quality === 'best' ? 'En iyi' : quality);
  const base = (type === 'MP3' ? 'MP3 ' : 'MP4 ') + q;
  if (!clip) return base;
  return base + ' ✂ ' + stampShort(clip.startSec) + '–' + stampShort(clip.endSec);
}

function activeCount(jobs) {
  return Object.values(jobs).filter(j => j.state === 'running' || j.state === 'downloading').length;
}

function updateBadge(jobs) {
  const n = activeCount(jobs);
  chrome.action.setBadgeText({ text: n > 0 ? String(n) : '' }).catch(() => {});
  chrome.action.setBadgeBackgroundColor({ color: '#cc0000' }).catch(() => {});
}

/* ------------------------------------------------------------------ jobs */

function publicJob(job) {
  return {
    jobId: job.jobId,
    label: job.label,
    type: job.type,
    quality: job.quality,
    clip: job.clip,
    state: job.state,
    percent: job.percent || 0,
    status: job.status || '',
    ok: job.ok,
    error: job.error || '',
    fileName: job.fileName || ''
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
  try { port.disconnect(); } catch (_) { /* yoksay */ }
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
    } catch (_) { /* yol alınamazsa dosyayı açma atlanır */ }
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
      try { await chrome.downloads.open(job.downloadId); } catch (_) { /* yoksay */ }
    } else if (job.filePath) {
      try { await shellNative('open_file', job.filePath); } catch (_) { /* yoksay */ }
    }
  }
  if (settings.openFolder) {
    if (job.downloadId !== undefined) {
      try { await chrome.downloads.show(job.downloadId); }
      catch (_) { try { chrome.downloads.showDefaultFolder(); } catch (_) { /* yoksay */ } }
    } else {
      try { await shellNative('open_folder', ''); } catch (_) { /* yoksay */ }
    }
  }
  if (settings.openDownloads) chrome.tabs.create({ url: 'chrome://downloads/' }).catch(() => {});
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

/* -------------------------------------------------------- native messaging */

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
    const port = connectHost(job.jobId);
    port.postMessage({
      action: 'download',
      jobId: job.jobId,
      type: job.type,
      quality: job.quality,
      url: job.url,
      clip: job.clip || null
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

/* ------------------------------------------------------------- downloads */

const downloadIndex = new Map(); // downloadId -> jobId

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

/* --------------------------------------------------------- native events */

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

/* ------------------------------------------------------- tek seferlik shell */

function shellNative(op, filePath) {
  return new Promise((resolve, reject) => {
    let port;
    try { port = chrome.runtime.connectNative(HOST); }
    catch (e) { reject(new Error((e && e.message) || 'Native Host bağlantısı kurulamadı.')); return; }
    let settled = false;
    const done = (ok, error) => {
      if (settled) return;
      settled = true;
      try { port.disconnect(); } catch (_) { /* yoksay */ }
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

/* --------------------------------------------------------------- signals */

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (!message) return;

  if (message.target === 'getube-bg') {
    if (message.action === 'keepalive') { sendResponse({ ok: true }); return; }
    onNativeEvent(message).catch(() => {});
    sendResponse({ ok: true });
    return;
  }
  if (message.target) return;

  if (message.action === 'open_downloads') {
    chrome.tabs.create({ url: 'chrome://downloads/' });
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

  if (message.action !== 'download') return;

  const type = message.type === 'MP3' ? 'MP3' : 'Video';
  const quality = normalizeQuality(type, String(message.quality || ''));
  const url = String(message.url || '');
  const clip = normalizeClip(message.clip);

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
  const job = {
    jobId,
    tabId,
    type,
    quality,
    url,
    clip,
    label: labelFor(type, quality, clip),
    state: 'queued',
    percent: 0,
    status: 'Hazırlanıyor...',
    createdAt: Date.now(),
    updatedAt: Date.now()
  };

  withLock(async () => {
    const jobs = await getJobs();
    jobs[jobId] = job;
    await persist(jobs);
    updateBadge(jobs);
    return activeCount(jobs) <= MAX_ACTIVE;
  }).then(canStartNow => {
    sendResponse({ ok: true, jobId, label: job.label });
    if (canStartNow) promoteQueued();
  }).catch(e => sendResponse({ ok: false, error: (e && e.message) || 'İş oluşturulamadı.' }));

  return true;
});

chrome.runtime.onInstalled.addListener(() => {
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
