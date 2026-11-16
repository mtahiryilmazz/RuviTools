/* GeTube — popup: son işler ve kısayollar. */
'use strict';

// Arayüz Türkçe: tarayıcı dilinden bağımsız sabit metinler.
const TR = {
  popupLast: 'Son işler',
  popupIdle: 'Bekleyen indirme yok',
  popupSettings: 'Ayarlar',
  popupDownloads: 'İndirilenler',
  popupOpenYoutube: "YouTube'da bir video açın",
  statusDone: 'Tamamlandı',
  statusError: 'Hata'
};

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

applyI18n();
loadJobs();

$('options').addEventListener('click', () => {
  chrome.runtime.openOptionsPage();
  window.close();
});

$('downloads').addEventListener('click', () => {
  chrome.tabs.create({ url: 'chrome://downloads/' });
  window.close();
});
