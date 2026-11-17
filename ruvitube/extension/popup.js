/* GeTube — popup: son işler ve kısayollar. */
'use strict';

// Arayüz Türkçe: tarayıcı dilinden bağımsız sabit metinler.
const TR = {
  popupLast: 'Son işler',
  popupIdle: 'Bekleyen indirme yok',
  popupJobs: 'Paralel Range Parçaları (--jobs)',
  popupSettings: 'Ayarlar',
  popupDownloads: 'İndirilenler',
  popupOpenYoutube: "YouTube'da bir video açın",
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
  name.textContent = job.label || job.type || 'GeTube';

  const status = document.createElement('em');
  if (job.state === 'done') status.textContent = job.fileName || (TR.statusDone || 'Tamamlandı');
  else if (job.state === 'failed') status.textContent = job.error || (TR.statusError || 'Hata');
  else status.textContent = Math.round(Number(job.percent) || 0) + '%';

  top.append(name, status);

  const bar = document.createElement('div');
  bar.className = 'bar';
  const fill = document.createElement('span');
  const pct = job.state === 'done' ? 100 : Math.max(0, Math.min(100, Number(job.percent) || 0));
  fill.style.width = pct + '%';
  bar.appendChild(fill);

  wrap.append(top, bar);
  wrap.title = job.status || '';
  return wrap;
}

async function loadJobs() {
  const container = $('jobs');
  let jobs = [];
  try {
    const response = await chrome.runtime.sendMessage({ action: 'get_jobs' });
    if (response && response.ok) jobs = response.jobs || [];
  } catch (_) { /* service worker uyanmamış olabilir */ }

  const visible = jobs.slice(0, 4);
  container.textContent = '';
  if (!visible.length) {
    const empty = document.createElement('div');
    empty.className = 'empty';
    empty.textContent = TR.popupIdle || 'Bekleyen indirme yok';
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

applyI18n();
loadJobs();
loadJobsSetting();

$('options').addEventListener('click', () => {
  chrome.runtime.openOptionsPage();
  window.close();
});

$('downloads').addEventListener('click', () => {
  chrome.tabs.create({ url: 'chrome://downloads/' });
  window.close();
});
