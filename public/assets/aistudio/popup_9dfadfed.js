// The toolbar button's popup: whether the browser reaches Xenon Download Manager (and, when it
// doesn't, a link to the help page's section for why), pausing capture, leaving the current
// site to the browser, opening the app or its Options, and the media the tab loaded (#38), each
// to download. background.js does the work.
const ext = globalThis.browser ?? globalThis.chrome;
const HELP = "/help/browser-help.html";
// The creator's GitHub Sponsors page (the app's Help → Support XDM): a plain link, in a new tab.
const SPONSORS = "";
let appMessages = null;
function formatCatalog(key, subs) {
  const entry = appMessages && appMessages[key];
  if (!entry || typeof entry.message !== "string") return "";
  let msg = entry.message;
  const ph = entry.placeholders;
  if (ph && typeof ph === "object") {
    msg = msg.replace(/\$([A-Za-z0-9_@]+)\$/g, (all, name) => {
      const k = Object.keys(ph).find((p) => p.toLowerCase() === name.toLowerCase());
      const raw = k && ph[k] && typeof ph[k].content === "string" ? ph[k].content : "";
      return raw ? raw.replace(/\$(\d+)/g, (_, n) => subs[Number(n) - 1] ?? "") : all;
    });
  }
  return msg.replace(/\$(\d+)/g, (_, n) => subs[Number(n) - 1] ?? "").replace(/\$\$/g, "$");
}
const t = (key, ...subs) => formatCatalog(key, subs) || ext.i18n.getMessage(key, subs) || key;
const $ = (id) => document.getElementById(id);
const ask = (m) => ext.runtime.sendMessage(m).catch(() => ({ ok: false }));
const lang = ext.i18n.getUILanguage();
let activeLang = lang;
const isTr = () => String(activeLang || "").split(/[-_]/)[0].toLowerCase() === "tr";
const uiText = (en, tr) => (isTr() ? tr : en);
const PREVIEWS_KEY = "showPreviews";
let showPreviews = true;
let lastMediaState = null;

function applyLocale(langCode, dir) {
  activeLang = langCode || lang;
  document.documentElement.lang = activeLang;
  document.documentElement.dir = dir || (t("@@bidi_dir") === "rtl" ? "rtl" : "ltr");
  for (const el of document.querySelectorAll("[data-i18n]")) el.textContent = t(el.dataset.i18n);
  for (const el of document.querySelectorAll("[data-i18n-placeholder]")) el.placeholder = t(el.dataset.i18nPlaceholder);
  const clearBtn = $("media-clear");
  if (clearBtn) clearBtn.textContent = uiText("Clear list", "Listeyi temizle");
  const prevLabel = $("previews-label");
  if (prevLabel) prevLabel.textContent = uiText("Video previews", "Video önizlemeleri");
  const pageBtn = $("media-page");
  if (pageBtn) pageBtn.textContent = uiText("Download video from this page...", "Bu sayfadan videoyu indir...");
  const proxyRow = $("proxy-row");
  if (proxyRow && !proxyRow.hidden) {
    const manifest = ext.runtime.getManifest();
    const isOpt = (manifest.optional_permissions || []).includes("proxy");
    proxyRow.title = t(isOpt ? "popup_proxy_title" : "popup_proxy_title_chromium");
  }
}
applyLocale(lang, null);
$("help").href = HELP;
$("support").hidden = !SPONSORS;
if (SPONSORS) $("support").href = SPONSORS;

// The tab it is about: the window's active tab; with `?page=ADDRESS` (this page opened in a tab of
// its own, for the tests and the pictures), the tab showing that address.
async function currentTab() {
  const page = new URLSearchParams(location.search).get("page");
  if (page) return (await ext.tabs.query({}).catch(() => [])).find((x) => x.url === page) || null;
  const [tab] = await ext.tabs.query({ active: true, currentWindow: true }).catch(() => []);
  return tab || null;
}

async function show() {
  const s = await ask({ type: "state" });
  if (s && s.messages) {
    appMessages = s.messages;
    applyLocale(s.lang, s.dir);
  }
  const box = $("status");
  const text = $("status-text");
  box.classList.remove("ok", "bad", "warn");
  if (s.connected && s.why === "update-app") {
    box.classList.add("warn");
    text.textContent = t("status_update_app");
  } else if (s.connected && s.remote) {
    box.classList.add(s.paused ? "warn" : "ok");
    text.textContent = s.paused ? t("status_paused") : t("status_remote", s.remote);
  } else if (s.connected) {
    box.classList.add(s.paused ? "warn" : "ok");
    text.textContent = s.paused ? t("status_paused") : s.app ? t("status_connected_version", s.app) : t("status_connected");
  } else {
    box.classList.add("bad");
    text.textContent = t(`status_${(s.why || "not_installed").replace(/-/g, "_")}`);
  }
  const fix = $("fix");
  fix.hidden = s.connected && s.why !== "update-app";
  fix.href = `${HELP}#${s.why || "not-installed"}`;
  $("take").checked = !s.paused;
  showDeclined(s.connected && s.declined);
  $("take").disabled = !s.connected;
  $("open").disabled = $("options").disabled = !s.connected;
  $("version").textContent = s.version ? `v${s.version}` : "";
  // Send downloads to: this computer, or another XDM (#157).
  $("to-here").checked = !s.remote && $("remote-form").hidden;
  $("to-remote").checked = Boolean(s.remote) || !$("remote-form").hidden;
  $("open").disabled = $("open").disabled || Boolean(s.remote);
  $("options").disabled = $("options").disabled || Boolean(s.remote);
  const tab = await currentTab();
  let site = "";
  try {
    const u = new URL(tab && tab.url);
    if (/^https?:$/.test(u.protocol)) site = u.hostname;
  } catch {
    // Not a web page (the browser's own): nothing to leave.
  }
  const exclude = $("exclude");
  exclude.hidden = !site || !s.connected;
  exclude.textContent = t("popup_exclude_site", site);
  exclude.title = exclude.textContent;
  exclude.onclick = async () => {
    const r = await ask({ type: "exclude-site", host: site });
    if (r.ok) {
      exclude.hidden = true;
      $("excluded").hidden = false;
      $("excluded").textContent = t("popup_excluded", site);
    }
  };
  if (site) await showMedia(tab, s.connected);
}

// The last download left to the browser by XDM's Options, and which one (#208); a later one
// taken clears it (background.js noteLeft). A reason this version can't name says no more.
function showDeclined(d) {
  const box = $("declined");
  box.hidden = !(d && d.why);
  if (box.hidden) return;
  const what = d.name ? t("popup_declined", d.name) : t("popup_declined_unnamed");
  const key = `declined_${String(d.why).replace(/-/g, "_")}`;
  const why = t(key);
  $("declined-what").textContent = what;
  $("declined-what").title = what;
  $("declined-why").textContent = why === key ? "" : why;
}

// ---- the media the tab loaded ------------------------------------------------------------------

const TYPES = {
  "video/mp4": "MP4",
  "video/mp2t": "TS",
  "audio/aac": "AAC",
  "audio/mp4": "M4A",
  "audio/mpeg": "MP3",
  "video/webm": "WEBM",
  "audio/webm": "WEBM",
  "audio/ogg": "OGG",
  "video/ogg": "OGV",
  "video/quicktime": "MOV",
  "video/x-matroska": "MKV",
  "audio/flac": "FLAC",
  "audio/wav": "WAV",
  "text/vtt": "VTT",
  "text/srt": "SRT",
  "application/x-subrip": "SRT",
  "application/ttml+xml": "TTML",
  "text/x-ssa": "SSA",
};

// Its type as a short tag: HLS, DASH, else the file's extension or its type's.
function tagOf(m) {
  if (m.kind === "hls") return "HLS";
  if (m.kind === "dash") return "DASH";
  const ext = (/\.([a-z0-9]{2,4})$/i.exec(m.url.split(/[?#]/)[0]) || [])[1];
  return (ext || TYPES[m.mime] || (m.kind === "subtitle" ? "SUB" : m.kind === "audio" ? "AUDIO" : "VIDEO")).toUpperCase();
}

// The address's last part, readable.
function nameOf(url) {
  const last = url.split(/[?#]/)[0].split("/").filter(Boolean).pop() || url;
  try {
    return decodeURIComponent(last);
  } catch {
    return last;
  }
}

function clock(seconds) {
  const s = Math.round(seconds);
  const two = (n) => String(n).padStart(2, "0");
  return s >= 3600 ? `${Math.floor(s / 3600)}:${two(Math.floor(s / 60) % 60)}:${two(s % 60)}` : `${Math.floor(s / 60)}:${two(s % 60)}`;
}

// A size in the browser's language (its own unit names).
function sizeText(bytes) {
  const units = [["gigabyte", 1 << 30], ["megabyte", 1 << 20], ["kilobyte", 1 << 10]];
  const [unit, n] = units.find(([, n]) => bytes >= n) || ["byte", 1];
  try {
    return new Intl.NumberFormat(activeLang, { style: "unit", unit, unitDisplay: "short", maximumFractionDigits: 1 }).format(bytes / n);
  } catch {
    return `${(bytes / n).toFixed(1)} ${unit}`;
  }
}

function displayTitleOf(m, tabTitle) {
  const raw = nameOf(m.url);
  const cleanTab = String(tabTitle || "").trim();
  const generic =
    /^(?:master|index|playlist|manifest|stream|chunklist|sublist|video|media|audio|sub|subs|subtitle|subtitles|caption|captions)(?:[-_.\d]*)?\.(?:m3u8?|mpd|txt|vtt|srt|ass|ssa|ttml)$/i.test(raw) ||
    /^[A-Za-z0-9_-]{20,}(?:\.[a-z0-9]{2,5})?$/i.test(raw);
  if (cleanTab && (generic || m.kind === "hls" || m.kind === "dash")) {
    return m.kind === "subtitle" && (m.label || m.lang) ? `${cleanTab} (${m.label || m.lang})` : cleanTab;
  }
  return raw;
}

function placeholderThumbSvg(kind) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 16 16");
  svg.setAttribute("fill", "currentColor");
  svg.setAttribute("aria-hidden", "true");
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  if (kind === "subtitle") {
    path.setAttribute("d", "M2 3.5A1.5 1.5 0 0 1 3.5 2h9A1.5 1.5 0 0 1 14 3.5v9a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 12.5v-9zm1.5 0v9h9v-9h-9zm1.25 4.25h4.5v1.1h-4.5v-1.1zm5.5 0h1v1.1h-1v-1.1zm-5.5 2.25h2.5v1.1h-2.5V10zm3.5 0h3v1.1h-3V10z");
  } else if (kind === "audio") {
    path.setAttribute("d", "M6 12.5a1.5 1.5 0 1 1-1-1.415V4.5l7-1.5v7.5a1.5 1.5 0 1 1-1-1.415V4.85L6 5.92V12.5z");
  } else {
    path.setAttribute("d", "M5 3.6v8.8a.6.6 0 0 0 .92.51l7.1-4.4a.6.6 0 0 0 0-1.02l-7.1-4.4A.6.6 0 0 0 5 3.6z");
  }
  svg.appendChild(path);
  return svg;
}

function said(text, bad) {
  const p = $("media-said");
  p.hidden = !text;
  p.textContent = text;
  p.classList.toggle("bad", Boolean(bad));
}

function renderMediaList(tab, connected, r) {
  const list = (r && r.media) || [];
  const kept = Boolean(r && r.keptOff);
  const tabTitle = (r && r.title) || (tab && tab.title) || "";
  $("media").hidden = false;
  const clearBtn = $("media-clear");
  if (clearBtn) {
    clearBtn.hidden = list.length === 0 || kept;
    clearBtn.onclick = async () => {
      await ask({ type: "media-clear", tab: tab.id });
      const fresh = await ask({ type: "media", tab: tab.id });
      lastMediaState = { tab, connected, r: fresh };
      renderMediaList(tab, connected, fresh);
    };
  }
  // Why there's nothing to list, honestly: this copy leaves the site alone (Help says what's left
  // to do), the player streams pieces no address names, or the page hasn't loaded any yet.
  const none = $("media-none");
  none.hidden = list.length > 0 && !kept;
  none.textContent = t(kept ? "popup_media_store" : r && r.streamed ? "popup_media_streamed" : "popup_media_none");
  const pageBtn = $("media-page");
  if (pageBtn) {
    const showPageBtn = list.length === 0 && !kept && Boolean(r && r.streamed);
    pageBtn.hidden = !showPageBtn;
    pageBtn.disabled = !connected;
    pageBtn.onclick = async () => {
      pageBtn.disabled = true;
      const reply = await ask({ type: "media-page", tab: tab.id });
      said(reply.ok ? t("video_sent") : reply.why === "options" ? t("video_off") : t("video_failed"), !reply.ok);
      pageBtn.disabled = false;
    };
  }
  $("help").href = kept ? `${HELP}#store-youtube` : HELP;
  const count = $("media-count");
  if (count) {
    count.hidden = list.length === 0 || kept;
    count.textContent = String(list.length);
  }
  const ul = $("media-list");
  ul.replaceChildren();
  for (const m of list) ul.append(mediaCard(m, tab, connected, tabTitle));
}

// A height as a badge (8K ... 144p) and its colour class.
function resBadge(h) {
  if (h >= 4320) return ["8K", "res-uhd"];
  if (h >= 2880) return ["5K", "res-uhd"];
  if (h >= 2160) return ["4K", "res-uhd"];
  if (h >= 1440) return ["2K", "res-qhd"];
  if (h >= 1080) return [`${h}p`, "res-fhd"];
  return [`${h}p`, ""];
}

const svgIcon = (d, view = "0 0 16 16", stroke = false) => {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", view);
  svg.setAttribute("aria-hidden", "true");
  if (stroke) {
    svg.setAttribute("fill", "none");
    svg.setAttribute("stroke", "currentColor");
    svg.setAttribute("stroke-width", "1.8");
    svg.setAttribute("stroke-linecap", "round");
    svg.setAttribute("stroke-linejoin", "round");
  } else {
    svg.setAttribute("fill", "currentColor");
  }
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("d", d);
  svg.appendChild(path);
  return svg;
};

// One captured item as a card: with previews on, a video or stream gets its picture (16:9), its
// length or LIVE over it, its type and a check when the page's playlist is in hand; under it the
// title, its qualities from the highest down, its size, Download and More options. A subtitle, a
// sound, or any item with previews off, is a compact row.
function mediaCard(m, tab, connected, tabTitle) {
  const li = document.createElement("li");
  li.dataset.kind = m.kind;
  const visual = showPreviews && m.kind !== "subtitle" && m.kind !== "audio";
  li.className = visual ? "rich-card" : "row-card";
  if (visual) {
    const shot = document.createElement("div");
    shot.className = "shot thumb";
    if (m.playlistCaptured) shot.classList.add("captured");
    const ph = () => {
      const box = document.createElement("span");
      box.className = "ph";
      box.appendChild(placeholderThumbSvg(m.kind));
      shot.prepend(box);
    };
    if (typeof m.thumb === "string" && (m.thumb.startsWith("data:image/") || /^https?:\/\//i.test(m.thumb))) {
      const img = document.createElement("img");
      img.src = m.thumb;
      img.alt = "";
      img.onerror = () => {
        img.remove();
        ph();
      };
      shot.appendChild(img);
    } else {
      ph();
    }
    const kind = document.createElement("span");
    kind.className = "kind";
    kind.textContent = tagOf(m);
    shot.appendChild(kind);
    if (m.playlistCaptured) {
      const ready = document.createElement("span");
      ready.className = "ready";
      ready.textContent = uiText("Ready", "Hazır");
      ready.title = uiText("Ready: Xenon has this video's playlist from the page", "Hazır: Xenon bu videonun oynatma listesini sayfadan yakaladı");
      shot.appendChild(ready);
    }
    if (m.live || m.duration > 0) {
      const dur = document.createElement("span");
      dur.className = m.live ? "dur live" : "dur";
      dur.textContent = m.live ? t("popup_media_live") : clock(m.duration);
      shot.appendChild(dur);
    }
    li.appendChild(shot);
  }
  const body = document.createElement("div");
  body.className = "body";
  if (!visual) {
    const tile = document.createElement("span");
    tile.className = "icon-tile";
    if (showPreviews) tile.appendChild(placeholderThumbSvg(m.kind));
    else tile.textContent = tagOf(m);
    body.appendChild(tile);
  }
  const what = document.createElement("div");
  what.className = "what";
  const name = document.createElement("span");
  name.className = "name";
  name.textContent = displayTitleOf(m, tabTitle);
  name.title = m.url;
  what.appendChild(name);
  const badges = document.createElement("div");
  badges.className = "badges";
  const badge = (text, cls = "") => {
    const b = document.createElement("span");
    b.className = cls ? `badge ${cls}` : "badge";
    b.textContent = text;
    badges.appendChild(b);
  };
  if (m.kind === "subtitle") badge(uiText("SUBTITLE", "ALTYAZI"), "badge-sub");
  if (!visual || m.kind === "subtitle") badge(tagOf(m));
  if (m.segmentExt) badge(String(m.segmentExt).toUpperCase());
  // Every quality the stream offers, the highest first.
  const heights = [...new Set((m.heights || []).map(Number).filter((h) => h > 0))].sort((a, b) => b - a);
  for (const h of heights.slice(0, 6)) badge(...resBadge(h));
  if (heights.length > 6) badge(`+${heights.length - 6}`);
  if (m.kind === "subtitle" && (m.label || m.lang)) badge(String(m.label || m.lang).toUpperCase());
  if (!visual && (m.live || m.duration > 0)) badge(m.live ? t("popup_media_live") : clock(m.duration));
  if (badges.childElementCount) what.appendChild(badges);
  body.appendChild(what);

  const actions = document.createElement("div");
  actions.className = "card-actions";
  const sizeStr = m.size > 0 ? sizeText(m.size) : m.estSize > 0 ? `~${sizeText(m.estSize)}` : "";
  const size = document.createElement("span");
  size.className = "size meta";
  size.textContent = [m.width && heights.length === 1 ? `${m.width}×${heights[0]}` : "", sizeStr].filter(Boolean).join(" · ");
  if (visual) actions.appendChild(size);
  if (m.drm) {
    // Copy-protected: listed, never sent.
    const drm = document.createElement("span");
    drm.className = "drm";
    drm.textContent = t("popup_media_protected");
    drm.title = t("video_drm");
    actions.append(drm);
  } else {
    const get = document.createElement("button");
    get.className = "dl";
    get.append(svgIcon("M8 2.5v8m0 0L4.8 7.3M8 10.5l3.2-3.2M3 13.5h10", "0 0 16 16", true));
    const word = document.createElement("span");
    word.textContent = t("popup_media_download");
    get.append(word);
    get.title = t("menu");
    get.disabled = !connected;
    get.onclick = async () => {
      get.disabled = true;
      const reply = await ask({ type: "media-download", tab: tab.id, url: m.url });
      said(reply.ok ? t("video_sent") : reply.why === "drm" ? t("video_drm") : t("video_failed"), !reply.ok);
      get.disabled = false;
    };
    actions.append(get);
    if (m.kind !== "subtitle" && m.kind !== "audio") {
      // More options: Xenon's own video window for the page, every quality and subtitle in it.
      const more = document.createElement("button");
      more.className = "more";
      more.title = `${t("video_more")}...`;
      more.setAttribute("aria-label", more.title);
      more.append(svgIcon("M3.5 8h.01M8 8h.01M12.5 8h.01", "0 0 16 16", true));
      more.disabled = !connected;
      more.onclick = async () => {
        more.disabled = true;
        const reply = await ask({ type: "media-page", tab: tab.id, url: m.url });
        said(reply.ok ? t("video_sent") : reply.why === "options" ? t("video_off") : t("video_failed"), !reply.ok);
        more.disabled = false;
      };
      actions.append(more);
    }
  }
  if (visual) {
    what.appendChild(actions);
  } else {
    if (sizeStr) {
      const meta = document.createElement("span");
      meta.className = "meta";
      meta.textContent = sizeStr;
      what.appendChild(meta);
    }
    body.appendChild(actions);
  }
  li.appendChild(body);
  return li;
}

async function showMedia(tab, connected) {
  try {
    const stored = await ext.storage.local.get({ [PREVIEWS_KEY]: true });
    showPreviews = stored[PREVIEWS_KEY] !== false;
  } catch {
    showPreviews = true;
  }
  const prevBox = $("previews");
  if (prevBox) {
    prevBox.checked = showPreviews;
    prevBox.onchange = () => {
      showPreviews = prevBox.checked;
      ext.storage.local.set({ [PREVIEWS_KEY]: showPreviews }).catch(() => {});
      if (lastMediaState) renderMediaList(lastMediaState.tab, lastMediaState.connected, lastMediaState.r);
    };
  }
  const r = await ask({ type: "media", tab: tab.id });
  lastMediaState = { tab, connected, r };
  renderMediaList(tab, connected, r);
}

// #42: reading the browser's proxy (a VPN or proxy extension's) is an opt-in, this switch, off as
// installed. Firefox: the optional "proxy" permission, asked for on it. Chromium can't make that
// permission optional, so the extension has it from the install, and the switch is a setting of
// its own (`proxyOn`, which background.js reads before each download).
const PROXY = { permissions: ["proxy"] };
const manifest = ext.runtime.getManifest();
if (!(manifest.optional_permissions || []).includes("proxy") && (manifest.permissions || []).includes("proxy")) {
  const row = $("proxy-row");
  const box = $("proxy");
  row.hidden = false;
  row.title = t("popup_proxy_title_chromium");
  ext.storage.local
    .get({ proxyOn: false })
    .then((v) => (box.checked = Boolean(v.proxyOn)))
    .catch(() => {});
  box.addEventListener("change", () => ext.storage.local.set({ proxyOn: box.checked }).catch(() => {}));
}
if ((manifest.optional_permissions || []).includes("proxy")) {
  const row = $("proxy-row");
  const box = $("proxy");
  row.hidden = false;
  row.title = t("popup_proxy_title");
  const sync = () =>
    ext.permissions
      .contains(PROXY)
      .catch(() => false)
      .then((on) => (box.checked = Boolean(on)));
  sync();
  box.addEventListener("change", () => {
    // Asked for in the click itself (the browser allows it from a user's action only): nothing
    // awaited before it. Refused, or taken back: the switch shows what the browser says.
    const asked = box.checked ? ext.permissions.request(PROXY) : ext.permissions.remove(PROXY);
    asked.catch(() => false).then(sync);
  });
}

$("take").addEventListener("change", async (e) => {
  await ask({ type: "pause", paused: !e.target.checked });
  show();
});
$("open").addEventListener("click", async () => {
  await ask({ type: "open-app" });
  window.close();
});
$("options").addEventListener("click", async () => {
  await ask({ type: "open-options" });
  window.close();
});

// Send downloads to (#157): another XDM is paired here with its address and a code it shows.
$("to-remote").addEventListener("change", () => {
  $("remote-form").hidden = false;
  $("remote-url").focus();
});
$("to-here").addEventListener("change", async () => {
  $("remote-form").hidden = true;
  await ask({ type: "remote-off" });
  show();
});
$("remote-open").addEventListener("click", () => {
  try {
    const u = new URL($("remote-url").value.trim());
    if (u.protocol === "https:") ext.tabs.create({ url: u.origin + "/" });
  } catch {
    $("remote-said").textContent = t("remote_address");
    $("remote-said").hidden = false;
  }
});
$("remote-pair").addEventListener("click", async () => {
  const said = $("remote-said");
  said.hidden = true;
  const r = await ask({ type: "remote-pair", url: $("remote-url").value, code: $("remote-code").value });
  if (r.ok) {
    $("remote-form").hidden = true;
    $("remote-code").value = "";
    show();
    return;
  }
  said.textContent = t(`remote_${String(r.why || "failed").replace(/-/g, "_")}`);
  said.hidden = false;
});

show();
