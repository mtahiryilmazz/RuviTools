/* RuviTube — popup: son işler, canlı yüzde ve kısayollar. */
'use strict';

const TR = {
  popupLast: 'Son işler',
  popupIdle: 'Bekleyen indirme yok',
  popupJobs: 'İndirme Hızı (Paralel)',
  popupSettings: 'Ayarlar',
  popupFolder: 'Klasörü Aç',
  popupOpenYoutube: "YouTube'da videonun sağ üstündeki RuviTube menüsünden kalite veya MP3 seçin",
  statusDone: 'Tamamlandı',
  statusError: 'Hata'
};

const SETTINGS_KEY = 'ruvitube.settings';
const VALID_JOBS = [1, 2, 4, 8, 16];
function normalizeJobs(raw) {
  const n = Math.round(Number(raw));
  if (!Number.isFinite(n)) return 4;
  if (VALID_JOBS.includes(n)) return n;
  return Math.min(16, Math.max(1, n));
}

const $ = id => document.getElementById(id);

function applyI18n() {
  document.querySelectorAll('[data-i18n]').forEach(node => {
    const message = TR[node.dataset.i18n];
    if (message) node.textContent = message;
  });
  document.documentElement.lang = 'tr';
  $('version').textContent = 'v' + chrome.runtime.getManifest().version;
}

function renderJob(job) {
  const wrap = document.createElement('div');
  wrap.className = 'job ' + (job.state || '');

  const top = document.createElement('div');
  top.className = 'top';

  const name = document.createElement('span');
  name.textContent = job.label || job.type || 'RuviTube';

  const status = document.createElement('em');
  if (job.state === 'done') status.textContent = TR.statusDone;
  else if (job.state === 'failed') status.textContent = TR.statusError;
  else status.textContent = Math.round(Number(job.percent) || 0) + '%';

  top.append(name, status);

  const sub = document.createElement('div');
  sub.className = 'sub';
  const detail = document.createElement('span');
  detail.textContent = job.state === 'done'
    ? (job.fileName || job.status || '')
    : (job.state === 'failed' ? (job.error || job.status || '') : (job.status || ''));
  sub.appendChild(detail);

  if (job.state === 'running' || job.state === 'downloading' || job.state === 'queued') {
    const cancelBtn = document.createElement('button');
    cancelBtn.type = 'button';
    cancelBtn.textContent = 'İptal';
    cancelBtn.addEventListener('click', async () => {
      await chrome.runtime.sendMessage({ action: 'cancel_job', jobId: job.jobId }).catch(() => {});
      loadJobs();
    });
    sub.appendChild(cancelBtn);
  } else if (job.state === 'done' && job.filePath) {
    const openBtn = document.createElement('button');
    openBtn.type = 'button';
    openBtn.textContent = 'Aç';
    openBtn.addEventListener('click', () => {
      chrome.runtime.sendMessage({ action: 'open_file', filePath: job.filePath }).catch(() => {});
    });
    sub.appendChild(openBtn);
  }

  const bar = document.createElement('div');
  bar.className = 'bar';
  const fill = document.createElement('span');
  const pct = job.state === 'done' ? 100 : Math.max(0, Math.min(100, Number(job.percent) || 0));
  fill.style.width = pct + '%';
  bar.appendChild(fill);

  wrap.append(top, sub, bar);
  wrap.title = job.status || '';
  return wrap;
}

async function loadJobs() {
  const container = $('jobs');
  let jobs = [];
  try {
    const response = await chrome.runtime.sendMessage({ action: 'get_jobs' });
    if (response && response.ok) jobs = response.jobs || [];
  } catch (_) {}

  const visible = jobs.slice(0, 4);
  container.textContent = '';
  if (!visible.length) {
    const empty = document.createElement('div');
    empty.className = 'empty';
    empty.textContent = TR.popupIdle;
    container.appendChild(empty);
    return;
  }
  visible.forEach(job => container.appendChild(renderJob(job)));
}

async function loadJobsSetting() {
  try {
    const stored = await chrome.storage.local.get(SETTINGS_KEY);
    const s = (stored && stored[SETTINGS_KEY]) || {};
    const j = normalizeJobs(s.jobs !== undefined ? s.jobs : s.jobsCount);
    $('jobsSelect').value = String(VALID_JOBS.includes(j) ? j : 4);
  } catch (_) {
    $('jobsSelect').value = '4';
  }
}

$('jobsSelect').addEventListener('change', async () => {
  const jobs = normalizeJobs($('jobsSelect').value);
  try {
    const stored = await chrome.storage.local.get(SETTINGS_KEY);
    const current = (stored && stored[SETTINGS_KEY]) || {};
    await chrome.storage.local.set({ [SETTINGS_KEY]: Object.assign({}, current, { jobs }) });
  } catch (_) {}
});

chrome.storage.onChanged.addListener((changes, area) => {
  if (area !== 'local' || !changes[SETTINGS_KEY]) return;
  const s = changes[SETTINGS_KEY].newValue || {};
  const j = normalizeJobs(s.jobs !== undefined ? s.jobs : s.jobsCount);
  $('jobsSelect').value = String(VALID_JOBS.includes(j) ? j : 4);
});

async function checkHostStatus() {
  const banner = $('setupBanner');
  if (!banner) return;
  try {
    const res = await chrome.runtime.sendMessage({ action: 'check_native_host' });
    banner.hidden = Boolean(res && res.ok);
  } catch (_) {
    banner.hidden = false;
  }
}

if ($('btnInstallHost')) {
  $('btnInstallHost').addEventListener('click', async () => {
    const btn = $('btnInstallHost');
    btn.disabled = true;
    try {
      const res = await chrome.runtime.sendMessage({ action: 'download_all_in_one' });
      btn.textContent = res && res.ok ? '✓ İndirildi — dosyayı açın' : 'Hata — tekrar deneyin';
    } catch (_) {
      btn.textContent = 'Hata — tekrar deneyin';
    }
    setTimeout(() => {
      btn.disabled = false;
      btn.textContent = '⬇ All-in-One Kurulumu İndir';
    }, 4000);
  });
}

if ($('btnRecheckHost')) {
  $('btnRecheckHost').addEventListener('click', () => {
    checkHostStatus();
  });
}

applyI18n();
checkHostStatus();
loadJobs();
loadJobsSetting();
initActiveMediaCard();
setInterval(loadJobs, 1000);

function normalizeYouTubeWatchUrl(rawUrl) {
  try {
    const u = new URL(rawUrl);
    const host = u.hostname.replace(/^(www|m|music)\./i, '').toLowerCase();
    if (host === 'youtu.be') {
      const id = u.pathname.split('/')[1];
      return id ? `https://www.youtube.com/watch?v=${encodeURIComponent(id)}` : null;
    }
    if (host === 'youtube.com' || host === 'youtube-nocookie.com') {
      const v = u.searchParams.get('v');
      if (u.pathname.startsWith('/watch') && v) {
        return `https://www.youtube.com/watch?v=${encodeURIComponent(v)}`;
      }
      const m = /^\/(shorts|live|embed)\/([\w-]+)/.exec(u.pathname);
      if (m) return `https://www.youtube.com/watch?v=${encodeURIComponent(m[2])}`;
    }
  } catch (_) {}
  return null;
}

async function initActiveMediaCard() {
  const card = $('activeMedia');
  const titleEl = $('activeTitle');
  if (!card || !titleEl) return;
  try {
    const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
    const tab = tabs && tabs[0];
    const ytUrl = tab && tab.url ? normalizeYouTubeWatchUrl(tab.url) : null;
    if (!ytUrl) return;
    const cleanTitle = String(tab.title || 'YouTube Video')
      .replace(/^\(\d+\)\s+/, '')
      .replace(/\s+[-–|]\s*YouTube$/i, '')
      .trim();
    titleEl.textContent = cleanTitle || ytUrl;
    card.hidden = false;

    card.querySelectorAll('button[data-type]').forEach(btn => {
      btn.addEventListener('click', async () => {
        const type = btn.dataset.type || 'MP3';
        const quality = btn.dataset.quality || '320kbps';
        const jobs = normalizeJobs($('jobsSelect').value);
        btn.disabled = true;
        try {
          await chrome.runtime.sendMessage({
            action: 'start_download',
            url: ytUrl,
            type,
            quality,
            jobs,
            label: `${type} · ${quality} (${cleanTitle.slice(0, 36)})`
          });
          await loadJobs();
        } catch (_) {}
        setTimeout(() => { btn.disabled = false; }, 600);
      });
    });
  } catch (_) {}
}

$('options').addEventListener('click', () => {
  chrome.runtime.openOptionsPage();
  window.close();
});

$('openFolder').addEventListener('click', () => {
  chrome.runtime.sendMessage({ action: 'open_folder' }).catch(() => {});
});

async function initUpdateBanner() {
  try {
    const stored = await chrome.storage.local.get(['ruvitube.updateInfo', 'ruvitube.updateInstallError']);
    const updateInfo = stored && stored['ruvitube.updateInfo'];
    if (updateInfo && updateInfo.version) {
      const banner = $('updateBanner');
      const versionText = $('updateVersionText');
      const instructions = $('updateInstructions');
      const btn = $('btnDownloadUpdate');
      if (banner && versionText) {
        versionText.textContent = 'v' + updateInfo.version;
        banner.hidden = false;
        if (instructions && stored['ruvitube.updateInstallError']) {
          instructions.textContent = 'Otomatik kurulum tamamlanamadı: ' + stored['ruvitube.updateInstallError'] + ' All-in-One kurulumunu bir kez çalıştırıp NativeHost güncelleyicisini kurun.';
        } else if (instructions) {
          instructions.textContent = 'Yeni sürüm bulundu. RuviTube güncellemeyi NativeHost ile kurup eklentiyi yeniden yükleyecek.';
        }
        if (btn) {
          if (updateInfo.sourceOnly) btn.textContent = '⬇ GitHub kaynak arşivini indir';
          btn.addEventListener('click', () => {
            chrome.tabs.create({ url: updateInfo.downloadUrl || 'https://github.com/mtahiryilmazz/RuviTools/releases/latest' });
          });
        }
      }
    }
  } catch (_) {}
}
initUpdateBanner();

