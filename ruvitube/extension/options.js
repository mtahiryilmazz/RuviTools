/* RuviTube — ayarlar sayfası. Tek doğruluk kaynağı chrome.storage.local: 'ruvitube.settings'. */
'use strict';

const TR = {
  optTitle: 'RuviTube ayarları',
  extName: 'RuviTube',
  optDefaults: 'Varsayılanlar',
  optVideoQuality: 'Varsayılan görüntü kalitesi',
  optAudioQuality: 'Varsayılan MP3 ses kalitesi',
  optClipLength: 'Varsayılan kesit süresi (saniye)',
  optJobs: 'Paralel Range Parçaları (--jobs)',
  optJobsHint: 'Aynı anda indirilecek HTTP Range parça sayısı',
  optAfterDownload: 'İndirme sonrası',
  optOpenDownloads: 'Chrome İndirilenler sayfasını aç',
  optOpenFile: 'İndirilen dosyayı aç',
  optOpenFolder: 'İndirme klasörünü aç',
  optBehavior: 'Davranış',
  optFloatingPill: 'Video oynatıcı üzerinde kalite & MP3 menüsü',
  optShowStatus: 'Düğme üzerinde yüzde göster',
  optRememberClip: 'Son kullanılan kesit süresini hatırla',
  optSaved: 'Kaydedildi',
  optReset: 'Varsayılana dön'
};

const SETTINGS_KEY = 'ruvitube.settings';
const DEFAULTS = {
  openDownloads: false,
  openFile: false,
  openFolder: false,
  videoQuality: '1080p',
  audioQuality: '320kbps',
  clipLength: 30,
  jobs: 4,
  rememberClip: true,
  showStatus: true,
  showFloatingPill: false
};

const VALID_JOBS = [1, 2, 4, 8, 16];
function normalizeJobs(raw) {
  const n = Math.round(Number(raw));
  if (!Number.isFinite(n)) return DEFAULTS.jobs;
  if (VALID_JOBS.includes(n)) return n;
  return Math.min(16, Math.max(1, n));
}

const CHECKS = ['openDownloads', 'openFile', 'openFolder', 'rememberClip', 'showStatus', 'showFloatingPill'];
const SELECTS = ['videoQuality', 'audioQuality'];
const $ = id => document.getElementById(id);

function applyI18n() {
  document.querySelectorAll('[data-i18n]').forEach(node => {
    const message = TR[node.dataset.i18n];
    if (message) node.textContent = message;
  });
  document.documentElement.lang = 'tr';
  $('version').textContent = chrome.runtime.getManifest().version;
}

function readForm() {
  const value = {};
  CHECKS.forEach(id => { value[id] = $(id).checked; });
  SELECTS.forEach(id => { value[id] = $(id).value; });
  value.jobs = normalizeJobs($('jobs').value);
  const clip = Number($('clipLength').value);
  value.clipLength = Number.isFinite(clip) ? Math.max(1, Math.round(clip)) : DEFAULTS.clipLength;
  return value;
}

function render(settings) {
  CHECKS.forEach(id => { $(id).checked = !!settings[id]; });
  SELECTS.forEach(id => {
    const el = $(id);
    const val = settings[id];
    const hasOption = Array.from(el.options).some(o => o.value === val);
    el.value = hasOption ? val : DEFAULTS[id];
  });
  const normalizedJobs = normalizeJobs(settings.jobs);
  $('jobs').value = String(VALID_JOBS.includes(normalizedJobs) ? normalizedJobs : DEFAULTS.jobs);
  $('clipLength').value = String(settings.clipLength);
}

let saveTimer = 0;
function scheduleSave() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    const settings = readForm();
    await chrome.storage.local.set({ [SETTINGS_KEY]: settings });
    render(settings);
    const badge = $('saved');
    badge.classList.add('show');
    setTimeout(() => badge.classList.remove('show'), 1200);
  }, 180);
}

async function load() {
  const stored = await chrome.storage.local.get(SETTINGS_KEY);
  const raw = (stored && stored[SETTINGS_KEY]) || {};
  const merged = Object.assign({}, DEFAULTS, raw);
  if (raw.jobs === undefined && raw.jobsCount !== undefined) {
    merged.jobs = normalizeJobs(raw.jobsCount);
  }
  render(merged);
}

async function checkHostStatus() {
  const banner = $('setupBanner');
  const statusEl = $('setupStatus');
  if (!banner) return;
  try {
    const res = await chrome.runtime.sendMessage({ action: 'check_native_host' });
    const ok = Boolean(res && res.ok);
    banner.hidden = ok;
    if (!ok && statusEl) {
      statusEl.textContent = (res && res.error) ? res.error : '';
    }
  } catch (_) {
    banner.hidden = false;
  }
}

if ($('btnInstallHost')) {
  $('btnInstallHost').addEventListener('click', async () => {
    const btn = $('btnInstallHost');
    const statusEl = $('setupStatus');
    btn.disabled = true;
    try {
      const res = await chrome.runtime.sendMessage({ action: 'download_all_in_one' });
      if (res && res.ok) {
        btn.textContent = '✓ İndirildi — dosyayı açın';
        if (statusEl) statusEl.textContent = 'İndirilenler klasöründen RuviTube-AllInOne.cmd dosyasını çalıştırıp ardından Yenile butonuna basın.';
      } else {
        btn.textContent = 'Hata — tekrar deneyin';
        if (statusEl && res && res.error) statusEl.textContent = res.error;
      }
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
    const statusEl = $('setupStatus');
    if (statusEl) statusEl.textContent = 'Kontrol ediliyor...';
    checkHostStatus();
  });
}

applyI18n();
load();
checkHostStatus();

CHECKS.concat(SELECTS, ['jobs', 'clipLength']).forEach(id => {
  $(id).addEventListener('change', scheduleSave);
});

$('reset').addEventListener('click', async () => {
  await chrome.storage.local.set({ [SETTINGS_KEY]: Object.assign({}, DEFAULTS) });
  render(DEFAULTS);
});
