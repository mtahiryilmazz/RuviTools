// Xenon Download Manager's browser extension: the background part (Chrome, Edge, Brave, Opera
// and Vivaldi run it as a service worker, Firefox as a background script; one file for both, see
// manifest.json and manifest.firefox.json).
//
// Downloads of the file types Xenon Download Manager takes are handed to it: the browser's own
// download is cancelled, and the address, referrer, cookies, User-Agent, file name, size, type
// and (for a form) the posted body go to the app through the native messaging host
// (crates/nmhost). When it can't be reached the download stays with the browser, so nothing is
// lost, and the toolbar button says why, linking the help page's section for it
// (docs/browser-help.md). The browser may stop this script whenever it is idle: what must last
// (the rules, the status, the pause) is kept in storage, not in memory.

const ext = globalThis.browser ?? globalThis.chrome;
const HOST = "com.xdm.host";
const VERSION = ext.runtime.getManifest().version;
const HELP = "/help/browser-help.html";
const action = ext.action ?? ext.browserAction;
// Safari has no downloads API at all (nor proxy): it can't take a download from the browser, so
// the links clicked in a page are handed over instead (`link`, below; page.js).
const SAFARI = !ext.downloads;
// Firefox has no pause before the file name (onDeterminingFilename): its own paths below.
const FIREFOX = !SAFARI && !ext.downloads.onDeterminingFilename;
// Brave and Opera say who they are only in the client hints, Firefox's forks (Zen, LibreWolf,
// Floorp, Waterfox) only in runtime.getBrowserInfo, which only Firefox's family has. Both go to
// the host as `brands` (Options lists them by name; crates/ipc capture::browser_named).
const BRANDS = (globalThis.navigator?.userAgentData?.brands || []).map((b) => b.brand);
const APP_NAME = ext.runtime.getBrowserInfo
  ? ext.runtime.getBrowserInfo().then((i) => (i && i.name) || null, () => null)
  : Promise.resolve(null);

const LOCALES = new Set([
  "am", "ar", "bg", "bn", "ca", "cs", "da", "de", "el", "en", "es", "es_419", "et", "fa", "fi",
  "fil", "fr", "gu", "he", "hi", "hr", "hu", "id", "it", "ja", "kn", "ko", "lt", "lv", "ml",
  "mr", "ms", "nl", "no", "pl", "pt_BR", "pt_PT", "ro", "ru", "sk", "sl", "sr", "sv", "ta",
  "te", "th", "tr", "uk", "vi", "zh_CN", "zh_TW",
]);
const LOCALE_MAP = {
  "es-mx": "es_419",
  nb: "no",
  pt: "pt_PT",
  "pt-br": "pt_BR",
  "zh-hans": "zh_CN",
  "zh-hant": "zh_TW",
};
const RTL_LOCALES = new Set(["ar", "fa", "he"]);
const VIDEO_KEYS = [
  "video_download", "video_more", "video_hide_here", "video_never_site", "video_settings",
  "video_sent", "video_failed", "video_off", "video_drm", "video_unnamed", "video_drag",
];

let appLang = "";
let appCatalog = null;
let enCatalog = null;
let loadingLocale = null;

function localeFolder(code) {
  const norm = String(code || "").trim().toLowerCase().replace(/_/g, "-");
  if (!norm) return "";
  const mapped = LOCALE_MAP[norm] || norm.replace(/-/g, "_");
  if (LOCALES.has(mapped)) return mapped;
  const base = norm.split("-")[0];
  const mappedBase = LOCALE_MAP[base] || base;
  return LOCALES.has(mappedBase) ? mappedBase : "en";
}

function formatFrom(catalog, fallback, key, subs) {
  const entry = (catalog && catalog[key]) || (fallback && fallback[key]);
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

async function fetchCatalog(folder) {
  try {
    const r = await fetch(ext.runtime.getURL(`_locales/${folder}/messages.json`));
    return r && r.ok ? JSON.parse(await r.text()) : null;
  } catch {
    return null;
  }
}

async function applyAppLang(code) {
  const norm = String(code || "").trim().toLowerCase().replace(/_/g, "-");
  if (!norm) return;
  if (norm === appLang && (appCatalog || enCatalog)) return;
  const run = (async () => {
    const folder = localeFolder(norm);
    const [cat, en] = await Promise.all([
      fetchCatalog(folder),
      folder === "en" ? Promise.resolve(null) : enCatalog ? Promise.resolve(enCatalog) : fetchCatalog("en"),
    ]);
    const changed = norm !== appLang;
    appLang = norm;
    appCatalog = cat || en;
    if (folder === "en" && cat) enCatalog = cat;
    else if (en) enCatalog = en;
    await ext.storage.local.set({ appLang: norm }).catch(() => {});
    if (changed) {
      createMenus();
      await showStatus();
    }
  })();
  loadingLocale = run;
  try {
    await run;
  } finally {
    if (loadingLocale === run) loadingLocale = null;
  }
}

async function ensureAppLocale() {
  if (loadingLocale) await loadingLocale;
  if (appCatalog || enCatalog) return;
  const { appLang: saved } = await ext.storage.local.get({ appLang: "" }).catch(() => ({ appLang: "" }));
  if (saved) await applyAppLang(saved);
}

function appDir() {
  if (!appLang) return ext.i18n.getMessage("@@bidi_dir") === "rtl" ? "rtl" : "ltr";
  return RTL_LOCALES.has(appLang.split(/[-_]/)[0]) ? "rtl" : "ltr";
}

function mergedCatalog(keys) {
  if (!appCatalog && !enCatalog) return null;
  const out = {};
  const names = keys || Object.keys({ ...(enCatalog || {}), ...(appCatalog || {}) });
  for (const k of names) {
    const entry = (appCatalog && appCatalog[k]) || (enCatalog && enCatalog[k]);
    if (entry) out[k] = entry;
  }
  return out;
}

const t = (key, ...subs) => {
  if (key === "@@bidi_dir" && appLang) return appDir();
  if (appCatalog || enCatalog) {
    const out = formatFrom(appCatalog, enCatalog, key, subs);
    if (out) return out;
  }
  return ext.i18n.getMessage(key, subs) || key;
};

// ---- the native host ---------------------------------------------------------------------

// How long an answer may take: the host starts Xenon Download Manager when it isn't running
// and waits for it (crates/nmhost START_WAIT); the browser's download waits meanwhile.
const HOST_WAIT = 30000;

function withTimeout(promise, ms) {
  let timer;
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error("no answer")), ms);
    }),
  ]).finally(() => clearTimeout(timer));
}

// Why the browser couldn't reach the host, as the help page's sections name it.
function hostProblem(error) {
  const m = String((error && error.message) || error);
  if (/not found|no such native application/i.test(m)) return "not-installed";
  if (/forbidden/i.test(m)) return "forbidden";
  if (/no answer/i.test(m)) return "not-running";
  return "host-failed";
}

// Every message says which browser and extension version it comes from (the app keeps each
// browser's status, and the rules depend on the browser). Never throws: failures come back as
// `{ok: false, why}`.
async function native(message, ms = HOST_WAIT) {
  // A download for another XDM (a server, #157): sent there, not to this computer's.
  if (!message.probe && !message.panel && typeof message.url === "string" && message.url) {
    const target = await remoteTarget();
    if (target) return remoteSend(target, message);
  }
  const app = await APP_NAME;
  const brands = app ? [...BRANDS, app] : BRANDS;
  const sent = { ...message, userAgent: navigator.userAgent, brands, v: VERSION };
  try {
    const reply = (await withTimeout(ext.runtime.sendNativeMessage(HOST, sent), ms)) || {};
    if (typeof reply.lang === "string" && reply.lang) await applyAppLang(reply.lang);
    if (reply.version) await noteStatus(older(reply.version, VERSION) ? "update-app" : null, reply.version);
    else if (reply.ok || reply.why === "options") await noteStatus(null);
    else if (reply.why && reply.why !== "refused") await noteStatus(reply.why);
    return reply;
  } catch (e) {
    const why = hostProblem(e);
    // With another XDM chosen, this computer's needn't be there.
    if (!(await remoteTarget())) await noteStatus(why);
    return { ok: false, why, error: String((e && e.message) || e) };
  }
}

// ---- another XDM (#157): a server's, through its remote control ------------------------------

// {url, token} while downloads go to another XDM instead of this computer's, else null.
async function remoteTarget() {
  const { remote } = await ext.storage.local.get({ remote: null });
  return remote && remote.url && remote.token ? remote : null;
}

// One request to the remote control's API at `base` (https://server:8463/). Never throws:
// {status, body}, status 0 when it isn't reached (not running, or its certificate not accepted
// in this browser yet). The browser's cookies never go with it.
async function remoteCall(base, path, body, token) {
  try {
    const r = await withTimeout(
      fetch(new URL(path, base), {
        method: "POST",
        headers: { "Content-Type": "application/json", ...(token ? { Authorization: `Bearer ${token}` } : {}) },
        body: JSON.stringify(body),
        credentials: "omit",
        cache: "no-store",
        redirect: "error",
      }),
      15000,
    );
    let json = null;
    try {
      json = await r.json();
    } catch {
      json = null;
    }
    return { status: r.status, body: json };
  } catch {
    return { status: 0, body: null };
  }
}

// A download handed to the other XDM: its address only (a site's sign-in stays in this browser).
async function remoteSend(target, message) {
  const r = await remoteCall(target.url, "api/add", { url: message.url, start: true }, target.token);
  if (r.status === 200 && r.body && !r.body.error) return { ok: true };
  if (r.status === 401) await ext.storage.local.set({ remote: null }); // unpaired there
  return { ok: false, why: "refused" };
}

// The file types the app takes by default (mdm_ipc::capture::DEFAULT_AUTO_TYPES; a test keeps
// them the same): what is caught for another XDM when this computer has none to ask.
const DEFAULT_TYPES =
  "00* 264 265 3DM 3DS 3G2 3GP 3GP2 3GPP 3GPP2 3MF 7Z AAB AAC ABC AC3 AC4 ACE AFDESIGN AFPHOTO AI AIF AIFF AMV APE APK APKM APKS APPIMAGE APPX APPXBUNDLE ARJ ARROW ARW ASF ASX AU AV1 AVC AVI AVRO AZW AZW3 BAK BAM BIK BIN BLEND BRAW BZ BZ2 C4D CAB CAF CBR CBZ CCD CHD CKPT CPIO CR2 CR3 CRAM CSO DAE DAT DCM DDS DEB DFF DIVX DJVU DMG DNG DOC DOCX DSF DTS DTSHD DV DVR DVR-MS DWG DXF EAC3 EC3 EPS EPUB EVO EXE EXR F3D F4A F4B F4P F4V FASTA FASTQ FB2 FBX FEATHER FIG FITS FLAC FLATPAK FLC FLI FLV FQ GCODE GCZ GEM GGML GGUF GLB GLTF GPKG GRIB GRIB2 GVI GZ GZIP H264 H265 H5 HDF5 HDMOV HDR HEVC IFO IGES IGS IMG INDD IPA ISM ISMA ISMV ISO ISZ IVF JAR JOBLIB JSONL KERAS KRA LAS LAZ LLAMAFILE LWO LZ LZ4 LZH LZMA M1V M2P M2T M2TS M2V M4A M4B M4P M4R M4V MA MAT MAX MB MBTILES MDF MDS MID MIDI MJ2 MJPG MK3D MKA MKS MKV MLMODEL MMV MOBI MOD MOOV MOV MOVIE MP2 MP21 MP2V MP3 MP4 MP4V MPA MPE MPEG MPEG1 MPEG2 MPEG4 MPG MPG2 MPV MPV2 MQV MSI MSIX MSIXBUNDLE MSP MSU MTS MTV MVE MXF NC NDJSON NEF NEMO NII NPY NPZ NRG NSP NSV NSZ NUPKG NUT NUV OBJ ODP ODS ODT OGA OGG OGM OGV OGX ONNX OPUS ORC ORF OTF OVA OVF PAK PARQUET PB PBF PDF PICKLE PK3 PK4 PKG PKL PLJ PLY POTX PPS PPSX PPT PPTX PSB PSD PT PTH PVA QCOW QCOW2 QT R0* R1* R2* R3* R4* R5* R6* R7* R8* R9* RA RAF RAR RDS REC RM RMD RMV RMVB ROQ RPM RTF RUN RV RVZ RW2 SAFETENSORS SEA SEC SF2 SFZ SHP SIT SITX SKETCH SKP SLDASM SLDPRT SMK SMV SNAP SQLITE SQLITE3 STEP STL STP STR SVI SWF TAR TBZ TBZ2 TFLITE TGA TGZ TIF TIFF TLZ TOD TORRENT TP TP0 TRP TS TTA TTC TTF TXZ TZST UNITYPACKAGE USD USDA USDC USDZ VC1 VDB VDI VDO VHD VHDX VID VIV VIVO VMDK VOB VPK VRO VSIX WAV WBFS WEBA WEBM WHL WIM WM WMA WMV WPL WRL WTV WUA WV X3D XAPK XCF XCI XESC XLS XLSB XLSM XLSX XVID XZ Y4M Z Z0* ZIP ZIPX ZPR ZST ZTL";

// Pair with the XDM at `address` with a code it shows; {ok} or {ok: false, why, left}.
async function remotePair(address, code) {
  let base;
  try {
    base = new URL(String(address).trim());
  } catch {
    return { ok: false, why: "address" };
  }
  if (base.protocol !== "https:") return { ok: false, why: "address" };
  base = `${base.origin}/`;
  const name = t("remote_device_name") === "remote_device_name" ? "Browser" : t("remote_device_name");
  const r = await remoteCall(base, "api/pair", { code: String(code), name }, null);
  if (r.status === 200 && r.body && typeof r.body.token === "string") {
    await ext.storage.local.set({ remote: { url: base, token: r.body.token } });
    await refreshRules();
    await showStatus();
    return { ok: true };
  }
  const why = { 0: "unreachable", 403: "wrong-code", 410: "no-code", 429: "too-soon" }[r.status] || "failed";
  return { ok: false, why, left: r.body && r.body.left };
}

// Back to this computer: unpaired there too (when it answers).
async function remoteOff() {
  const target = await remoteTarget();
  if (target) await remoteCall(target.url, "api/forget", {}, target.token);
  await ext.storage.local.set({ remote: null });
  await refreshRules();
  await showStatus();
  return { ok: true };
}

// a < b, both "x.y.z".
// Is version `a` an older release than `b`? Only the first two numbers count: the store can publish
// a bug-fix add-on before its app is out, and nothing it offers needs that app.
function older(a, b) {
  const pa = String(a).split(/[.-]/).map(Number);
  const pb = String(b).split(/[.-]/).map(Number);
  for (let i = 0; i < 2; i++) {
    if ((pa[i] || 0) !== (pb[i] || 0)) return (pa[i] || 0) < (pb[i] || 0);
  }
  return false;
}

// ---- status: the toolbar button's badge and title, the popup's first line ------------------

let status = null; // {why, app, at}
async function readStatus() {
  if (!status) status = (await ext.storage.local.get({ status: { why: null } })).status;
  return status;
}

// `why`: null when connected, else the reason (the help page's section). Written only when it
// changes.
async function noteStatus(why, app) {
  const now = await readStatus();
  if (now.why === why && (!app || now.app === app)) return;
  const was = !now.why || now.why === "update-app";
  status = { why, app: app || now.app || null, at: Date.now() };
  await ext.storage.local.set({ status });
  await showStatus();
  // Unreachable, PDFs and music open in the browser again (and back once connected).
  if (was !== (!why || why === "update-app")) await syncOpenedRule();
}

async function showStatus() {
  const why = (await remoteTarget()) ? null : (await readStatus()).why;
  const { paused } = await ext.storage.local.get({ paused: false });
  try {
    await action.setBadgeBackgroundColor({ color: why ? "#C42B1C" : "#6B6B6B" });
    await action.setBadgeText({ text: why && why !== "update-app" ? "!" : paused ? "II" : "" });
    await action.setTitle({ title: why ? t(`status_${why.replace(/-/g, "_")}`) : paused ? t("status_paused") : t("status_connected") });
  } catch {
    // No toolbar button (a browser without one): the popup and the help page still say it.
  }
}

// ---- the rules: Options in the app, kept here between the browser's stops ------------------

// {types, capture, captureOff, excludedSites, excludedAddresses, opened, minKb, at}; null until
// first asked. `captureOff`: why `capture` is false (a host before #208 doesn't say).
let rules = null;

async function storedRules() {
  if (!rules) rules = (await ext.storage.local.get({ rules: null })).rules;
  return rules;
}

// One question at a time: callers meanwhile get its answer.
let asking = null;
// How long Options rules stay cached before re-checking the desktop app (short enough that
// adding or removing an extension takes effect immediately, while back-to-back downloads reuse
// one query). Window focus changes also refresh immediately.
const RULES_TTL_MS = 5000;
let rulesAskedAt = 0;
function refreshRules() {
  asking = asking || askRules().finally(() => (asking = null));
  return asking;
}

async function askRules() {
  const reply = await native({ rules: true });
  if (reply.ok && typeof reply.types === "string") {
    rules = {
      types: reply.types,
      capture: reply.capture !== false,
      captureOff: typeof reply.captureOff === "string" ? reply.captureOff : "",
      excludedSites: reply.excludedSites || "",
      excludedAddresses: Array.isArray(reply.excludedAddresses) ? reply.excludedAddresses : [],
      opened: reply.opened !== false,
      minKb: Number(reply.minKb) || 0,
      at: Date.now(),
    };
    await ext.storage.local.set({ rules });
    await syncOpenedRule();
    return rules;
  }
  // No XDM on this computer, another one chosen: the app's default file types (the browser
  // keeps the files it would open).
  if (await remoteTarget()) {
    rules = { types: DEFAULT_TYPES, capture: true, excludedSites: "", excludedAddresses: [], opened: false, minKb: 0, at: Date.now() };
    // Kept as the app's are: Safari's page.js holds links by the stored copy.
    await ext.storage.local.set({ rules });
    return rules;
  }
  return null;
}

// The rules to decide by now: cached while fresh (`RULES_TTL_MS`), else awaited from the host
// so newly added or removed file types apply on the very next click/download.
async function currentRules() {
  const r = await storedRules();
  const { why } = await readStatus();
  if (!r || (why && why !== "update-app") || Date.now() - r.at > RULES_TTL_MS) {
    const fresh = await refreshRules();
    return fresh || r;
  }
  return r;
}

function normalizeTypeTokens(list) {
  return String(list || "")
    .split(/[\s,;]+/)
    .map((raw) => {
      const t = raw.trim();
      if (!t) return "";
      return t === "*" ? "*" : t.replace(/^\*\./, "").replace(/^\.+/, "");
    })
    .filter(Boolean);
}

function typePatterns(list) {
  return normalizeTypeTokens(list).map(
    (t) => new RegExp("^" + t.replace(/[.+?^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*") + "$", "i"),
  );
}

function extensionOf(name) {
  const base = name.split(/[\\/]/).pop() || "";
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(dot + 1) : "";
}

// As the app matches them (mdm_ipc::capture): `*.example.com` takes the domain and its
// subdomains; an address pattern's `*` any run of characters.
function hostMatches(host, pattern) {
  let p = pattern.trim().toLowerCase().replace(/^[a-z]+:\/\//, "").split("/")[0];
  if (p.startsWith("*.")) {
    p = p.slice(2);
    return host === p || host.endsWith("." + p);
  }
  return p !== "" && host === p;
}

function wildcard(pattern, text) {
  const re = new RegExp("^" + pattern.toLowerCase().split("*").map((s) => s.replace(/[.+?^${}()|[\]\\]/g, "\\$&")).join(".*") + "$");
  return re.test(text.toLowerCase());
}

// An address pattern with a scheme is the whole address; without one, the rest after it.
function addressMatches(pattern, url) {
  const p = pattern.trim();
  if (!p) return false;
  return wildcard(p, p.includes("://") ? url : url.replace(/^[a-z][a-z0-9+.-]*:\/\//i, ""));
}

// Which of Options' lists leaves `url` to the browser: "site", "address", or "" (neither).
function excludedBy(r, url) {
  let host = "";
  try {
    host = new URL(url).hostname.toLowerCase();
  } catch {
    return "site";
  }
  if (r.excludedSites.split(/\s+/).some((p) => p && hostMatches(host, p))) return "site";
  if (r.excludedAddresses.some((p) => addressMatches(p, url))) return "address";
  return "";
}

// ---- downloads left to the browser: why, for the popup and XDM's log (#208) ----------------

// A host's refusal by Options names the rule (`declined`); one before #208 says it only in its
// error text, by the rule's Rust name (mdm_ipc::capture::Declined).
const DECLINED_NAMES = {
  IntegrationOff: "integration-off",
  Browser: "browser",
  FileType: "file-type",
  Site: "site",
  Address: "address",
  Small: "small",
};
function declinedOf(reply) {
  if (typeof reply.declined === "string" && reply.declined) return reply.declined;
  const m = /left to the browser by Options \((\w+)\)/.exec(String(reply.error || ""));
  return (m && DECLINED_NAMES[m[1]]) || "";
}

function siteOf(url) {
  try {
    return new URL(url).hostname;
  } catch {
    return "";
  }
}

// The last download left to the browser by Options (or because the extension may not read the
// site): kept for the popup ({why, name, site, at}; a private window's without its name or
// site), and, when the extension decided alone (`told` false), told to the host for XDM's log.
// The host logs its own refusals itself. A download taken later clears it.
async function noteLeft(why, name, url, incognito, told) {
  const declined = incognito ? { why, name: "", site: "", at: Date.now() } : { why, name, site: siteOf(url), at: Date.now() };
  await ext.storage.local.set({ declined });
  if (told) return;
  // Under a key of its own, with no address: an older host refuses it as one it can't
  // download, and hands nothing over.
  await native({ declined: { why, name: declined.name, site: declined.site }, ...(incognito ? { private: true } : {}) });
}

// ---- what the page told us: forms posted, links clicked with Alt -------------------------

// Forms posted in a page or frame, by address: a download they lead to is posted again by the
// app, with the same body (it can't be sent as a GET). Kept two minutes.
const posts = new Map();
const BODY_LIMIT = 512 * 1024;
// The Referer each page load was sent with, by request (Firefox, two minutes), and by resource
// address (`extraHeaders` on Chromium, five minutes) so Referer-gated CDNs get the exact header.
const referrers = new Map();
const requestReferrers = new Map();
// Passive MAIN-world manifest bodies (`media-observer.js`), keyed by URL without `#fragment`,
// so single-use/short-lived tokenized HLS/DASH manifests are probed and downloaded without
// issuing duplicate GET requests.
const manifestSnapshots = new Map();
const MANIFEST_SNAPSHOT_TTL = 600000;
const MANIFEST_SNAPSHOT_MAX = 64;
// Frames where `HTMLMediaElement.setMediaKeys` was called (Widevine/PlayReady/FairPlay EME),
// including detached `new Audio()` players not present in the DOM.
const drmFrames = new Map();
const DRM_FRAME_TTL = 3600000;

function clearDrmForTab(tabId) {
  const prefix = `${tabId}:`;
  for (const k of drmFrames.keys()) if (k.startsWith(prefix)) drmFrames.delete(k);
}

function hasDrmHint(tabId, frameId) {
  const hit = drmFrames.get(`${tabId}:${frameId || 0}`);
  return Boolean(hit && Date.now() - hit.at <= DRM_FRAME_TTL);
}

function forget(map, ms) {
  const now = Date.now();
  for (const [k, v] of map) if (now - v.at > ms) map.delete(k);
}

// A form's body as the browser sent it: its fields (formData: put together again in the
// form's own encoding once its Content-Type is known, `postOf`), or the raw bytes when they
// are text. Anything else (a file upload) can't be sent again: `body: null` leaves its download
// to the browser.
function formBody(details) {
  const rb = details.requestBody;
  if (!rb) return { body: null };
  if (rb.formData) {
    const fields = Object.entries(rb.formData).flatMap(([k, values]) => values.map((v) => [k, String(v)]));
    const size = fields.reduce((n, [k, v]) => n + k.length + v.length + 2, 0);
    return size > BODY_LIMIT ? { body: null } : { fields, type: "application/x-www-form-urlencoded" };
  }
  if (Array.isArray(rb.raw) && rb.raw.every((p) => p.bytes)) {
    const size = rb.raw.reduce((n, p) => n + p.bytes.byteLength, 0);
    if (size > BODY_LIMIT) return { body: null };
    const bytes = new Uint8Array(size);
    let at = 0;
    for (const p of rb.raw) {
      bytes.set(new Uint8Array(p.bytes), at);
      at += p.bytes.byteLength;
    }
    try {
      return { body: new TextDecoder("utf-8", { fatal: true }).decode(bytes), type: null };
    } catch {
      return { body: null };
    }
  }
  return { body: null };
}

if (ext.webRequest) {
  const navigations = { urls: ["<all_urls>"], types: ["main_frame", "sub_frame"] };
  ext.webRequest.onBeforeRequest.addListener(
    (d) => {
      if (d.method !== "POST") return;
      forget(posts, 120000);
      posts.set(d.url, { ...formBody(d), at: Date.now(), id: d.requestId });
    },
    navigations,
    ["requestBody"],
  );
  // The Content-Type the form went with (a multipart boundary must stay the same), (Firefox)
  // the page a load came from, and the exact Referer sent on media/XHR requests (`extraHeaders`
  // on Chromium, where cross-origin Referer headers are otherwise hidden from extensions).
  const onSendHeaders = (d) => {
    const header = (n) => (d.requestHeaders || []).find((h) => h.name.toLowerCase() === n);
    const ref = header("referer");
    const orig = header("origin");
    if ((ref && ref.value) || (orig && orig.value)) {
      forget(requestReferrers, 3600000);
      while (requestReferrers.size >= 1000) {
        const oldest = requestReferrers.keys().next().value;
        if (oldest === undefined) break;
        requestReferrers.delete(oldest);
      }
      const prev = requestReferrers.get(d.url);
      requestReferrers.set(d.url, {
        referer: (ref && ref.value) || (prev && prev.referer) || "",
        origin: (orig && orig.value) || (prev && prev.origin) || "",
        frameId: d.frameId || 0,
        tabId: d.tabId,
        at: Date.now(),
      });
      if (FIREFOX && ref && ref.value) {
        forget(referrers, 120000);
        referrers.set(d.requestId, { url: ref.value, at: Date.now() });
      }
    }
    if (d.method !== "POST") return;
    const p = posts.get(d.url);
    const ct = header("content-type");
    if (p && p.id === d.requestId && ct && ct.value) p.type = ct.value;
  };
  const headerTypes = {
    urls: ["<all_urls>"],
    types: ["main_frame", "sub_frame", "xmlhttprequest", "media", "other"],
  };
  try {
    ext.webRequest.onBeforeSendHeaders.addListener(
      onSendHeaders,
      headerTypes,
      FIREFOX ? ["requestHeaders"] : ["requestHeaders", "extraHeaders"],
    );
  } catch {
    ext.webRequest.onBeforeSendHeaders.addListener(onSendHeaders, headerTypes, ["requestHeaders"]);
  }
}

// The form posted to `url` (within two minutes), as the app sends it again: urlencoded fields,
// or a raw text body with its own type. null: it can't be sent again (a file in it; a multipart
// form, whose fields the browser gives without its files): the browser keeps the download.
function postOf(url) {
  const p = posts.get(url);
  if (!p || Date.now() - p.at > 120000) return undefined;
  if (p.body === null || (p.fields && /multipart/i.test(p.type || ""))) return null;
  if (!p.fields) return { body: p.body, type: p.type };
  const params = new URLSearchParams();
  for (const [k, v] of p.fields) params.append(k, v);
  return { body: params.toString(), type: "application/x-www-form-urlencoded" };
}

// Where page loads' redirects started, by where they ended (two minutes): Firefox names a
// download by its last address only, and the app keeps the first (an expired signed link is
// renewed from it).
const redirected = new Map();
if (ext.webRequest && ext.webRequest.onBeforeRedirect) {
  ext.webRequest.onBeforeRedirect.addListener(
    (d) => {
      forget(redirected, 120000);
      const first = (redirected.get(d.url) || {}).from || d.url;
      redirected.set(d.redirectUrl, { from: first, at: Date.now() });
    },
    { urls: ["<all_urls>"], types: ["main_frame", "sub_frame"] },
  );
}

// Pages the browser loaded in a tab or frame (two minutes), by address (the listener below).
const loadedPages = new Map();

// The address a download was first asked for: the browser's own (Chromium), else the start of
// the redirects that led to it. Chromium also counts the page a script or a meta refresh left
// from as a download's start (#374: a thank-you page setting location.href to the installer);
// that page isn't the file, and asking it again would save the page under the file's name.
function firstUrl(item) {
  if (item.finalUrl && item.finalUrl !== item.url) {
    const page = loadedPages.get(item.url);
    return page && Date.now() - page.at < 120000 ? item.finalUrl : item.url;
  }
  const r = redirected.get(item.url);
  return r && Date.now() - r.at < 120000 ? r.from : item.url;
}

// Links clicked with Alt held (page.js): the browser keeps their download.
const bypass = new Map();

// ---- the media a tab loaded (#38) ------------------------------------------------------------

// What the tab's pages loaded that can be downloaded, per tab, in the order seen: streams'
// manifests (HLS .m3u8, DASH .mpd; a master playlist comes before its variants) and plain video
// and sound files (a <video>'s or <audio>'s own file, or a large whole file a player fetched). The
// popup lists them (`mediaList`); the download button over a video sends the stream its frame
// loaded first (`streamOf`): a player that plays from a blob: has no address of its own, and
// yt-dlp reads the manifest (qualities, separate audio). Segments of a manifest already listed are
// pieces of that stream and stay out of the list. A transport stream or a separate audio file with
// no such manifest is listed (one video and one audio per folder, so a film of many segments does
// not fill the list). Bounded: MEDIA_MAX per tab, MEDIA_TABS tabs; forgotten when the tab goes
// to another page or closes.
const media = new Map(); // tab id -> {page, items: [{url, kind, frameId, mime, size, info}], frames}
const MEDIA_MAX = 60;
const MEDIA_TABS = 50;
const URL_MAX = 8192;
// A player's own fetch of a whole file is listed from this size (smaller ones are pieces).
const FETCHED_MIN = 1 << 20;
// Less than this of a single file, played by the page itself: a page's sound effect (mediaKind).
const SOUND_MIN = 64 << 10;
const MANIFEST = /\.(m3u8?|mpd)(?:[?#]|$)/i;
const SEGMENT_TYPE = /^(video\/mp2t|video\/iso\.segment|audio\/iso\.segment)$/i;
const SEGMENT_EXT = /\.(ts|m4s|m4f|cmfv|cmfa|cmft)$/i;
const MEDIA_EXT =
  /\.(mp4|m4v|m4b|m4p|webm|mkv|mk3d|mka|mov|moov|qt|avi|wmv|wm|asf|flv|f4v|3gp|3g2|3gp2|3gpp|3gpp2|ogv|ogm|ogx|m1v|m2p|m2t|m2ts|m2v|mts|mpg|mpg2|mpeg|mpeg1|mpeg4|mpe|mp2|mp2v|mp4v|mpv|mpv2|mqv|divx|xvid|vob|vro|dat|hdmov|bik|braw|dv|dvr|dvr-ms|evo|flc|fli|gvi|ifo|ism|isma|ismv|ivf|mj2|mjp|mjpg|mmv|mod|movie|mtv|mve|mxf|nsv|nut|nuv|pva|rec|rm|rmd|rmv|rmvb|roq|rv|sec|smk|smv|str|svi|swf|tod|tp|tp0|trp|vc1|vdo|vid|viv|vivo|wtv|y4m|264|h264|avc|mp3|m4a|m4r|aac|ac3|aif|aiff|ape|au|caf|dts|ogg|oga|opus|flac|ra|tta|wav|wma|wv)$/i;
const AUDIO_EXT = /^(mp3|m4a|m4r|aac|ac3|aif|aiff|ape|au|caf|dts|mka|oga|ogg|opus|flac|ra|tta|wav|wma|wv)$/i;
const SUBTITLE_EXT = /\.(vtt|srt|ass|ssa|ttml|dfxp|sbv)$/i;
const SUBTITLE_TYPE = /^(text\/vtt|text\/srt|application\/x-subrip|application\/ttml\+xml|text\/x-ssa)$/i;
const SUBTITLE_MAX = 30;
const SUBTITLE_MAX_BYTES = 4 << 20;
// The types a manifest is sometimes served as, besides its own (#171): plain text, no type, a
// file's, XML. Such an answer is only noted (`maybe`, MAYBE_MAX a tab), never listed unread.
const MAYBE_TYPE = /^(text\/plain|(application|binary)\/octet-stream|(application|text)\/xml|)$/;
const MAYBE_MAX = 20;
// A manifest's own types: HLS's (Apple's and the x- ones), DASH's (and its older video/ name).
const MANIFEST_TYPE = /mpegurl|dash\+xml|dash\.mpd/;
const MANIFEST_MAX = 2 << 20;
// An answer with neither a type nor an extension this big is a segment, not a playlist: the
// longest playlists a site serves stay well under it (`mediaKind`).
const OPAQUE_MEDIA = 512 << 10;

const pathOf = (url) => url.split(/[?#]/)[0];
const headerOf = (d, name) => ((d.responseHeaders || []).find((h) => h.name.toLowerCase() === name) || {}).value || "";

// The whole file's size: a range's total, else the length of a whole answer (0 when the server
// didn't say, as with chunked MPEG-TS).
function responseSize(d) {
  const range = headerOf(d, "content-range");
  const total = /\/\s*(\d+)\s*$/.exec(range);
  if (total) return Number(total[1]);
  if (d.statusCode === 206) return 0;
  return Number(headerOf(d, "content-length")) || 0;
}

// An MPEG-TS or fMP4 segment (.ts, .m4s, video/mp2t), or a fragment of a stream whose playlist
// was never seen (CMAF names its fragments .mp4 and .webm, `mediaKind`). A known playlist already
// covers these (`noteMedia`); with no playlist they are the film, which a grabber has to show.
// `piece` marks one entry per folder. A segment a player asked for in byte ranges (#38: an
// EXT-X-BYTERANGE playlist, or a player that ranges every fetch) is still a whole file at its own
// address, so it is listed with the size Content-Range gives (`responseSize`) and not dropped.
function pieceKind(d, mime, path) {
  const size = responseSize(d);
  if (size > 0 && size < 1024) return null;
  const audio = mime.startsWith("audio/") || /(?:^|[/?&_.-])audio(?:[=/_.-]|$)/i.test(d.url) || /\.(cmfa|m4a|aac)$/i.test(path);
  return { kind: audio ? "audio" : "video", mime, size, piece: true };
}

// What a response the tab got is, for the list: {kind: "hls" | "dash" | "video" | "audio", mime,
// size}, or null (not media, a segment of a known stream, a piece of a file, too small to be a
// file of its own). `piece` is a transport stream or a short audio file kept one-per-folder.
function mediaKind(d) {
  if (d.tabId < 0 || !/^https?:/i.test(d.url) || d.url.length > URL_MAX) return null;
  if (d.statusCode && (d.statusCode < 200 || d.statusCode > 299)) return null;
  const mime = headerOf(d, "content-type").split(";")[0].trim().toLowerCase();
  const path = pathOf(d.url).toLowerCase();
  // The address says manifest, the type says it isn't: a page or JSON that names one (e.g. a
  // switcher answering .../dash-full.mpd with JSON naming the real manifest; loaded first, it went
  // to the app and yt-dlp found no video in it, #377). Read before believed, like any `maybe`.
  if (/\.(m3u8?|mpd)$/.test(path) && !MANIFEST_TYPE.test(mime) && !MAYBE_TYPE.test(mime)) {
    return Number(headerOf(d, "content-length")) > MANIFEST_MAX ? null : { kind: "maybe", mime, size: 0 };
  }
  if (/\.m3u8?$/.test(path) || /mpegurl/.test(mime)) return { kind: "hls", mime, size: 0 };
  if (/\.mpd$/.test(path) || /dash\+xml/.test(mime)) return { kind: "dash", mime, size: 0 };
  if (
    SUBTITLE_TYPE.test(mime) ||
    (SUBTITLE_EXT.test(path) && (!mime || /^(text\/|application\/(?:octet-stream|xml|x-subrip|ttml\+xml))/.test(mime)))
  ) {
    const size = responseSize(d);
    if (size > SUBTITLE_MAX_BYTES) return null;
    return { kind: "subtitle", mime, size };
  }
  if (SEGMENT_TYPE.test(mime) || SEGMENT_EXT.test(path)) return pieceKind(d, mime, path);
  const ext = (MEDIA_EXT.exec(path) || [])[1] || "";
  // Maybe a manifest whose address and type don't say so (#171): a player's own small text or
  // untyped answer, read only when a stream is asked for and none is known (`sniffTab`). Past
  // OPAQUE_MEDIA it is too big to be a playlist and is taken for a segment instead: a CDN that
  // hands its segments out under a token ("/get?seg=5&tok=…") names neither a type nor an
  // extension, and nothing else in here would show that film.
  if (d.type !== "media" && !ext && MAYBE_TYPE.test(mime) && d.statusCode !== 206 && !headerOf(d, "content-range")) {
    const len = Number(headerOf(d, "content-length"));
    if (len > MANIFEST_MAX) return null;
    return len >= OPAQUE_MEDIA ? pieceKind(d, mime, path) : { kind: "maybe", mime, size: 0 };
  }
  // A page's own short sound (a click, a voice-search chime: 6 kB on YouTube in Edge), not media
  // anyone came for: the list would show nothing else on a page whose video is streamed.
  const whole = Number(headerOf(d, "content-length"));
  if (d.type === "media" && whole > 0 && whole < SOUND_MIN) return null;
  const typed = /^(video|audio)\//.test(mime) || mime === "application/ogg";
  const untyped = !mime || /^(application|binary)\/octet-stream$/.test(mime);
  if (!typed && !(ext && untyped)) return null;
  const range = headerOf(d, "content-range");
  const size = responseSize(d);
  const queryRanged = /[?&](range|bytestart|byterange)=/i.test(d.url);
  const ranged = d.statusCode === 206 || Boolean(range) || queryRanged;
  // A <video>'s or <audio>'s own file comes in ranges; a player's fetch of a range, or of a
  // small video file, is a piece of a stream. Whole-file ranged audio fetches (`Content-Range`
  // with total >= SOUND_MIN and no query byte-slice) are listed as audio (e.g. web music/podcast
  // players); DRM-protected ones are marked Protected in `mediaList`.
  const audio = mime.startsWith("audio/") || (!mime.startsWith("video/") && AUDIO_EXT.test(ext));
  if (d.type !== "media" && ranged) {
    if (audio && !queryRanged && size >= SOUND_MIN) return { kind: "audio", mime, size };
    return null;
  }
  if (d.type !== "media" && size < FETCHED_MIN) {
    const namedAudio = mime.startsWith("audio/") || AUDIO_EXT.test(ext);
    if (audio && (size >= SOUND_MIN || (size === 0 && namedAudio))) return { kind: "audio", mime, size, piece: true };
    // A fragment of a stream with no playlist of its own: CMAF serves them as .mp4 and .webm, so
    // neither SEGMENT_EXT nor SEGMENT_TYPE names them and nothing else would list the film.
    // Under SOUND_MIN it is a page's own short clip or sound, not media anyone came for.
    if (size === 0 || size >= SOUND_MIN) return pieceKind(d, mime, path);
    return null;
  }
  return { kind: audio ? "audio" : "video", mime, size };
}

const isStream = (it) => it.kind === "hls" || it.kind === "dash";
// The folder a manifest's pieces are in ("https://cdn/v/"), or null for a site's root (every
// file of the site would be its piece).
function folderOf(url) {
  const p = pathOf(url);
  const dir = p.slice(0, p.lastIndexOf("/") + 1);
  return /^https?:\/\/[^/]+\/.+/i.test(dir) ? dir : null;
}

function capturedRefererFor(url) {
  if (!url) return "";
  const hit = requestReferrers.get(url);
  if (hit && Date.now() - hit.at <= 3600000 && hit.referer) return hit.referer;
  const dir = folderOf(url);
  if (dir) {
    for (const [u, v] of requestReferrers) {
      if (Date.now() - v.at <= 3600000 && u.startsWith(dir) && v.referer) return v.referer;
    }
  }
  return "";
}

function capturedOriginFor(url) {
  if (!url) return "";
  const hit = requestReferrers.get(url);
  if (hit && Date.now() - hit.at <= 3600000 && hit.origin) return hit.origin;
  const dir = folderOf(url);
  if (dir) {
    for (const [u, v] of requestReferrers) {
      if (Date.now() - v.at <= 3600000 && u.startsWith(dir) && v.origin) return v.origin;
    }
  }
  return "";
}

function snapshotKey(url) {
  return String(url || "").split("#")[0];
}

function snapshotFor(url) {
  const key = snapshotKey(url);
  if (!key) return "";
  const hit = manifestSnapshots.get(key);
  if (hit && Date.now() - hit.at <= MANIFEST_SNAPSHOT_TTL && typeof hit.text === "string" && hit.text) {
    return hit.text;
  }
  return "";
}

function rememberSnapshot(url, text, kind, tabId, frameId) {
  const key = snapshotKey(url);
  if (!key) return;
  forget(manifestSnapshots, MANIFEST_SNAPSHOT_TTL);
  manifestSnapshots.delete(key);
  while (manifestSnapshots.size >= MANIFEST_SNAPSHOT_MAX) {
    const oldest = manifestSnapshots.keys().next().value;
    if (oldest === undefined) break;
    manifestSnapshots.delete(oldest);
  }
  manifestSnapshots.set(key, { text, kind, tabId, frameId, at: Date.now() });
}

function hlsSnapshotExtras(src, tabId) {
  if (!src) return {};
  const srcKey = snapshotKey(src);
  const main = snapshotFor(srcKey);
  const extras = {};
  if (main && main.startsWith("#EXTM3U") && main.length <= 1024 * 1024) {
    extras.hlsManifest = main;
  }
  const siblings = {};
  let count = 0;
  let totalBytes = 0;
  for (const [u, entry] of manifestSnapshots) {
    if (count >= 12) break;
    if (u === srcKey || Date.now() - entry.at > MANIFEST_SNAPSHOT_TTL) continue;
    if (tabId >= 0 && entry.tabId >= 0 && entry.tabId !== tabId) continue;
    if (entry.kind !== "hls" || typeof entry.text !== "string" || !entry.text.startsWith("#EXTM3U")) continue;
    if (totalBytes + entry.text.length > 2 * 1024 * 1024) continue;
    siblings[u] = entry.text;
    totalBytes += entry.text.length;
    count++;
  }
  if (count > 0) extras.hlsSnapshots = siblings;
  return extras;
}

function hasCapturedHlsPlaylist(it) {
  if (!it || it.kind !== "hls") return false;
  const info = it.info && !it.info.failed ? it.info : null;
  if (info && Array.isArray(info.childUrls) && info.childUrls.length) {
    const hasVideo = info.childUrls.some((u) => Boolean(snapshotFor(u)));
    if (!hasVideo) return false;
    if (Array.isArray(info.audioTracks) && info.audioTracks.length) {
      return info.audioTracks.some((a) => a && a.uri && Boolean(snapshotFor(a.uri)));
    }
    return true;
  }
  return Boolean(snapshotFor(it.url));
}

function noteMedia(d, found) {
  const tab = media.get(d.tabId) || { page: null, items: [] };
  const old = tab.items.find((it) => it.url === d.url);
  if (old) {
    if (found.size && !old.size) old.size = found.size;
    if (found.label && !old.label) old.label = found.label;
    if (found.lang && !old.lang) old.lang = found.lang;
    return;
  }
  // A file in a stream's own folder is one of its pieces (a DASH init segment, a fragment). It is
  // listed too, so the popup shows every kind the page loaded and not the manifest alone, but as a
  // `piece`: one entry per folder stands for the many segments of one film. Subtitles stay separate
  // so multiple language tracks in the same folder remain individually downloadable.
  const isSub = found.kind === "subtitle";
  const inStream = (it) => isStream(it) && folderOf(it.url) && d.url.startsWith(folderOf(it.url));
  if (!isStream(found) && !isSub && tab.items.some(inStream)) found = { ...found, piece: true };
  // Many segments of one film: keep the largest of each kind in that folder, not every piece.
  if (found.piece) {
    const dir = folderOf(d.url) || pathOf(d.url);
    const same = tab.items.find((it) => it.piece && it.kind === found.kind && (folderOf(it.url) || pathOf(it.url)) === dir);
    if (same) {
      if ((found.size || 0) > (same.size || 0)) {
        same.url = d.url;
        same.mime = found.mime;
        same.size = found.size;
        same.tabId = d.tabId;
        same.frameId = d.frameId || 0;
      }
      keepMedia();
      return;
    }
  }
  if (isSub) {
    const subCount = tab.items.filter((it) => it.kind === "subtitle").length;
    if (subCount >= SUBTITLE_MAX) {
      const oldestSub = tab.items.findIndex((it) => it.kind === "subtitle");
      if (oldestSub >= 0) tab.items.splice(oldestSub, 1);
    }
  }
  tab.items.push({
    url: d.url,
    kind: found.kind,
    tabId: d.tabId,
    frameId: d.frameId || 0,
    mime: found.mime,
    size: found.size,
    ...(found.piece ? { piece: true } : {}),
    ...(found.label ? { label: found.label } : {}),
    ...(found.lang ? { lang: found.lang } : {}),
  });
  // The playlist arrived after its segments: they are its pieces now, and stay in the list as
  // such (one per folder), the manifest above them.
  if (isStream(found)) {
    const dir = folderOf(d.url);
    if (dir) for (const it of tab.items) if (!isStream(it) && it.kind !== "subtitle" && it.url.startsWith(dir)) it.piece = true;
  }
  // Over the cap: the oldest go, but each frame's first stream stays (the button sends it).
  while (tab.items.length > MEDIA_MAX) {
    const firsts = new Set();
    const i = tab.items.findIndex((it) => {
      if (!isStream(it) || firsts.has(it.frameId)) return true;
      firsts.add(it.frameId);
      return false;
    });
    tab.items.splice(Math.max(i, 0), 1);
  }
  // The tab seen last goes last; the least recent ones go past MEDIA_TABS.
  media.delete(d.tabId);
  media.set(d.tabId, tab);
  for (const id of media.keys()) {
    if (media.size <= MEDIA_TABS) break;
    media.delete(id);
  }
  keepMedia();
  if (isStream(found)) {
    const lastItem = tab.items[tab.items.length - 1];
    if (lastItem && !lastItem.info) {
      probe(lastItem).then((inf) => {
        if (inf && !inf.failed) {
          lastItem.info = inf;
          keepMedia();
        }
      }).catch(() => {});
    }
  }
}

// A response that may be a manifest (#171), noted apart: the newest MAYBE_MAX of the tab.
function noteMaybe(d, found) {
  const tab = media.get(d.tabId) || { page: null, items: [] };
  if (tab.items.some((it) => it.url === d.url)) return;
  tab.maybe = (tab.maybe || []).filter((m) => m.url !== d.url);
  tab.maybe.push({ url: d.url, frameId: d.frameId || 0, mime: found.mime });
  if (tab.maybe.length > MAYBE_MAX) tab.maybe.splice(0, tab.maybe.length - MAYBE_MAX);
  if (!media.has(d.tabId)) {
    media.set(d.tabId, tab);
    for (const id of media.keys()) {
      if (media.size <= MEDIA_TABS) break;
      media.delete(id);
    }
  }
  keepMedia();
}

// What a text starts as: an HLS playlist ("hls"), a DASH manifest ("dash"), or neither ("").
function manifestKind(text) {
  const t = String(text).replace(/^﻿/, "").trimStart();
  if (t.startsWith("#EXTM3U")) return "hls";
  // An XML declaration, comments, then the MPD element (with a namespace prefix or not).
  if (/^(?:<\?xml[^>]*\?>\s*)?(?:<!--[\s\S]*?-->\s*)*<(?:[\w-]+:)?MPD[\s>/]/.test(t)) return "dash";
  return "";
}

// Executed inside the player's own frame when a background fetch is refused (e.g. a CDN that
// returns 403/404 without the frame's exact Referer/Origin headers).
async function frameFetchResource(url, maxBytes, binary) {
  let reader = null;
  try {
    // Use same-origin first so CORS wildcard (`Access-Control-Allow-Origin: *`) CDNs succeed
    // without credentialed-CORS rejection; escalate to `include` only on 401/403.
    let r = await fetch(url, { credentials: "same-origin" });
    if (r.status === 401 || r.status === 403) {
      try {
        r = await fetch(url, { credentials: "include" });
      } catch {
        return { xdmFrameFetch: true, ok: false, status: r.status };
      }
    }
    if (!r.ok) return { xdmFrameFetch: true, ok: false, status: r.status };
    const cl = Number(r.headers.get("content-length")) || 0;
    const canStreamTextHead = Boolean(r.body && r.body.getReader && maxBytes && maxBytes <= 16384);
    if (maxBytes && cl > maxBytes && !binary && !canStreamTextHead) return { xdmFrameFetch: true, ok: false, status: 413 };
    if (binary) {
      const cap = maxBytes || 564;
      const range = r.headers.get("content-range") || "";
      const rangeTotal = /\/\s*(\d+)\s*$/.exec(range);
      const totalBytes = rangeTotal ? Number(rangeTotal[1]) : cl;
      if (r.body && r.body.getReader) {
        reader = r.body.getReader();
        const chunks = [];
        let total = 0;
        while (total < cap) {
          const { done, value } = await reader.read();
          if (done) break;
          chunks.push(value);
          total += value.byteLength;
        }
        const out = new Uint8Array(Math.min(total, cap));
        let at = 0;
        for (const c of chunks) {
          const take = Math.min(c.byteLength, out.length - at);
          out.set(c.subarray(0, take), at);
          at += take;
          if (at >= out.length) break;
        }
        return { xdmFrameFetch: true, ok: true, status: r.status, bytes: Array.from(out), totalBytes };
      }
      const ab = await r.arrayBuffer();
      return {
        xdmFrameFetch: true,
        ok: true,
        status: r.status,
        bytes: Array.from(new Uint8Array(ab.slice(0, cap))),
        totalBytes: totalBytes || ab.byteLength,
      };
    }
    if (canStreamTextHead) {
      reader = r.body.getReader();
      const dec = new TextDecoder();
      let text = "";
      while (text.length < maxBytes) {
        const { done, value } = await reader.read();
        if (done) break;
        text += dec.decode(value, { stream: true });
      }
      return { xdmFrameFetch: true, ok: true, status: r.status, text: text.slice(0, maxBytes) };
    }
    const text = await r.text();
    if (maxBytes && text.length > maxBytes) return { xdmFrameFetch: true, ok: false, status: 413 };
    return { xdmFrameFetch: true, ok: true, status: r.status, text };
  } catch {
    return { xdmFrameFetch: true, ok: false, status: 0 };
  } finally {
    if (reader) reader.cancel().catch(() => {});
  }
}

async function inFrameFetchRaw(tabId, frameId, url, maxBytes, binary = false) {
  if (!(tabId >= 0) || !ext.scripting || !ext.scripting.executeScript) return null;
  try {
    const [found] = await withTimeout(
      ext.scripting.executeScript({
        target: { tabId, frameIds: [frameId || 0] },
        func: frameFetchResource,
        args: [url, maxBytes || 0, Boolean(binary)],
      }),
      5000,
    );
    const res = found && found.result;
    return res && res.xdmFrameFetch === true ? res : null;
  } catch {
    return null;
  }
}

async function inFrameFetch(tabId, frameId, url, maxBytes, binary = false) {
  const res = await inFrameFetchRaw(tabId, frameId, url, maxBytes, binary);
  return res && res.ok ? res : null;
}

function shouldPreferFrameFetch(url, tabId, frameId) {
  return tabId >= 0 && (frameId !== 0 || Boolean(capturedRefererFor(url)));
}

function isTransientStatus(status) {
  return status === 0 || status === 408 || status === 425 || status === 429 || status >= 500;
}

// The start of a response (SNIFF_BYTES), fetched again with the site's cookies (or inside the
// player's frame first when the request came from an embed frame or carried a Referer):
// `{ kind, definite }` so transient fetch errors don't discard candidates permanently.
const SNIFF_BYTES = 4096;
async function sniffDetailed(url, tabId = -1, frameId = 0) {
  const cached = snapshotFor(url);
  if (cached) {
    const kind = manifestKind(cached);
    if (kind) return { kind, definite: true };
  }
  const preferFrame = shouldPreferFrameFetch(url, tabId, frameId);
  let frameRes = null;
  if (preferFrame) {
    frameRes = await inFrameFetchRaw(tabId, frameId, url, SNIFF_BYTES, false);
    if (frameRes && frameRes.ok && typeof frameRes.text === "string") {
      return { kind: manifestKind(frameRes.text), definite: true };
    }
  }
  let reader = null;
  let bgStatus = 0;
  let bgTextRead = false;
  let bgThrew = false;
  try {
    const r = await withTimeout(fetch(url, { credentials: "include" }), 5000);
    bgStatus = r.status || (r.ok ? 200 : 0);
    if (r.ok) {
      let text = "";
      if (!r.body || !r.body.getReader) {
        text = (await withTimeout(r.text(), 5000)).slice(0, SNIFF_BYTES);
      } else {
        reader = r.body.getReader();
        const dec = new TextDecoder();
        while (text.length < SNIFF_BYTES) {
          const { done, value } = await withTimeout(reader.read(), 5000);
          if (done) break;
          text += dec.decode(value, { stream: true });
        }
      }
      bgTextRead = true;
      const kind = manifestKind(text);
      if (kind) return { kind, definite: true };
    }
  } catch {
    // Fall through to in-frame fetch when background fetch is blocked or 404s without Referer.
    bgThrew = true;
  } finally {
    if (reader) reader.cancel().catch(() => {});
  }
  if (!preferFrame && tabId >= 0) {
    frameRes = await inFrameFetchRaw(tabId, frameId, url, SNIFF_BYTES, false);
    if (frameRes && frameRes.ok && typeof frameRes.text === "string") {
      return { kind: manifestKind(frameRes.text), definite: true };
    }
  }
  if (bgTextRead) return { kind: "", definite: true };
  const frameTransient = frameRes ? isTransientStatus(frameRes.status) : tabId >= 0;
  const bgTransient = bgThrew || isTransientStatus(bgStatus);
  return { kind: "", definite: !frameTransient && !bgTransient };
}

async function sniff(url, tabId = -1, frameId = 0) {
  return (await sniffDetailed(url, tabId, frameId)).kind;
}

// The tab's possible manifests read, those of `frames` only (a Set of frame ids, or null: all),
// each once when answered definitively: the ones that are become its streams, in the order they were loaded. One reading at
// a time per tab: a second ask waits for the first.
const sniffing = new Map();
function sniffTab(tabId, frames) {
  const run = async () => {
    const tab = media.get(tabId);
    if (!tab || !tab.maybe || !tab.maybe.length) return;
    const now = Date.now();
    const asked = tab.maybe.filter((m) => (!frames || frames.has(m.frameId)) && (!m.lastTry || now - m.lastTry >= 1200));
    if (!asked.length) return;
    tab.maybe = tab.maybe.filter((m) => !asked.includes(m));
    const results = await Promise.all(asked.map((m) => sniffDetailed(m.url, tabId, m.frameId)));
    // The tab went to another page meanwhile: its old page's answers don't count.
    if (media.get(tabId) !== tab) return;
    const seen = new Set(tab.maybe.map((m) => m.url));
    asked.forEach((m, i) => {
      const res = results[i] || { kind: "", definite: true };
      if (res.kind) {
        noteMedia({ tabId, url: m.url, frameId: m.frameId }, { kind: res.kind, mime: m.mime, size: 0 });
      } else if (!res.definite && (m.tries || 0) < 3 && !seen.has(m.url)) {
        m.tries = (m.tries || 0) + 1;
        m.lastTry = Date.now();
        tab.maybe.push(m);
        seen.add(m.url);
      }
    });
    keepMedia();
  };
  const next = (sniffing.get(tabId) || Promise.resolve()).then(run, run);
  sniffing.set(tabId, next);
  next.finally(() => sniffing.get(tabId) === next && sniffing.delete(tabId)).catch(() => {});
  return next;
}

if (ext.webRequest && ext.webRequest.onResponseStarted) {
  ext.webRequest.onResponseStarted.addListener(
    (d) => {
      if (d.type === "main_frame" || d.type === "sub_frame") {
        const type = (d.responseHeaders || []).find((h) => h.name.toLowerCase() === "content-type");
        if (d.statusCode === 200 && type && /^\s*(text\/html|application\/xhtml)/i.test(type.value || "")) {
          if (d.type === "main_frame") {
            forget(loadedPages, 120000);
            loadedPages.set(d.url, { at: Date.now() });
            if (d.tabId >= 0) {
              mediaBack.then(() => {
                media.set(d.tabId, { page: String(d.url || "").split("#")[0], items: [] });
                keepMedia();
              });
            }
          } else if (d.tabId >= 0 && (d.frameId || 0) > 0) {
            mediaBack.then(() => {
              const tab = media.get(d.tabId);
              if (!tab) return;
              const fId = d.frameId || 0;
              let changed = false;
              if (Array.isArray(tab.items)) {
                const next = tab.items.filter((it) => it.frameId !== fId);
                if (next.length !== tab.items.length) {
                  tab.items = next;
                  changed = true;
                }
              }
              if (Array.isArray(tab.maybe)) {
                const nextMaybe = tab.maybe.filter((m) => m.frameId !== fId);
                if (nextMaybe.length !== tab.maybe.length) {
                  tab.maybe = nextMaybe;
                  changed = true;
                }
              }
              if (changed) keepMedia();
            });
          }
        }
        return;
      }
      const found = mediaKind(d);
      if (found) mediaBack.then(() => (found.kind === "maybe" ? noteMaybe(d, found) : noteMedia(d, found)));
    },
    { urls: ["<all_urls>"], types: ["main_frame", "sub_frame", "xmlhttprequest", "media", "other"] },
    ["responseHeaders"],
  );
}
ext.tabs.onRemoved.addListener((id) => {
  clearDrmForTab(id);
  mediaBack.then(() => media.delete(id) && keepMedia());
});
// Another page in the tab: what the last one loaded goes (not for a move within the page, #part).
ext.tabs.onUpdated.addListener((id, change) => {
  if (!change.url) return;
  mediaBack.then(() => {
    const page = change.url.split("#")[0];
    const tab = media.get(id);
    if (tab && tab.page === page) return;
    clearDrmForTab(id);
    media.set(id, { page, items: [] });
    keepMedia();
  });
});

// Kept in the session's storage (memory only, gone with the browser): the browser stops this
// script when idle, and a tab's player loaded its stream long before it is asked for. Read back
// once at start, merged with what was seen meanwhile; everything else waits for it.
const session = ext.storage.session;
let keeping = null;
function keepMedia() {
  if (!session || keeping) return;
  keeping = setTimeout(() => {
    keeping = null;
    session.set({ media: [...media] }).catch(() => {});
  }, 200);
}
const mediaBack = session
  ? session
      .get({ media: [] })
      .then((v) => {
        for (const [id, kept] of v.media || []) {
          const now = media.get(id);
          const fresh = ((now && now.items) || []).filter((s) => !kept.items.some((o) => o.url === s.url));
          const maybe = [...(kept.maybe || []), ...((now && now.maybe) || [])].filter((m, i, all) => all.findIndex((o) => o.url === m.url) === i);
          media.set(id, { ...kept, page: (now && now.page) || kept.page, items: [...kept.items, ...fresh].slice(-MEDIA_MAX), maybe: maybe.slice(-MAYBE_MAX) });
        }
      })
      .catch(() => {})
  : Promise.resolve();

// The stream a frame's player loaded: prefer a master playlist (with child variants/audio)
// over an individual child media/audio playlist, keeping frame affinity first.
function needsProbe(it) {
  if (!it || !isStream(it)) return false;
  if (!it.info) return true;
  return Boolean(it.info.failed && it.info.at && Date.now() - it.info.at >= 2500);
}

function playerStreamInFrame() {
  const okStream = (u) =>
    typeof u === "string" &&
    /^https?:/i.test(u) &&
    !/(?:^|\/)ad[s_/-]|doubleclick|googlesyndication/i.test(u) &&
    (/(\.m3u8|\.mpd)(\?|#|$)/i.test(u) ||
      (/\.txt(\?|#|$)/i.test(u) && /\/(hls|stream|manifest|master|playlist|sublist|index|chunklist)/i.test(u)));
  try {
    const jw = typeof globalThis.jwplayer === "function" ? globalThis.jwplayer() : null;
    const pl = jw && typeof jw.getPlaylist === "function" ? jw.getPlaylist() : null;
    if (Array.isArray(pl)) {
      for (const item of pl) {
        if (!item) continue;
        if (okStream(item.file)) return { xdmPlayerStream: true, url: item.file };
        if (Array.isArray(item.sources)) {
          for (const s of item.sources) {
            if (s && okStream(s.file || s.src)) return { xdmPlayerStream: true, url: s.file || s.src };
          }
        }
      }
    }
  } catch {
    // Player object inaccessible.
  }
  try {
    if (globalThis.videojs && typeof globalThis.videojs.getPlayers === "function") {
      for (const p of Object.values(globalThis.videojs.getPlayers() || {})) {
        const u = p && typeof p.currentSrc === "function" ? p.currentSrc() : "";
        if (okStream(u)) return { xdmPlayerStream: true, url: u };
      }
    }
  } catch {
    // Ignore videojs lookup errors.
  }
  try {
    if (globalThis.Hls && Array.isArray(globalThis.Hls.instances)) {
      for (const inst of globalThis.Hls.instances) {
        const u = inst && inst.url;
        if (okStream(u)) return { xdmPlayerStream: true, url: u };
      }
    }
  } catch {
    // Ignore Hls lookup errors.
  }
  for (const el of document.querySelectorAll("video, audio, source")) {
    const u = el.currentSrc || el.src || el.dataset?.src || "";
    if (okStream(u)) return { xdmPlayerStream: true, url: u };
  }
  return { xdmPlayerStream: true, url: "" };
}

function streamOf(sender) {
  const list = ((sender.tab && media.get(sender.tab.id)) || { items: [] }).items.filter(isStream);
  if (!list.length) return "";
  const isChildOfAny = (s) => {
    const p = pathOf(s.url);
    return list.some((other) => other !== s && other.info && !other.info.failed && Array.isArray(other.info.children) && other.info.children.includes(p));
  };
  const rank = (s, i) => {
    const frameMatch = s.frameId === sender.frameId ? 1000 : 0;
    const probedOk = s.info && !s.info.failed ? 200 : 0;
    const isMaster = s.info && !s.info.failed && Array.isArray(s.info.children) && s.info.children.length > 0 ? 100 : 0;
    const notChild = !isChildOfAny(s) ? 50 : 0;
    return frameMatch + probedOk + isMaster + notChild + (isMaster ? i * 0.001 : -i * 0.001);
  };
  let best = list[0];
  let bestScore = rank(best, 0);
  for (let i = 1; i < list.length; i++) {
    const sc = rank(list[i], i);
    if (sc > bestScore) {
      best = list[i];
      bestScore = sc;
    }
  }
  return best.url || "";
}

// The same for a player that plays from a blob: (asked by the button over it or the right-click
// menu), reading the tab's possible manifests when no stream is known (#171): the frame's own
// first, then inspecting the frame's Performance resources & MAIN-world active player config if a
// worker restart wiped `media`, and finally the tab's other frames. Also probes any un-inspected
// stream manifests in the tab so `streamOf` can distinguish master playlists from child variants.
async function streamFor(sender) {
  const tabId = sender.tab && sender.tab.id;
  if (tabId === undefined || tabId === null) return streamOf(sender);
  const frameId = sender.frameId || 0;
  const own = () => ((media.get(tabId) || { items: [] }).items.some((it) => isStream(it) && it.frameId === frameId));
  if (!own()) await sniffTab(tabId, new Set([frameId]));
  if (!own() && ext.scripting && ext.scripting.executeScript) {
    try {
      const found = await withTimeout(
        ext.scripting.executeScript({ target: { tabId, frameIds: [frameId] }, func: mediaElements }),
        2000,
      );
      const frames = {};
      for (const f of found || []) if (f && f.result) frames[f.frameId ?? frameId] = f.result;
      if (Object.keys(frames).length) {
        noteFrameLoads(tabId, frames);
        await sniffTab(tabId, new Set([frameId]));
      }
    } catch {
      // Frame resource scan is best-effort.
    }
  }
  if (!own() && ext.scripting && ext.scripting.executeScript) {
    try {
      const found = await withTimeout(
        ext.scripting.executeScript({ target: { tabId, frameIds: [frameId] }, world: "MAIN", func: playerStreamInFrame }),
        2000,
      );
      const res = found && found[0] && found[0].result;
      const u = res && res.xdmPlayerStream === true && typeof res.url === "string" ? res.url : "";
      if (u) {
        const k = kindFromUrl(u);
        if (k && k.kind === "maybe") {
          noteMaybe({ tabId, url: u, frameId }, k);
          await sniffTab(tabId, new Set([frameId]));
        } else if (k && isStream(k)) {
          noteMedia({ tabId, url: u, frameId }, k);
        }
      }
    } catch {
      // MAIN-world player inspection is best-effort.
    }
  }
  if (!own() && !streamOf(sender)) await sniffTab(tabId, null);
  if (!own() && !streamOf(sender) && ext.scripting && ext.scripting.executeScript) {
    try {
      const found = await withTimeout(
        ext.scripting.executeScript({ target: { tabId, allFrames: true }, func: mediaElements }),
        2000,
      );
      const frames = {};
      for (const f of found || []) if (f && f.result) frames[f.frameId || 0] = f.result;
      if (Object.keys(frames).length) {
        noteFrameLoads(tabId, frames);
        await sniffTab(tabId, null);
      }
    } catch {
      // Cross-frame resource scan is best-effort.
    }
  }
  const tab = media.get(tabId);
  const unprobed = tab ? tab.items.filter(needsProbe) : [];
  if (unprobed.length) {
    await Promise.all(unprobed.map(async (it) => (it.info = await probe(it).catch(() => ({ failed: true, at: Date.now() })))));
    keepMedia();
  }
  return streamOf(sender);
}

// ---- a stream's manifest: its qualities, length, and whether it is protected -------------------

// An HLS tag's attributes (KEY=value,KEY="value").
function hlsAttrs(s) {
  const out = {};
  for (const [, k, v] of s.matchAll(/([A-Z0-9-]+)=("[^"]*"|[^,]*)/g)) out[k] = v.replace(/^"|"$/g, "");
  return out;
}

// A copy-protected (DRM) key: a key system's (Apple's FairPlay, Google's Widevine, Microsoft's
// PlayReady), or SAMPLE-AES, which no download tool decrypts. AES-128 with a plain key
// ("identity") is not DRM: yt-dlp fetches its key as the player does.
function drmKey(a) {
  const method = (a.METHOD || "").toUpperCase();
  if (!method || method === "NONE") return false;
  return method.startsWith("SAMPLE-AES") || (a.KEYFORMAT !== undefined && a.KEYFORMAT !== "identity");
}

// {heights, variants, duration, drm, live, children, childUrls}: `children`, the addresses
// (without their query) a master playlist names: its variants and renditions, the same stream.
function parseHls(text, base) {
  const lines = text.replace(/^﻿/, "").split(/\r?\n/).map((l) => l.trim());
  if (!lines[0].startsWith("#EXTM3U")) return null;
  const info = {
    heights: [],
    variants: [],
    audioTracks: [],
    duration: 0,
    drm: false,
    live: true,
    hasSeparateAudio: false,
    firstSegmentUrl: "",
    firstSegmentDuration: 0,
    children: [],
    childUrls: [],
  };
  const videoChildEntries = [];
  const otherChildUrls = [];
  const child = (uri, videoVariant = false, height = 0, bandwidth = 0) => {
    try {
      const full = new URL(uri, base).href;
      info.children.push(pathOf(full));
      if (videoVariant) videoChildEntries.push({ url: full, height, bandwidth });
      else otherChildUrls.push(full);
      return full;
    } catch {
      return "";
    }
  };
  let afterInf = false;
  let lastInf = 0;
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];
    if (!l) continue;
    const value = l.slice(l.indexOf(":") + 1);
    if (l.startsWith("#EXT-X-STREAM-INF:")) {
      afterInf = false;
      const a = hlsAttrs(value);
      if (a.AUDIO) info.hasSeparateAudio = true;
      let h = Number((/x(\d+)$/.exec(a.RESOLUTION || "") || [])[1]) || 0;
      const bw = Number(a.BANDWIDTH) || Number(a["AVERAGE-BANDWIDTH"]) || 0;
      // BANDWIDTH is the variant's peak rate (RFC 8216 4.3.4.2): it ranks variants, but a size
      // from it came out up to twice the real file. AVERAGE-BANDWIDTH, when the playlist gives it,
      // is the mean the file is actually made of.
      const avg = Number(a["AVERAGE-BANDWIDTH"]) || 0;
      const uri = lines.slice(i + 1).find((x) => x && !x.startsWith("#"));
      if (!h && a.NAME) {
        h = Number((/(\d{3,4})p?/i.exec(a.NAME) || [])[1]) || 0;
      }
      if (!h && uri) {
        h = Number((/(?:^|[^\d])(1080|1036|900|720|540|480|360|240)p?(?:[^\d]|$)/i.exec(uri) || [])[1]) || 0;
      }
      if (!h && bw > 0) {
        if (bw >= 3500000) h = 1080;
        else if (bw >= 1800000) h = 720;
        else if (bw >= 800000) h = 480;
        else if (bw >= 400000) h = 360;
        else h = 240;
      }
      if (h) info.heights.push(h);
      if (h || bw) info.variants.push({ height: h || 0, bandwidth: bw, average: avg, codecs: a.CODECS || "", audioGroup: a.AUDIO || "" });
      if (uri) child(uri, true, h || 0, bw);
    } else if (l.startsWith("#EXT-X-MEDIA:") || l.startsWith("#EXT-X-I-FRAME-STREAM-INF:")) {
      const a = hlsAttrs(value);
      const isAudio = l.startsWith("#EXT-X-MEDIA:") && String(a.TYPE || "").toUpperCase() === "AUDIO";
      if (isAudio) info.hasSeparateAudio = true;
      const full = a.URI ? child(a.URI, false) : "";
      if (isAudio && full) {
        info.audioTracks.push({
          groupId: a["GROUP-ID"] || "",
          name: a.NAME || "",
          language: a.LANGUAGE || "",
          default: String(a.DEFAULT || "").toUpperCase() === "YES",
          autoselect: String(a.AUTOSELECT || "").toUpperCase() === "YES",
          uri: full,
        });
      }
    } else if (l.startsWith("#EXT-X-KEY:") || l.startsWith("#EXT-X-SESSION-KEY:")) {
      if (drmKey(hlsAttrs(value))) info.drm = true;
    } else if (l.startsWith("#EXTINF:")) {
      const segDur = parseFloat(value) || 0;
      info.duration += segDur;
      lastInf = segDur;
      afterInf = true;
    } else if (l.startsWith("#EXT-X-ENDLIST") || /^#EXT-X-PLAYLIST-TYPE:\s*VOD/.test(l)) {
      info.live = false;
    } else if (!l.startsWith("#") && afterInf) {
      afterInf = false;
      if (!info.firstSegmentUrl) {
        try {
          info.firstSegmentUrl = new URL(l, base).href;
          info.firstSegmentDuration = lastInf;
        } catch {
          // Ignore malformed URI.
        }
      }
    }
  }
  // Inspect highest-resolution video variant first so mixed fMP4 (e.g. 1080p AV1) + MPEG-TS (e.g. 480p H.264)
  // masters sniff the top variant's container instead of the lowest variant.
  videoChildEntries.sort((a, b) => (b.height - a.height) || (b.bandwidth - a.bandwidth));
  info.childUrls = [...videoChildEntries.map((e) => e.url), ...otherChildUrls];
  // A master playlist says nothing of an end: its variants do.
  if (info.children.length) info.live = false;
  return info;
}

// Video codecs that cannot be safely muxed into MPEG-TS (.ts) and require MP4/fMP4 output.
function needsMp4Container(codecs) {
  return /(?:^|[\s,])(av01|vp09|vp9|vp08|vp8|hvc1|hev1)\b/i.test(codecs || "");
}

// Pick the right container for a native HLS download based on the selected variant's codecs and
// sniffed segment bytes (never forcing `.ts` on AV1/VP9/HEVC fMP4 variants).
function hlsPreferredContainer(info, selector = "") {
  const segExt = (info && info.segmentExt) || "";
  const variants = info && Array.isArray(info.variants) ? info.variants : [];
  const sel = String(selector || "").toLowerCase();
  const hm = /(?:^hls-(?:avc-)?|height<=)(\d+)/.exec(sel);
  const wantedH = hm ? Number(hm[1]) : 0;
  const wantAvc = /vcodec\^=avc|^hls-avc\b/.test(sel);
  let pool = variants;
  if (wantAvc) {
    const avcOnly = variants.filter((v) => v && /(?:^|[\s,])avc[13]/i.test(v.codecs || ""));
    if (avcOnly.length) pool = avcOnly;
  }
  let chosen = null;
  if (wantedH > 0) {
    chosen =
      pool.filter((v) => v && Number(v.height) === wantedH).sort((a, b) => (b.bandwidth || 0) - (a.bandwidth || 0))[0] ||
      pool.filter((v) => v && Number(v.height) > 0 && Number(v.height) <= wantedH).sort((a, b) => (b.height || 0) - (a.height || 0) || (b.bandwidth || 0) - (a.bandwidth || 0))[0] ||
      null;
  }
  if (!chosen && pool.length) {
    chosen = [...pool].sort((a, b) => (Number(b.height) || 0) - (Number(a.height) || 0) || (Number(b.bandwidth) || 0) - (Number(a.bandwidth) || 0))[0];
  }
  if (chosen && needsMp4Container(chosen.codecs)) return "mp4";
  return segExt || "ts";
}

// Seconds of an ISO 8601 duration (PT1H2M3.5S).
function isoSeconds(d) {
  const m = /P(?:(\d+)D)?T?(?:(\d+)H)?(?:(\d+)M)?(?:([\d.]+)S)?/.exec(d || "") || [];
  return (+m[1] || 0) * 86400 + (+m[2] || 0) * 3600 + (+m[3] || 0) * 60 + (+m[4] || 0);
}

// A DASH manifest, read as text (a service worker has no XML parser). Any ContentProtection is
// encryption: Widevine, PlayReady, or CENC with a key the site gives its own player only.
function parseDash(text) {
  if (!/<(?:\w+:)?MPD[\s>]/.test(text)) return null;
  const attr = (tag, name) => (new RegExp(`\\s${name}\\s*=\\s*["']([^"']*)["']`).exec(tag) || [])[1];
  const mpd = (/<(?:\w+:)?MPD\b[^>]*>/.exec(text) || [""])[0];
  const heights = [];
  const variants = [];
  for (const [tag] of text.matchAll(/<(?:\w+:)?(?:Representation|AdaptationSet)\b[^>]*>/g)) {
    const h = Number(attr(tag, "height"));
    const bw = Number(attr(tag, "bandwidth")) || 0;
    if (h) heights.push(h);
    if (h || bw) {
      variants.push({
        height: h || 0,
        bandwidth: bw,
        mimeType: attr(tag, "mimeType") || "",
        codecs: attr(tag, "codecs") || "",
      });
    }
  }
  return {
    heights,
    variants,
    duration: isoSeconds(attr(mpd, "mediaPresentationDuration")),
    drm: /<(?:\w+:)?ContentProtection\b/.test(text),
    live: attr(mpd, "type") === "dynamic",
    children: [],
  };
}

// A manifest, fetched again by the extension (the player's copy can't be read): its details, or
// {failed: true}: a site that refuses the extension's request (it checks the page that asks) is
// listed without them.
const probesInFlight = new Map();

async function fetchManifestText(url, tabId = -1, frameId = 0, timeoutMs = 5000) {
  const cached = snapshotFor(url);
  if (cached && cached.length <= MANIFEST_MAX && manifestKind(cached)) {
    return cached;
  }
  const cacheAndReturn = (text) => {
    const kind = manifestKind(text);
    if (!kind) return null;
    rememberSnapshot(url, text, kind, tabId, frameId);
    return text;
  };
  const preferFrame = shouldPreferFrameFetch(url, tabId, frameId);
  if (preferFrame) {
    const viaFrame = await inFrameFetch(tabId, frameId, url, MANIFEST_MAX, false);
    if (viaFrame && typeof viaFrame.text === "string" && viaFrame.text.length <= MANIFEST_MAX && manifestKind(viaFrame.text)) {
      return cacheAndReturn(viaFrame.text);
    }
  }
  try {
    const r = await withTimeout(fetch(url, { credentials: "include" }), timeoutMs);
    if (r.ok && Number(r.headers.get("content-length")) <= MANIFEST_MAX) {
      const text = await withTimeout(r.text(), timeoutMs);
      if (text.length <= MANIFEST_MAX && manifestKind(text)) return cacheAndReturn(text);
      if (text.length > MANIFEST_MAX) return null;
    }
  } catch {
    // Fall back to in-frame fetch when background fetch fails or 404s on Referer-gated CDNs.
  }
  if (!preferFrame && tabId >= 0) {
    const viaFrame = await inFrameFetch(tabId, frameId, url, MANIFEST_MAX, false);
    if (viaFrame && typeof viaFrame.text === "string" && viaFrame.text.length <= MANIFEST_MAX && manifestKind(viaFrame.text)) {
      return cacheAndReturn(viaFrame.text);
    }
  }
  return null;
}

function detectContainerBytes(bytes) {
  if (!bytes || bytes.length < 8) return "";
  if (bytes.length >= 189 && bytes[0] === 0x47 && bytes[188] === 0x47) return "ts";
  const box = String.fromCharCode(bytes[4], bytes[5], bytes[6], bytes[7]);
  if (box === "ftyp" || box === "styp" || box === "moof" || box === "sidx") return "mp4";
  return "";
}

async function sniffDisguisedSegment(segUrl, tabId = -1, frameId = 0) {
  const preferFrame = shouldPreferFrameFetch(segUrl, tabId, frameId);
  if (preferFrame) {
    const viaFrame = await inFrameFetch(tabId, frameId, segUrl, 564, true);
    const detected = viaFrame && Array.isArray(viaFrame.bytes) ? detectContainerBytes(viaFrame.bytes) : "";
    const size = viaFrame && Number(viaFrame.totalBytes) > 0 ? Number(viaFrame.totalBytes) : 0;
    if (detected || size > 0) return { ext: detected, size };
  }
  let reader = null;
  try {
    const r = await withTimeout(fetch(segUrl, { credentials: "include" }), 2500);
    if (r.ok && r.body && r.body.getReader) {
      const range = r.headers.get("content-range") || "";
      const rangeTotal = /\/\s*(\d+)\s*$/.exec(range);
      const size = rangeTotal ? Number(rangeTotal[1]) : Number(r.headers.get("content-length")) || 0;
      reader = r.body.getReader();
      const chunks = [];
      let total = 0;
      while (total < 564) {
        const { done, value } = await withTimeout(reader.read(), 2500);
        if (done) break;
        chunks.push(value);
        total += value.byteLength;
      }
      const out = new Uint8Array(Math.min(total, 564));
      let at = 0;
      for (const c of chunks) {
        const take = Math.min(c.byteLength, out.length - at);
        out.set(c.subarray(0, take), at);
        at += take;
        if (at >= out.length) break;
      }
      const detected = detectContainerBytes(out);
      if (detected || size > 0) return { ext: detected, size };
    }
  } catch {
    // Fall back to in-frame binary sniff.
  } finally {
    if (reader) reader.cancel().catch(() => {});
  }
  if (!preferFrame && tabId >= 0) {
    const viaFrame = await inFrameFetch(tabId, frameId, segUrl, 564, true);
    const ext = viaFrame && Array.isArray(viaFrame.bytes) ? detectContainerBytes(viaFrame.bytes) : "";
    const size = viaFrame && Number(viaFrame.totalBytes) > 0 ? Number(viaFrame.totalBytes) : 0;
    return { ext, size };
  }
  return { ext: "", size: 0 };
}

async function probe(item) {
  if (!item || !item.url) return { failed: true, at: Date.now() };
  if (probesInFlight.has(item.url)) return probesInFlight.get(item.url);
  const tabId = Number.isInteger(item.tabId) ? item.tabId : -1;
  const frameId = item.frameId || 0;
  const run = (async () => {
    try {
      const text = await fetchManifestText(item.url, tabId, frameId, 5000);
      if (text === null) return { failed: true, at: Date.now() };
      const parsed = item.kind === "hls" ? parseHls(text, item.url) : parseDash(text);
      const info = parsed || { failed: true, at: Date.now() };
      let segUrl = info.firstSegmentUrl || "";
      let segDur = Number(info.firstSegmentDuration) || 0;
      if (!info.failed && item.kind === "hls" && info.duration === 0 && Array.isArray(info.childUrls) && info.childUrls.length) {
        try {
          const ct = await fetchManifestText(info.childUrls[0], tabId, frameId, 3500);
          if (ct !== null) {
            const ci = parseHls(ct, info.childUrls[0]);
            if (ci) {
              if (ci.duration > 0) info.duration = ci.duration;
              if (!ci.children.length) info.live = Boolean(ci.live);
              if (ci.drm) info.drm = true;
              if (!segUrl && ci.firstSegmentUrl) {
                segUrl = ci.firstSegmentUrl;
                segDur = Number(ci.firstSegmentDuration) || 0;
              }
            }
          }
        } catch {
          // Master variant duration lookup is best-effort.
        }
      }
      if (!info.failed && item.kind === "hls") {
        if (!info.variants.length) {
          // Direct media playlist (e.g. sublist_aud1.txt or single-variant playlist.m3u8)
          info.variants.push({ height: 0, bandwidth: 0, codecs: "" });
        }
        let segSize = 0;
        if (segUrl && !info.drm) {
          const segPath = pathOf(segUrl).toLowerCase();
          if (/\.ts$/.test(segPath)) info.segmentExt = "ts";
          else if (/\.(m4s|mp4|cmfv)$/.test(segPath)) info.segmentExt = "mp4";
          else {
            const detected = await sniffDisguisedSegment(segUrl, tabId, frameId);
            if (detected.ext) info.segmentExt = detected.ext;
            if (detected.size > 0) segSize = detected.size;
          }
        }
        // Estimate stream bitrate from the already-sniffed first segment when a direct media
        // playlist omits BANDWIDTH / EXT-X-STREAM-INF, without issuing any extra network requests.
        if (
          info.variants.length === 1 &&
          !info.variants[0].bandwidth &&
          segSize > 0 &&
          segDur > 0
        ) {
          const estBps = Math.round((segSize * 8) / segDur);
          if (estBps >= 32000 && estBps <= 100000000) {
            info.variants[0].bandwidth = estBps;
            info.variants[0].average = estBps;
          }
        }
      }
      if (info && info.childUrls) delete info.childUrls;
      if (info && "firstSegmentUrl" in info) delete info.firstSegmentUrl;
      if (info && "firstSegmentDuration" in info) delete info.firstSegmentDuration;
      return info;
    } catch {
      return { failed: true, at: Date.now() };
    } finally {
      probesInFlight.delete(item.url);
    }
  })();
  probesInFlight.set(item.url, run);
  return run;
}

// Run in each frame of the tab: its address and its players (their file, picture size, length,
// and whether the page gave them a DRM key system).
function mediaElements() {
  const thumbOf = (v) => {
    try {
      if (v.tagName === "VIDEO" && v.readyState >= 2 && v.videoWidth > 0 && v.videoHeight > 0) {
        const w = 128;
        const h = Math.max(1, Math.min(128, Math.round((v.videoHeight / v.videoWidth) * w)));
        const c = document.createElement("canvas");
        c.width = w;
        c.height = h;
        const g = c.getContext("2d");
        if (g) {
          g.drawImage(v, 0, 0, w, h);
          const data = c.toDataURL("image/jpeg", 0.72);
          if (data && data.length <= 24576) return data;
        }
      }
    } catch {
      // Cross-origin tainted video canvas: fall back to poster or placeholder icon.
    }
    const p = v.tagName === "VIDEO" && typeof v.poster === "string" && /^https?:/i.test(v.poster) ? v.poster : "";
    return p;
  };
  const players = [...document.querySelectorAll("video, audio")].map((v) => ({
    src: v.currentSrc || v.src || v.querySelector("source[src]")?.src || v.dataset?.src || "",
    w: v.videoWidth || 0,
    h: v.videoHeight || 0,
    duration: Number.isFinite(v.duration) ? v.duration : 0,
    drm: Boolean(v.mediaKeys),
    thumb: thumbOf(v),
  }));
  const subtitles = [];
  const seenSub = new Set();
  for (const tr of document.querySelectorAll("track[src]")) {
    const kind = String(tr.kind || "").toLowerCase();
    if (kind && kind !== "subtitles" && kind !== "captions") continue;
    const raw = tr.src || tr.getAttribute("src") || "";
    if (!/^https?:/i.test(raw) || seenSub.has(raw)) continue;
    seenSub.add(raw);
    subtitles.push({
      url: raw,
      label: String(tr.label || "").trim().slice(0, 64),
      lang: String(tr.srclang || "").trim().slice(0, 16),
    });
  }
  // What the frame itself says it loaded (Resource Timing). The background watches responses as
  // they arrive, but it is a service worker: a stream the page started before the worker was up,
  // and a fetch from a worker of the page's own (no tab to it), never reached it. The page keeps
  // its own list of everything it loaded, so the two together miss far less. Sizes and types are
  // only filled in for its own server's answers, so the address decides and the kinds are worked
  // out in the background as for any response. Up to RES_MAX, newest last.
  const resources = [];
  try {
    for (const e of performance.getEntriesByType("resource")) {
      // Nothing that answered with an error or nothing at all, and no page, style or script.
      if (e.responseStatus && (e.responseStatus >= 400 || e.responseStatus === 204)) continue;
      if (["css", "script", "link", "beacon", "img", "image", "navigation"].includes(e.initiatorType)) continue;
      if (!/^https?:/i.test(e.name)) continue;
      resources.push({ url: e.name, size: e.decodedBodySize || e.encodedBodySize || 0 });
    }
  } catch {
    // A page that keeps no timing: its players alone.
  }
  return { href: location.href, players, subtitles, resources };
}

// Whether a player in the tab plays from a blob: (a MediaSource fed by the page).
async function playsStream(tabId) {
  try {
    const found = await withTimeout(ext.scripting.executeScript({ target: { tabId, allFrames: true }, func: mediaElements }), 3000);
    return (found || []).some((f) => f && f.result && f.result.players.some((v) => /^blob:/i.test(v.src)));
  } catch {
    return false;
  }
}

// How many of a frame's own loads are looked at (the browser keeps 250 by default).
const RES_MAX = 400;

// What an address alone says a file is, for what the frame loaded and the background did not see
// (`mediaElements`): a manifest, a segment, or a plain video or sound file by its extension.
// Nothing is claimed about its size, and anything the address doesn't name stays out: a response
// read nothing of is only media when its own server said so.
function kindFromUrl(url) {
  const path = pathOf(url).toLowerCase();
  if (/\.m3u8?$/.test(path)) return { kind: "hls", mime: "", size: 0 };
  if (/\.mpd$/.test(path)) return { kind: "dash", mime: "", size: 0 };
  if (SUBTITLE_EXT.test(path)) return { kind: "subtitle", mime: "", size: 0 };
  if (/\.txt$/.test(path) && /(?:^|\/)(?:hls|stream|manifest|master|playlist|sublist|index|chunklist)(?:[/_.-]|$)/i.test(path)) {
    return { kind: "maybe", mime: "text/plain", size: 0 };
  }
  if (SEGMENT_EXT.test(path)) return { kind: "video", mime: "", size: 0, piece: true };
  const ext = (MEDIA_EXT.exec(path) || [])[1] || "";
  if (!ext) return null;
  // Nothing watched this one answer, so a film and one fragment of it look alike (CMAF names its
  // fragments .mp4): it counts as a `piece`, which stands one per folder. A response the
  // background did see keeps what it said (`noteMedia` leaves an address it already has).
  return { kind: AUDIO_EXT.test(ext) ? "audio" : "video", mime: "", size: 0, piece: true };
}

// The frames' own loads merged in, in the order they were loaded, after what the background saw
// (`noteMedia` drops a piece of a stream already known and keeps one per folder).
function noteFrameLoads(tabId, frames) {
  for (const [id, f] of Object.entries(frames)) {
    const frameId = Number(id) || 0;
    const list = (f && f.resources) || [];
    for (const r of list.slice(-RES_MAX)) {
      if (!r || !r.url || r.url.length > URL_MAX) continue;
      const found = kindFromUrl(r.url);
      if (!found) continue;
      if (found.kind === "maybe") {
        noteMaybe({ tabId, url: r.url, frameId }, found);
      } else {
        noteMedia({ tabId, url: r.url, frameId }, { ...found, size: r.size > 0 ? r.size : 0 });
      }
    }
    for (const sub of (f && f.subtitles) || []) {
      if (!sub || !sub.url || sub.url.length > URL_MAX) continue;
      noteMedia(
        { tabId, url: sub.url, frameId },
        { kind: "subtitle", mime: "", size: 0, label: sub.label || "", lang: sub.lang || "" },
      );
    }
  }
}

// The tab's media as the popup lists them: a stream's variants inside it, each with what is known
// (qualities, length, size, protected). Manifests are read once.
async function mediaList(tabId) {
  await mediaBack;
  // Asked even for a tab nothing was seen of: this worker may have been started only now, and
  // what the frames themselves loaded (`noteFrameLoads`) is then all there is.
  const frames = {};
  try {
    const found = await withTimeout(ext.scripting.executeScript({ target: { tabId, allFrames: true }, func: mediaElements }), 3000);
    for (const f of found || []) if (f && f.result) frames[f.frameId || 0] = f.result;
  } catch {
    // A page extensions may not read: listed without its players' details.
  }
  noteFrameLoads(tabId, frames);
  // #171: the possible manifests of frames whose player plays from a blob: (a MediaSource), read.
  const fed = Object.entries(frames)
    .filter(([, f]) => f.players.some((v) => /^blob:/i.test(v.src)))
    .map(([id]) => Number(id));
  if (fed.length) await sniffTab(tabId, new Set(fed));
  const tab = media.get(tabId);
  if (!tab) return [];
  await Promise.all(tab.items.filter(needsProbe).map(async (it) => (it.info = await probe(it))));
  tab.frames = Object.fromEntries(Object.entries(frames).map(([id, f]) => [id, f.href]));
  keepMedia();
  const out = [];
  const shown = [];
  for (const it of tab.items) {
    // A variant of a master playlist listed already: it names it, or (unread) the variant is in
    // its folder or below.
    const p = pathOf(it.url);
    const variantOf = (s) =>
      s.info && !s.info.failed ? s.info.children.includes(p) : s.frameId === it.frameId && folderOf(s.url) && p.startsWith(folderOf(s.url));
    if (it.kind === "hls" && shown.some((s) => s.kind === "hls" && variantOf(s))) continue;
    shown.push(it);
    const frame = frames[it.frameId] || { players: [] };
    const frameVideos = (frame.players || []).filter((v) => v && (v.h > 0 || v.w > 0 || v.thumb || /^blob:/i.test(v.src || "")));
    const bestFramePlayer = frameVideos.sort((a, b) => (b.w * b.h) - (a.w * a.h))[0] || null;
    const player = frame.players.find((v) => v.src === it.url) || (isStream(it) ? bestFramePlayer : null);
    const info = it.info && !it.info.failed ? it.info : null;
    const heights =
      info && info.heights.length
        ? [...new Set(info.heights)].sort((a, b) => b - a)
        : player && player.h
          ? [player.h]
          : [];
    const duration = (info ? info.duration : player && player.duration) || (player && player.duration) || 0;
    let estSize = 0;
    if (!it.size && info && duration > 0 && Array.isArray(info.variants) && info.variants.length) {
      const top = [...info.variants].sort(
        (a, b) => (Number(b.height) || 0) - (Number(a.height) || 0) || (Number(b.average || b.bandwidth) || 0) - (Number(a.average || a.bandwidth) || 0),
      )[0];
      const bps = top ? Number(top.average || top.bandwidth) || 0 : 0;
      if (bps > 0) estSize = Math.round((bps / 8) * duration);
    }
    const directPlayer = frame.players.find((v) => v.src === it.url) || null;
    const frameDrm = frame.players.some((v) => v.drm) || hasDrmHint(tabId, it.frameId);
    const audioDrm =
      it.kind === "audio" &&
      (directPlayer
        ? Boolean(directPlayer.drm)
        : hasDrmHint(tabId, it.frameId) || frame.players.some((v) => v.drm && /^blob:/i.test(v.src || "")));
    const playlistCaptured = hasCapturedHlsPlaylist(it);
    out.push({
      url: it.url,
      kind: it.kind,
      mime: it.mime,
      size: it.size || 0,
      ...(estSize > 0 ? { estSize } : {}),
      ...(info && info.segmentExt ? { segmentExt: info.segmentExt } : {}),
      ...(player && player.thumb ? { thumb: player.thumb } : {}),
      ...(it.label ? { label: it.label } : {}),
      ...(it.lang ? { lang: it.lang } : {}),
      ...(playlistCaptured ? { playlistCaptured: true } : {}),
      heights,
      width: (!info || !info.heights.length) && player && player.w ? player.w : 0,
      duration,
      live: Boolean(info && info.live),
      // Protected: its manifest says so, or (unread) its frame's player has a DRM key system.
      drm: info ? info.drm : (isStream(it) && frameDrm) || audioDrm,
    });
  }
  return out;
}

// A file's name from the tab's title when its address ends in a long hash (#345), else none: the
// app names it from the address, as for any download.
function titledName(url, title) {
  const last = pathOf(url).split("/").pop() || "";
  const dot = last.lastIndexOf(".");
  const stem = dot > 0 ? last.slice(0, dot) : last;
  const name = String(title || "").replace(/[\\/:*?"<>|]/g, " ").trim().slice(0, 150);
  const hash = stem.length >= 24 && /^[A-Za-z0-9=_-]+$/.test(stem) && (stem.match(/[-_]/g) || []).length <= 2;
  return hash && name ? `${name}${dot > 0 ? last.slice(dot) : ""}` : "";
}

// One of the tab's media, handed to the app as the popup asked: a stream to the Download video
// window (yt-dlp), a file as a download; with the page that played it (the referrer), the cookies
// its own server gets, and the browser's User-Agent (`native`). A protected one is never sent.
async function mediaDownload(tabId, url, wantWindow = false) {
  const listed = await mediaList(tabId);
  const shown = listed.find((x) => x.url === url);
  if (!shown) return { ok: false, why: "gone" };
  if (shown.drm) return { ok: false, why: "drm" };
  const kept = media.get(tabId);
  const it = kept.items.find((x) => x.url === url);
  let tab;
  try {
    tab = await ext.tabs.get(tabId);
  } catch {
    return { ok: false, why: "gone" };
  }
  const frame = (kept.frames || {})[it.frameId];
  const page = frame && /^https?:/i.test(frame) ? frame : tab.url || "";
  const topPage = tab.url && /^https?:/i.test(tab.url) ? tab.url : "";
  const reqRef = capturedRefererFor(it.url) || page;
  const reqOrigin = capturedOriginFor(it.url);
  const ctx = { incognito: Boolean(tab.incognito), storeId: tab.cookieStoreId };
  if (isStream(it)) {
    const isVodHls = it.kind === "hls" && !shown.live;
    const isDisguisedVodHls = isVodHls && !MANIFEST.test(it.url.split(/[?#]/)[0]);
    const tracks = isVodHls && it.info && Array.isArray(it.info.audioTracks) ? it.info.audioTracks : [];
    const defAud =
      tracks.find((a) => a && a.default && a.uri) ||
      tracks.find((a) => a && a.autoselect && a.uri) ||
      tracks.find((a) => a && a.uri) ||
      null;
    return native({
      video: true,
      // Asked for: whether the button over videos shows on this site doesn't matter.
      explicit: true,
      url: page,
      src: it.url,
      title: String(tab.title || "").slice(0, 300),
      referrer: tab.url && tab.url !== page ? tab.url : "",
      ...(reqRef ? { requestReferer: reqRef } : {}),
      ...(reqOrigin ? { requestOrigin: reqOrigin } : {}),
      ...(topPage ? { pageUrl: topPage } : {}),
      ...(shown.duration > 0 ? { duration: shown.duration } : {}),
      ...(shown.live ? { live: true } : {}),
      ...(wantWindow
        ? { window: true }
        : isDisguisedVodHls
          ? { selector: "hls-best", container: hlsPreferredContainer(it.info, "hls-best") }
          : {}),
      ...(defAud && defAud.uri ? { hlsAudio: defAud.uri } : {}),
      ...(it.kind === "hls" ? hlsSnapshotExtras(it.url, tabId) : {}),
      ...(await videoCookies(it.url, ctx)),
      ...(ctx.incognito ? { private: true } : {}),
    });
  }
  return handOff({
    url: it.url,
    referrer: reqRef || page,
    ...(topPage && topPage !== (reqRef || page) ? { pageUrl: topPage } : {}),
    size: it.size,
    mime: it.mime,
    explicit: true,
    ...(wantWindow ? { window: true } : {}),
    filename: titledName(it.url, tab.title),
    ...ctx,
  });
}

// ---- the hand-off ------------------------------------------------------------------------

// Cookies from the store the download belongs to, never another: Firefox names it on the
// download (private windows and containers each have their own); Chrome's incognito store is "1".
async function cookiesFor(url, { incognito, storeId }) {
  try {
    const query = storeId ? { url, storeId } : incognito ? { url, storeId: "1" } : { url };
    return (await ext.cookies.getAll(query)) || [];
  } catch {
    return [];
  }
}

function header(list) {
  return list.map((c) => `${c.name}=${c.value}`).join("; ");
}

async function cookieHeader(url, ctx) {
  return header(await cookiesFor(url, ctx));
}

// For yt-dlp, which may ask another host of the site than the page's (a YouTube Music page is
// looked up on www.youtube.com): each cookie with the domain, path and flags the browser keeps,
// so the app files `.youtube.com`'s sign-in for every host of it and a host's own cookie for
// that host alone. The header goes too, for an older app.
async function videoCookies(url, ctx, extraUrl = "") {
  const list = await cookiesFor(url, ctx);
  const all = [...list];
  if (extraUrl && /^https?:/i.test(extraUrl) && siteOf(extraUrl) !== siteOf(url)) {
    const more = await cookiesFor(extraUrl, ctx);
    const seen = new Set(all.map((c) => `${c.domain}|${c.path || "/"}|${c.name}`));
    for (const c of more) {
      const k = `${c.domain}|${c.path || "/"}|${c.name}`;
      if (!seen.has(k)) {
        seen.add(k);
        all.push(c);
      }
    }
  }
  const cookieJar = all.map((c) => ({
    name: c.name,
    value: c.value,
    domain: c.domain,
    hostOnly: Boolean(c.hostOnly),
    path: c.path || "/",
    secure: Boolean(c.secure),
    ...(typeof c.expirationDate === "number" ? { expirationDate: c.expirationDate } : {}),
  }));
  return { cookies: header(list), cookieJar };
}

// Has the user left this extension the site? With access limited ("On click", some sites), its
// cookies can't be read, and a download handed over without them would fail.
async function siteAccess(url) {
  try {
    const u = new URL(url);
    // By host name: Firefox's match patterns take no port.
    return await ext.permissions.contains({ origins: [`${u.protocol}//${u.hostname}/*`] });
  } catch {
    return false;
  }
}

// ---- the browser's proxy (#42) -------------------------------------------------------------

// A VPN or proxy extension is a proxy for the browser only: the app downloads through the same
// one (Options → Proxy / Socks → "Use proxy/socks from a browser's request"), or the download
// would leave from the user's own address. What goes with each download:
// - `proxy`: the proxy the browser uses for its address, "scheme://host:port";
// - `proxyPac`: the browser's proxy auto-config script (its text), when it uses one;
// - `proxyLogin`: that proxy asked the browser for a login. The extension that set it answers
//   with its own, which no other program can have: the app says so rather than going without.
// Firefox tells each request's proxy (`proxyInfo`), what proxy extensions set per request
// (`proxy.onRequest`) included; Chromium has no such event, only its proxy settings
// (`proxy.settings.get`: a fixed server per scheme with its bypass list, or a PAC script).
const proxyLogins = new Set();
const proxyUsed = new Map();
const PAC_MAX = 256 * 1024;

// Reading the browser's proxy is the user's choice, the popup's switch (off as installed):
// - Firefox: the optional "proxy" permission, asked for by the switch and taken back by it.
// - Chromium can't make "proxy" optional (chrome.permissions), so its manifest has it from the
//   install, and the switch is the extension's own setting (`proxyOn` in storage.local). While it
//   is off, nothing of the browser's proxy is read or sent.
// `proxyAllowed` follows the permission or the setting as it changes.
const PROXY_PERMISSION = { permissions: ["proxy"] };
const PROXY_SWITCH = "proxyOn";
const [PROXY_DECLARED, PROXY_REQUIRED] = (() => {
  try {
    const m = ext.runtime.getManifest();
    const required = (m.permissions || []).includes("proxy");
    return [required || (m.optional_permissions || []).includes("proxy"), required];
  } catch {
    return [false, false];
  }
})();
// Off until checked.
let proxyAllowed = false;
async function checkProxy() {
  try {
    proxyAllowed = PROXY_REQUIRED
      ? Boolean((await ext.storage.local.get({ [PROXY_SWITCH]: false }))[PROXY_SWITCH])
      : PROXY_DECLARED && Boolean(await ext.permissions.contains(PROXY_PERMISSION));
  } catch {
    proxyAllowed = false;
  }
  if (!proxyAllowed) {
    // Nothing kept of what was seen while it was allowed.
    proxyLogins.clear();
    proxyUsed.clear();
  }
}
const proxyChecked = PROXY_DECLARED ? checkProxy() : Promise.resolve();
if (PROXY_REQUIRED && ext.storage.onChanged) {
  ext.storage.onChanged.addListener((changes, area) => {
    if (area === "local" && PROXY_SWITCH in changes) checkProxy();
  });
} else if (PROXY_DECLARED && ext.permissions && ext.permissions.onAdded) {
  ext.permissions.onAdded.addListener(checkProxy);
  ext.permissions.onRemoved.addListener(checkProxy);
}

if (PROXY_DECLARED && ext.webRequest && ext.webRequest.onAuthRequired) {
  try {
    ext.webRequest.onAuthRequired.addListener(
      (d) => {
        if (proxyAllowed && d.isProxy && d.challenger) proxyLogins.add(`${d.challenger.host}:${d.challenger.port}`.toLowerCase());
      },
      { urls: ["<all_urls>"] },
    );
  } catch {
    // Not offered here: no login is noticed.
  }
}
// Firefox: the proxy a request went through, kept for its hand-over. Noted as its response
// starts, and from its answer's headers when the extension takes the download there (#208): that
// request is cancelled, so it never starts.
function noteProxy(d) {
  if (!proxyAllowed || !d.proxyInfo || d.proxyInfo.type === "direct") return;
  forget(proxyUsed, 120000);
  proxyUsed.set(d.url, { info: d.proxyInfo, at: Date.now() });
}
if (PROXY_DECLARED && FIREFOX && ext.webRequest && ext.webRequest.onResponseStarted) {
  ext.webRequest.onResponseStarted.addListener(noteProxy, { urls: ["<all_urls>"] });
}

const hostPort = (host, port) => `${String(host).includes(":") ? `[${host}]` : host}:${port}`;

// "scheme://host:port" for one of the browser's proxy descriptions, or null.
function proxyUrl(scheme, host, port, remoteDns) {
  if (!host || !port) return null;
  const s = String(scheme || "http").toLowerCase();
  const kind =
    s === "socks" || s === "socks5" ? (remoteDns === false ? "socks5" : "socks5h") : s === "socks4" ? "socks4" : s === "https" ? "https" : s === "http" ? "http" : null;
  return kind ? `${kind}://${hostPort(host, port)}` : null;
}

// Does `pattern` (a proxy bypass entry: "<local>", "*.example.com", ".example.com", "host",
// "host:port") cover `u`? CIDR entries aren't read (the proxy is then used).
function bypassed(pattern, u) {
  const p = String(pattern || "").trim().toLowerCase();
  if (!p) return false;
  const host = u.hostname.toLowerCase().replace(/^\[|\]$/g, "");
  if (p === "<local>") return !host.includes(".") || host === "localhost" || host === "127.0.0.1" || host === "::1";
  const bare = p.replace(/^[a-z]+:\/\//, "").replace(/:\d+$/, "");
  if (bare.startsWith("*.")) return host.endsWith(bare.slice(1));
  if (bare.startsWith(".")) return host.endsWith(bare) || host === bare.slice(1);
  return host === bare;
}

// Chromium's bypass list (its proxy rules' `bypassList`) for `u`, as Chromium reads one: "<local>"
// (a name without a dot), "[scheme://]host-pattern[:port]" with "*" anywhere (".example.com" is
// "*.example.com"), an IPv4 range "10.0.0.0/8"; and, unless "<-loopback>" is listed, the names
// Chromium never sends to a proxy (localhost, 127.x, ::1, link-local). IPv6 ranges aren't read
// (the proxy is then used).
function chromiumBypassed(list, u) {
  const host = u.hostname.toLowerCase().replace(/^\[|\]$/g, "");
  const scheme = u.protocol.replace(/:$/, "");
  const port = u.port || (scheme === "https" ? "443" : "80");
  const v4 = /^\d{1,3}(\.\d{1,3}){3}$/.test(host) ? host.split(".").reduce((n, b) => n * 256 + Number(b), 0) : null;
  const ip = v4 !== null || host.includes(":");
  const rules = (list || []).map((r) => String(r).trim().toLowerCase()).filter(Boolean);
  if (!rules.includes("<-loopback>")) {
    if (host === "localhost" || host.endsWith(".localhost") || host === "::1" || (host.includes(":") && /^fe[89ab]/.test(host))) return true;
    if (v4 !== null && (host.startsWith("127.") || host.startsWith("169.254."))) return true;
  }
  return rules.some((r) => {
    if (r === "<-loopback>") return false;
    if (r === "<local>") return !ip && !host.includes(".");
    const range = /^(\d{1,3}(?:\.\d{1,3}){3})\/(\d{1,2})$/.exec(r);
    if (range) {
      if (v4 === null) return false;
      const base = range[1].split(".").reduce((n, b) => n * 256 + Number(b), 0);
      const bits = Number(range[2]);
      const size = 2 ** (32 - Math.min(bits, 32));
      return Math.floor(v4 / size) === Math.floor(base / size);
    }
    const m = /^(?:([a-z][a-z0-9+.-]*):\/\/)?(\[[^\]]+\]|[^:/]+)(?::(\d+))?\/?$/.exec(r);
    if (!m || (m[1] && m[1] !== scheme) || (m[3] && m[3] !== port)) return false;
    const pattern = m[2].replace(/^\[|\]$/g, "").replace(/^\./, "*.");
    const glob = pattern.replace(/[.+?^$|\\()[\]{}]/g, "\\$&").replace(/\*/g, ".*");
    return new RegExp("^" + glob + "$").test(host);
  });
}

// The port Chromium takes for a proxy given without one.
const chromiumPort = (scheme) => ({ https: 443, quic: 443, socks: 1080, socks4: 1080, socks5: 1080 })[String(scheme || "http").toLowerCase()] || 80;

async function pacText(url) {
  try {
    const r = await withTimeout(fetch(url, { credentials: "omit" }), 10000);
    if (!r.ok) return null;
    const text = await r.text();
    return text.length <= PAC_MAX && /FindProxyForURL/.test(text) ? text : null;
  } catch {
    return null;
  }
}

// Firefox never sends loopback addresses through its proxy settings' proxy
// (network.proxy.allow_hijacking_localhost, off by default): localhost and its names, 127.0.0.0/8, ::1.
function firefoxLoopback(u) {
  const host = u.hostname.toLowerCase().replace(/^\[|\]$/g, "");
  return host === "localhost" || host.endsWith(".localhost") || host === "::1" || /^127(\.\d{1,3}){3}$/.test(host);
}

// What the app needs to download `url` through the browser's proxy: {} when it uses none (or
// the system's, which the app reads itself), and {} unless the user switched it on (above).
async function browserProxy(url, incognito) {
  // Chromium's switch is read again for each download: the popup may have just changed it.
  await (PROXY_REQUIRED ? checkProxy() : proxyChecked);
  if (!proxyAllowed) return {};
  let u;
  try {
    u = new URL(url);
  } catch {
    return {};
  }
  const login = (host, port) => proxyLogins.has(`${host}:${port}`.toLowerCase());
  // Firefox: the proxy its own request for this address went through.
  const used = proxyUsed.get(url);
  if (used) {
    const p = used.info;
    const proxy = proxyUrl(p.type, p.host, p.port, p.proxyDNS);
    if (!proxy) return {};
    return { proxy, ...(p.username || login(p.host, p.port) ? { proxyLogin: true } : {}) };
  }
  if (!ext.proxy || !ext.proxy.settings) return {};
  let v;
  try {
    v = (await ext.proxy.settings.get(incognito ? { incognito: true } : {})).value || {};
  } catch {
    return {};
  }
  // Chromium: {mode, rules, pacScript}, the browser's effective proxy whoever set it (its
  // levelOfControl): a VPN or proxy extension ("controlled_by_other_extensions", the case this
  // is for), a policy or the command line ("not_controllable"), or nobody ({mode: "system"}).
  // "direct", "system" and "auto_detect" send nothing: the app then goes by its own Options
  // (which read the system's proxy). The proxy's login is never in it, on Chromium either.
  if (v.mode === "fixed_servers" && v.rules) {
    const r = v.rules;
    if (chromiumBypassed(r.bypassList, u)) return {};
    const s = r.singleProxy || (u.protocol === "https:" ? r.proxyForHttps : r.proxyForHttp) || r.fallbackProxy;
    const port = s && (s.port || chromiumPort(s.scheme));
    const proxy = s && proxyUrl(s.scheme, s.host, port);
    if (!proxy) return {};
    return { proxy, ...(login(s.host, port) ? { proxyLogin: true } : {}) };
  }
  if (v.mode === "pac_script" && v.pacScript) {
    const data = v.pacScript.data && v.pacScript.data.length <= PAC_MAX ? v.pacScript.data : null;
    const text = data || (v.pacScript.url ? await pacText(v.pacScript.url) : null);
    return text ? { proxyPac: text } : {};
  }
  // Firefox: {proxyType, http, ssl, socks, socksVersion, proxyDNS, autoConfigUrl, passthrough}.
  if (v.proxyType === "manual") {
    if (firefoxLoopback(u)) return {};
    if (String(v.passthrough || "").split(/[,\s]+/).some((b) => bypassed(b, u))) return {};
    const pick = (hp) => {
      const m = /^\[?([^\]]+?)\]?:(\d+)$/.exec(String(hp || "").trim());
      return m ? { host: m[1], port: Number(m[2]) } : null;
    };
    const http = pick(v.http);
    const ssl = v.httpProxyAll ? http : pick(v.ssl);
    const socks = pick(v.socks);
    const s = (u.protocol === "https:" ? ssl : http) || null;
    const proxy = s
      ? proxyUrl("http", s.host, s.port)
      : socks && proxyUrl(v.socksVersion === 4 ? "socks4" : "socks5", socks.host, socks.port, v.proxyDNS);
    if (!proxy) return {};
    const at = s || socks;
    return { proxy, ...(login(at.host, at.port) ? { proxyLogin: true } : {}) };
  }
  if (v.proxyType === "autoConfig" && v.autoConfigUrl) {
    const text = await pacText(v.autoConfigUrl);
    return text ? { proxyPac: text } : {};
  }
  return {};
}

// `explicit`: the user asked for this one (the right-click item), so Options' automatic-capture
// rules (file types, excluded sites) don't apply; the host checks them for everything else.
async function handOff(d) {
  const [cookies, proxy] = await Promise.all([
    cookieHeader(d.url, d),
    browserProxy(d.finalUrl || d.url, d.incognito),
  ]);
  const capturedRef = !d.referrer ? capturedRefererFor(d.finalUrl || d.url) : "";
  const message = {
    url: d.url,
    finalUrl: d.finalUrl && d.finalUrl !== d.url ? d.finalUrl : "",
    referrer: d.referrer || capturedRef || "",
    ...(d.pageUrl ? { pageUrl: d.pageUrl } : {}),
    cookies,
    filename: d.filename || "",
    ...(d.size > 0 ? { size: d.size } : {}),
    ...(d.mime ? { mime: d.mime } : {}),
    ...(d.post ? { post: d.post.body, postType: d.post.type || "" } : {}),
    ...(d.incognito ? { private: true } : {}),
    ...(d.explicit ? { explicit: true } : {}),
    // Through the browser's proxy too (#42): the one its own request for the file took.
    ...proxy,
  };
  return native(message);
}

// The file name to offer the app the download under (`{name}`), or why Options leave it to
// the browser (`{left, name}`), or null: the browser keeps it and there is nothing to say (not a
// web address, paused, Alt held, a form that can't be posted again, XDM not reached).
async function candidate(item, url) {
  if (!/^https?:/i.test(url)) return null; // blob:, data:, file: stay the browser's
  const { paused } = await ext.storage.local.get({ paused: false });
  if (paused) return null;
  forget(bypass, 10000);
  if (bypass.has(item.url) || bypass.has(url)) return null;
  const first = firstUrl(item);
  if (bypass.has(first) || postOf(first) === null) return null; // Alt; a form not to post again
  const r = await currentRules();
  if (!r) return null;
  const name = (item.filename || "").split(/[\\/]/).pop() || url.split(/[?#]/)[0].split("/").pop() || "";
  const left = (why) => ({ left: why, name });
  if (!r.capture) return left(r.captureOff || "browser");
  const by = excludedBy(r, first) || excludedBy(r, url);
  if (by) return left(by);
  const type = extensionOf(name) || mimeTypeExt(item.mime);
  if (!type || !typePatterns(r.types).some((p) => p.test(type))) return left("file-type");
  // Smaller than Options' minimum (when the browser knows the size already).
  const size = item.totalBytes > 0 ? item.totalBytes : item.fileSize;
  if (r.minKb > 0 && size > 0 && size < r.minKb * 1024) return left("small");
  if (!(await siteAccess(first))) return left("site-access");
  return { name: extensionOf(name) ? name : `${name || "download"}.${type}` };
}

// Why it stays with the browser, said (the popup, XDM's log).
function leftAlone(c, item, url) {
  if (c && c.left) noteLeft(c.left, c.name, firstUrl(item) || url, item.incognito, false).catch(() => {});
}

// Offer the download to the app under `name`; true if it took it.
async function offer(item, url, name) {
  const first = firstUrl(item);
  const reply = await handOff({
    url: first,
    finalUrl: url,
    referrer: item.referrer,
    filename: name,
    size: item.totalBytes > 0 ? item.totalBytes : item.fileSize,
    mime: item.mime,
    post: postOf(first),
    incognito: item.incognito,
    storeId: item.cookieStoreId,
  });
  if (reply.ok) {
    await ext.storage.local.remove("declined").catch(() => {});
  } else {
    // Refused by Options in the app (changed since the extension asked, or a rule only the app
    // checks, as a redirect's end): the host logged it; the popup says it, and we immediately
    // resync cached rules so deleted file types / newly added exclusions apply everywhere.
    const why = declinedOf(reply);
    if (why) {
      await noteLeft(why, name, first, item.incognito, true).catch(() => {});
      refreshRules().catch(() => {});
    }
  }
  return Boolean(reply.ok);
}

// Safari: a link clicked in a page (page.js) that may be a download, offered as the browser's own
// downloads are: through `candidate` (Options' rules, Alt, pause) then `offer`. `{taken: true}`
// when the app took it; else page.js lets the click go on, so nothing is lost. The link's file
// name is its `download` attribute's (`name`), else its address's (Safari names a download only
// when it starts, which is too late).
// The name an address gives, readable: the app saves a name as it is told it (the browsers' own
// downloads come already named), so `My%20file.zip` would stay that.
function addressName(url) {
  const last = url.split(/[?#]/)[0].split("/").pop() || "";
  try {
    return decodeURIComponent(last);
  } catch {
    return last; // not valid percent-encoding
  }
}

async function link(url, referrer, incognito, name) {
  // What the browser opens itself (a PDF, a song) is taken only when Options say so (`opened`,
  // "Also take files the browser would open"), as in the other browsers.
  const r = await currentRules();
  const type = extensionOf(name || url.split(/[?#]/)[0]).toUpperCase();
  if (r && r.opened === false && OPENED[type]) {
    bypass.set(url, { at: Date.now() });
    return { taken: false };
  }
  const filename = name || addressName(url);
  const item = { url, referrer, incognito, ...(filename ? { filename } : {}) };
  const c = await candidate(item, url).catch(() => null);
  if (!c || c.left) {
    leftAlone(c, item, url);
    bypass.set(url, { at: Date.now() });
    return { taken: false };
  }
  const taken = await offer(item, url, c.name).catch(() => false);
  if (!taken) bypass.set(url, { at: Date.now() });
  return { taken };
}

// Tabs opened in the last seconds: one the download was opened in (a link opened in a new tab
// or window, Shift+click) is closed with it, as when the browser keeps the download.
const newTabs = new Map();
ext.tabs.onCreated.addListener((tab) => {
  forget(newTabs, 10000);
  newTabs.set(tab.id, { at: Date.now() });
});

async function closeLeftTab(urls) {
  forget(newTabs, 10000);
  for (const id of newTabs.keys()) {
    try {
      const tab = await ext.tabs.get(id);
      const at = tab.pendingUrl || tab.url || "";
      // Only a tab opened for the download's own address (never a new tab the user opened).
      if (urls.includes(at)) {
        newTabs.delete(id);
        await ext.tabs.remove(id);
      }
    } catch {
      newTabs.delete(id);
    }
  }
}

// The browser's download, gone without a trace once the app has it.
async function drop(item) {
  const id = item.id;
  try {
    await ext.downloads.cancel(id);
  } catch {
    // Already finished: its file goes.
  }
  try {
    const [d] = await ext.downloads.search({ id });
    if (d && d.state === "complete" && d.exists !== false) await ext.downloads.removeFile(id);
  } catch {
    // Gone already.
  }
  ext.downloads.erase({ id }).catch(() => {});
  closeLeftTab([firstUrl(item), item.url, item.finalUrl].filter(Boolean)).catch(() => {});
}

if (SAFARI) {
  // Nothing to listen to: see `link`.
} else if (!FIREFOX) {
  // Chromium: once the final file name is known (after redirects and Content-Disposition). The
  // download waits for `suggest`, writing nothing, while the app is asked: taken, it is
  // cancelled; else it goes on as the browser's. So it is never lost or kept by both (the app
  // asks the address once more: a form's download is posted again with its body).
  ext.downloads.onDeterminingFilename.addListener((item, suggest) => {
    const url = item.finalUrl || item.url;
    candidate(item, url)
      .then(async (c) => {
        const taken = Boolean(c && c.name && !c.left) && (await offer(item, url, c.name));
        if (taken) return drop(item);
        // The browser goes on first; why is said after.
        suggest();
        leftAlone(c, item, url);
      })
      .catch(() => suggest());
    return true; // suggest() comes later
  });
} else {
  // Firefox names the file before the download is created and has no such pause. Most are
  // caught earlier, from the answer to the page load (`headersReceived`, below); the others (a
  // download started by a page's script, a type the answer didn't say) here: the download is
  // paused at once when it may be the app's, then cancelled or resumed; one that finished
  // meanwhile (a small file) has its file removed.
  // A page load decided from its answer already (`headersReceived`, #208) isn't asked again.
  ext.downloads.onCreated.addListener(async (item) => {
    const url = item.finalUrl || item.url;
    forget(decided, 120000);
    if (decided.has(url) || decided.has(firstUrl(item))) return;
    const c = await candidate(item, url).catch(() => null);
    if (!c || c.left) return leftAlone(c, item, url);
    const name = c.name;
    let paused = false;
    try {
      await ext.downloads.pause(item.id);
      paused = true;
    } catch {
      // Not pausable (no resume support), or finished already.
    }
    const taken = await offer(item, url, name).catch(() => false);
    if (taken) drop(item);
    else if (paused) ext.downloads.resume(item.id).catch(() => {});
  });
}

// ---- files the browser would open itself (a PDF, an MP3): made downloads -----------------

// Options → General in the app: also take the listed types the browser shows or plays when a
// link leads to one. Chromium: a session rule marks such answers as attachments, so they come
// down the download path above (where the rules and Alt apply). Firefox: taken from the answer
// as a download is (`headersReceived`), else it opens.
const OPENED = {
  PDF: ["application/pdf"],
  MP3: ["audio/mpeg", "audio/mp3"],
  MP4: ["video/mp4"],
  M4A: ["audio/mp4", "audio/x-m4a"],
  M4V: ["video/x-m4v"],
  OGG: ["audio/ogg"],
  OGV: ["video/ogg"],
  WAV: ["audio/wav", "audio/x-wav", "audio/wave"],
  AAC: ["audio/aac"],
  MOV: ["video/quicktime"],
  TIF: ["image/tiff"],
  TIFF: ["image/tiff"],
  WMA: ["audio/x-ms-wma"],
  WMV: ["video/x-ms-wmv"],
  AVI: ["video/x-msvideo", "video/avi"],
  MKV: ["video/x-matroska", "video/mkv", "video/matroska", "application/x-matroska"],
  WEBM: ["video/webm", "audio/webm"],
  WEBA: ["audio/webm"],
  FLV: ["video/x-flv"],
  F4V: ["video/x-f4v"],
  TS: ["video/mp2t"],
  MPEG: ["video/mpeg"],
  MPG: ["video/mpeg"],
  "3GP": ["video/3gpp"],
  "3G2": ["video/3gpp2"],
  ASF: ["video/x-ms-asf"],
  FLAC: ["audio/flac", "audio/x-flac"],
  OPUS: ["audio/opus"],
};
function mimeTypeExt(mime) {
  const clean = String(mime || "").split(";")[0].trim().toLowerCase();
  if (!clean) return "";
  for (const [ext, list] of Object.entries(OPENED)) {
    if (list.includes(clean)) return ext.toLowerCase();
  }
  return "";
}
const OPENED_RULE = 1;
// The allow rules for exact excluded hosts follow it (ids from 2).
const OPENED_ALLOW = 2;
let openedTypes = [];

// Options' excluded sites as the rules need them, read as the app reads them: `*.example.com`
// is the domain and its subdomains (a rule's domain condition), a plain name that host only
// (an allow rule of its own). As the browser writes a host: no port, punycode.
function excludedHosts() {
  const wide = [];
  const exact = [];
  for (const raw of ((rules && rules.excludedSites) || "").split(/\s+/)) {
    const p = raw.trim().replace(/^[a-z]+:\/\//i, "").split("/")[0];
    const sub = p.startsWith("*.");
    let host = "";
    try {
      host = new URL(`http://${sub ? p.slice(2) : p}`).hostname;
    } catch {
      continue;
    }
    if (!host || host.includes("*")) continue;
    (sub ? wide : exact).push(host);
  }
  return { wide, exact };
}

function openedMimes(r) {
  if (!r || !r.capture || !r.opened) return [];
  const pats = typePatterns(r.types);
  return [...new Set(Object.entries(OPENED).flatMap(([k, m]) => (pats.some((p) => p.test(k)) ? m : [])))];
}

async function syncOpenedRule() {
  const { paused } = await ext.storage.local.get({ paused: false });
  const { why } = await readStatus();
  openedTypes = paused || (why && why !== "update-app") ? [] : openedMimes(rules);
  const dnr = ext.declarativeNetRequest;
  if (FIREFOX || !dnr || !dnr.updateSessionRules) return;
  const hosts = excludedHosts();
  const rule = {
    id: OPENED_RULE,
    priority: 1,
    action: {
      type: "modifyHeaders",
      responseHeaders: [{ header: "content-disposition", operation: "set", value: "attachment" }],
    },
    condition: {
      resourceTypes: ["main_frame"],
      responseHeaders: [{ header: "content-type", values: openedTypes.map((m) => `${m}*`) }],
      // Options' excluded sites keep opening in the browser.
      ...(hosts.wide.length ? { excludedRequestDomains: hosts.wide } : {}),
    },
  };
  const esc = (h) => h.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const allows = hosts.exact.slice(0, 500).map((h, i) => ({
    id: OPENED_ALLOW + i,
    priority: 2,
    action: { type: "allow" },
    condition: { resourceTypes: ["main_frame"], regexFilter: `^[a-z]+://${esc(h)}(:[0-9]+)?/` },
  }));
  try {
    const old = (await dnr.getSessionRules()).map((r) => r.id);
    await dnr.updateSessionRules({
      removeRuleIds: old.filter((id) => id >= OPENED_RULE),
      addRules: openedTypes.length ? [rule, ...allows] : [],
    });
  } catch {
    // A browser without header conditions (Chrome before 128): it opens them as before.
  }
}

// ---- Firefox: a download caught from its answer, before the browser's dialog (#208) ----------

// Firefox creates a download (downloads.onCreated, above) only once it has a file to save to:
// when it asks first ("What should Firefox do with this file?", which Waterfox always asks for
// a new type), only after the user answered, while it downloads the whole file behind the
// dialog. So a page load whose answer would be a download is caught as the answer arrives: the
// load waits while the app is asked, as Chromium's download does, and is cancelled when the app
// took it (no dialog, no browser download). Else it goes on as before, and the download it
// becomes is left to the browser (`decided`). A file the browser would show or play (a PDF, an
// MP3) is taken the same way when Options say so (`openedTypes`); else it opens.

// What Firefox shows or plays itself: a download only when its answer says "attachment".
const SHOWN = /^(text\/|image\/|video\/|audio\/|multipart\/|application\/(pdf|xhtml\+xml|xml|json|javascript|x-javascript|ecmascript|ogg|x-xpinstall|[\w.+-]+\+(xml|json))$)/;

// The file name a Content-Disposition gives (filename*=UTF-8''…, else filename="…"), or "".
function dispositionName(cd) {
  const star = /filename\*\s*=\s*([\w-]+)'[^']*'([^;]+)/i.exec(cd);
  if (star) {
    try {
      return decodeURIComponent(star[2].trim().replace(/^"|"$/g, ""));
    } catch {
      // Not percent-encoded as it says: the plain name below.
    }
  }
  const plain = /(?:^|;)\s*filename\s*=\s*("((?:[^"\\]|\\.)*)"|[^;]+)/i.exec(cd);
  if (!plain) return "";
  return decodedPlain((plain[2] !== undefined ? plain[2].replace(/\\(.)/g, "$1") : plain[1]).trim());
}

// A plain `filename` as sent, or its percent-encoded UTF-8 decoded when that is what it holds
// (non-ASCII once decoded: `100%25.txt` stays as it is), as the app reads it (naming.rs).
function decodedPlain(name) {
  if (!name.includes("%")) return name;
  try {
    const decoded = decodeURIComponent(name);
    return /[^\x00-\x7f]/.test(decoded) ? decoded : name;
  } catch {
    return name; // not percent-encoded UTF-8
  }
}

// A response header's value as text. Firefox gives its bytes one character each (ISO-8859-1), so
// a name sent as raw UTF-8 (`filename="تقرير.pdf"`) arrived as `ØªÙ…` (#447). It is read as UTF-8
// when those bytes are valid UTF-8, as Firefox reads the name for its own downloads and the app
// for its own requests; bytes that aren't (ISO-8859-1's `café`) stay as they came.
function headerText(value) {
  if (!/[\x80-\xff]/.test(value) || /[^\x00-\xff]/.test(value)) return value;
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(Uint8Array.from(value, (c) => c.charCodeAt(0)));
  } catch {
    return value;
  }
}

// {name, size, mime, opened} when this answer to a page load becomes a download (or is one of
// the files Options take though the browser would open them), else null: a page, a picture, a
// redirect, an error, a piece of a file.
function answerFile(d) {
  const code = d.statusCode || 0;
  if (code < 200 || code > 299 || code === 204 || code === 205 || code === 206) return null;
  const headers = d.responseHeaders || [];
  const get = (n) => ((headers.find((h) => h.name.toLowerCase() === n) || {}).value || "").trim();
  const mime = get("content-type").split(";")[0].trim().toLowerCase();
  const cd = get("content-disposition");
  const attachment = /^attachment/i.test(cd);
  const opened = !attachment && openedTypes.some((m) => mime.startsWith(m));
  // No type: the browser guesses from the bytes (a page, mostly); a download it becomes is
  // still seen by onCreated.
  if (!attachment && !opened && (!mime || SHOWN.test(mime))) return null;
  let name = dispositionName(headerText(cd));
  if (!name) {
    try {
      name = decodeURIComponent(pathOf(d.url).split("/").pop() || "");
    } catch {
      name = pathOf(d.url).split("/").pop() || "";
    }
  }
  name = name.split(/[\\/]/).pop() || "";
  // The size the file will have: the length of an answer that isn't compressed on the way.
  const size = get("content-encoding") ? 0 : Number(get("content-length")) || 0;
  return { name, size, mime, opened };
}

// The page loads decided here (by their first and last addresses, two minutes): the download
// one of them becomes after the browser's dialog is left to the browser (asked once).
const decided = new Map();

// A tab opened for the download only (a link opened in a new tab or window), closed with it.
async function closeDownloadTab(tabId, urls) {
  forget(newTabs, 10000);
  if (!(tabId >= 0) || !newTabs.has(tabId)) return;
  try {
    const tab = await ext.tabs.get(tabId);
    const at = tab.url || "";
    if (at === "about:blank" || urls.includes(at)) {
      newTabs.delete(tabId);
      await ext.tabs.remove(tabId);
    }
  } catch {
    // Closed already.
  }
}

async function headersReceived(d) {
  if (d.method !== "GET" && d.method !== "POST") return {};
  const file = answerFile(d);
  if (!file) return {};
  const ref = referrers.get(d.requestId);
  const item = {
    url: d.url,
    filename: file.name,
    totalBytes: file.size,
    fileSize: file.size,
    mime: file.mime,
    referrer: (ref && ref.url) || "",
    incognito: Boolean(d.incognito),
    cookieStoreId: d.cookieStoreId,
  };
  if (PROXY_DECLARED) {
    await proxyChecked;
    noteProxy(d);
  }
  const first = firstUrl(item);
  forget(decided, 120000);
  decided.set(d.url, { at: Date.now() });
  decided.set(first, { at: Date.now() });
  const c = await candidate(item, d.url).catch(() => null);
  if (!c || c.left) {
    // A file the browser opens stays a page: there is no download to say anything about.
    if (!file.opened) leftAlone(c, item, d.url);
    return {};
  }
  const taken = await offer(item, d.url, c.name).catch(() => false);
  if (!taken) return {};
  closeDownloadTab(d.tabId, [first, d.url]).catch(() => {});
  return { cancel: true };
}

if (FIREFOX && ext.webRequest && ext.webRequest.onHeadersReceived) {
  ext.webRequest.onHeadersReceived.addListener(
    (d) => headersReceived(d).catch(() => ({})),
    { urls: ["<all_urls>"], types: ["main_frame", "sub_frame"] },
    ["blocking", "responseHeaders"],
  );
}

// ---- right-click menus -------------------------------------------------------------------

// Created at install and update (removed first: they persist, and a second create fails), in
// the browser's language (_locales, from the app's catalogs by tools/extension/locales.py).
function createMenus() {
  ext.contextMenus.removeAll(() => {
    const add = (id, contexts, key) =>
      ext.contextMenus.create({ id, title: t(key), contexts }, () => void ext.runtime.lastError);
    add("mdm-link", ["link", "video", "audio", "image"], "menu");
    add("mdm-selected", ["selection"], "menu_selected");
    add("mdm-all", ["page"], "menu_all");
  });
}

// The links in the page (all of them, or those in the selection), collected in the page.
function pageLinks(selectedOnly) {
  const ok = (h) => /^(https?|ftps?|ftpes):/i.test(h);
  let anchors = [...document.querySelectorAll("a[href]")];
  if (selectedOnly) {
    const sel = getSelection();
    anchors = sel.rangeCount ? anchors.filter((a) => sel.containsNode(a, true)) : [];
  }
  const seen = new Set();
  const links = [];
  for (const a of anchors) {
    if (ok(a.href) && !seen.has(a.href)) {
      seen.add(a.href);
      links.push(a.href);
    }
  }
  for (const m of document.querySelectorAll("video[src], audio[src], source[src]")) {
    if (ok(m.src) && !seen.has(m.src) && !selectedOnly) {
      seen.add(m.src);
      links.push(m.src);
    }
  }
  return links;
}

// When the user asked and it couldn't be done: the help page's section opens beside (a link
// XDM refused has none of its own: the connection's).
const SECTIONS = ["not-installed", "forbidden", "not-running", "host-failed", "too-large", "update-app", "options"];
function explain(why) {
  const at = SECTIONS.includes(why) ? why : "host-failed";
  ext.tabs.create({ url: `${HELP}#${at}` }).catch(() => {});
}

ext.contextMenus.onClicked.addListener(async (info, tab) => {
  const incognito = Boolean(tab && tab.incognito);
  const storeId = tab && tab.cookieStoreId;
  if (info.menuItemId === "mdm-link") {
    const url = info.linkUrl || info.srcUrl;
    // A player's own stream (blob:, a MediaSource) or a stream's manifest: the Download video
    // window, which reads streams (a manifest downloaded as a file is only a short list).
    // The manifest's own cookies (its server may not be the page's), as the media list sends.
    const topPage = tab && tab.url && /^https?:/i.test(tab.url) ? tab.url : info.pageUrl || "";
    const tabTitle = tab && tab.title ? String(tab.title).slice(0, 300) : "";
    if (url && MANIFEST.test(url)) {
      const page = info.frameUrl || info.pageUrl || "";
      const reply = await native({
        video: true,
        explicit: true,
        url: info.pageUrl,
        src: url,
        ...(tabTitle ? { title: tabTitle } : {}),
        ...videoRefExtras(url, info.pageUrl || "", page, topPage),
        ...(await videoCookies(url, { incognito, storeId })),
        private: incognito,
      });
      if (!reply.ok && reply.why !== "options") explain(reply.why);
      return;
    }
    // A player playing from a blob: has no address yt-dlp can read: the stream its frame loaded,
    // as the button over videos sends (#155: the page alone was sent, and yt-dlp, not knowing
    // the site, answered "Unsupported URL"). Firefox names no address at all for a player fed
    // by a MediaSource (nothing was sent then).
    const player = info.mediaType === "video" || info.mediaType === "audio";
    if ((url && /^(blob|mediastream):/i.test(url)) || (player && !url)) {
      const page = info.frameUrl || info.pageUrl;
      await mediaBack;
      const src = await streamFor({ tab, frameId: info.frameId || 0 });
      const reply = await native({
        video: true,
        explicit: true,
        url: page,
        ...(src ? { src } : {}),
        ...(tabTitle ? { title: tabTitle } : {}),
        referrer: info.pageUrl && info.pageUrl !== page ? info.pageUrl : "",
        ...videoRefExtras(src, page, page, topPage),
        ...(await videoCookies(src || page, { incognito, storeId })),
        private: incognito,
      });
      if (!reply.ok && reply.why !== "options") explain(reply.why);
      return;
    }
    // ftp links too: browsers no longer download them, XDM does.
    if (!url || !/^(https?|ftps?|ftpes):/i.test(url)) return;
    const reply = await handOff({ url, referrer: info.pageUrl, incognito, storeId, explicit: true });
    if (!reply.ok) explain(reply.why);
    return;
  }
  if ((info.menuItemId === "mdm-selected" || info.menuItemId === "mdm-all") && tab) {
    let links = [];
    try {
      const [res] = await ext.scripting.executeScript({
        target: { tabId: tab.id, frameIds: [info.frameId || 0] },
        func: pageLinks,
        args: [info.menuItemId === "mdm-selected"],
      });
      links = (res && res.result) || [];
    } catch {
      // A page extensions may not read (the browser's own, a store).
    }
    if (!links.length) return;
    const reply = await native({ links, referrer: info.pageUrl, cookies: await cookieHeader(info.pageUrl, { incognito, storeId }), private: incognito });
    if (!reply.ok) explain(reply.why);
  }
});

// ---- install, start, the toolbar popup ------------------------------------------------------

ext.runtime.onInstalled.addListener(async (details) => {
  createMenus();
  // Hello: the app learns the extension is there (its offer window says Connected).
  const r = await refreshRules();
  if (details.reason === "install" && !r) {
    const { why } = await readStatus();
    explain(why || "not-installed");
  }
  injectOpenTabs();
});

ext.runtime.onStartup.addListener(() => {
  refreshRules();
  showStatus();
});

// A match pattern's host and path as a test (`*://*.youtube.com/*`).
function matcher(pattern) {
  const [, host = "", path = "/*"] = /^[^:]+:\/\/([^/]*)(\/.*)?$/.exec(pattern) || [];
  const glob = (s) => s.split("*").map((x) => x.replace(/[.+?^${}()|[\]\\]/g, "\\$&")).join(".*");
  const h = host.startsWith("*.") ? new RegExp(`^(.*\\.)?${glob(host.slice(2))}$`, "i") : new RegExp(`^${glob(host)}$`, "i");
  const pth = new RegExp(`^${glob(path)}$`);
  return (u) => h.test(u.hostname) && pth.test(u.pathname + u.search);
}

// Whether this copy of the extension leaves the video button off `url` (the Chrome Web Store's
// copy: its policy, tools/extension/pack.ps1). Read from its manifest, so the popup says what is true.
function videoKeptOff(url) {
  const entry = (ext.runtime.getManifest().content_scripts || []).find((e) => (e.js || []).includes("video.js"));
  try {
    const u = new URL(url);
    return ((entry && entry.exclude_matches) || []).some((p) => matcher(p)(u));
  } catch {
    return false;
  }
}

// Pages open before the extension was installed or updated get its scripts now (else the
// button waits for a reload).
async function injectOpenTabs() {
  if (!ext.scripting) return;
  const tabs = await ext.tabs.query({ url: ["http://*/*", "https://*/*"] }).catch(() => []);
  for (const entry of ext.runtime.getManifest().content_scripts || []) {
    const excludes = (entry.exclude_matches || []).map(matcher);
    for (const tab of tabs) {
      let u;
      try {
        u = new URL(tab.url);
      } catch {
        continue;
      }
      if (excludes.some((m) => m(u))) continue;
      ext.scripting
        .executeScript({ target: { tabId: tab.id, allFrames: Boolean(entry.all_frames) }, files: entry.js })
        .catch(() => {});
    }
  }
}

// ---- messages from the page (video.js, page.js) and the popup (popup.js) -------------------

// The download button over videos asks: may it show on this page, the button was clicked, its
// menu. The app decides through the native host; its answer for a site is kept a minute (one
// question per page load at most). A failure to reach it isn't kept.
const panelCache = new Map();
async function panelOn(page) {
  // The host name without a port: as video.js names the site, and as the app matches sites.
  const site = new URL(page).hostname;
  forget(panelCache, 60000);
  const kept = panelCache.get(site);
  if (kept) return kept.on;
  const reply = await native({ panel: true, url: page });
  if (reply.ok || !reply.why) panelCache.set(site, { on: Boolean(reply.ok), at: Date.now() });
  return Boolean(reply.ok);
}

// The page the message came from, as the browser knows it (never what the page says). The top
// frame's is its tab's address, which follows a page that moves in place (history.pushState:
// YouTube, most video sites). `sender.url` is the document's and goes stale there: in Chromium it
// stays the address the document loaded at, in Firefox it lags a navigation behind, so the button
// sent the video watched before this one.
function senderPage(sender) {
  const top = sender.frameId === 0 && sender.tab && sender.tab.url;
  const u = top || sender.url || (sender.tab && sender.tab.url) || "";
  return /^https?:/i.test(u) ? u : null;
}

// A YouTube page's player, asked for what it plays. In the page's own world, the only place its
// player's API exists (content scripts are isolated from it, and an injected <script> is refused
// by its Trusted Types; `scripting` isn't). Run in the page: no outside names, only its arguments.
function playingAt(x, y) {
  let best = null;
  for (const v of document.querySelectorAll("video")) {
    const r = v.getBoundingClientRect();
    if (x < r.left || x > r.right || y < r.top || y > r.bottom) continue;
    if (!best || r.width * r.height < best.area) best = { v, area: r.width * r.height };
  }
  const player = best && best.v.closest(".html5-video-player");
  try {
    return player && player.getVideoData ? String(player.getVideoData().video_id || "") : "";
  } catch {
    return "";
  }
}

const isYouTube = (url) => /(^|\.)(youtube\.com|youtube-nocookie\.com|youtu\.be)$/i.test(siteOf(url));

// The address yt-dlp lists a YouTube page's whole list by (video.js listRows): a playlist's own
// page, or for a mix (RD…, made from a video; it has no page of its own) the video with its list,
// which the app looks up as a list. "" when the page is in none.
function youTubeListAddress(page) {
  try {
    const url = new URL(page);
    if (!/^(www\.|m\.|music\.)?youtube\.com$/i.test(url.hostname)) return "";
    const list = url.searchParams.get("list") || "";
    if (!/^[\w-]{2,64}$/.test(list) || list === "WL" || list === "LL") return "";
    if (/^RD/.test(list)) {
      const v = url.searchParams.get("v") || (list.length === 13 ? list.slice(2) : "");
      return /^[\w-]{11}$/.test(v) ? `https://www.youtube.com/watch?v=${v}&list=${list}` : "";
    }
    return `https://www.youtube.com/playlist?list=${list}`;
  } catch {
    return "";
  }
}

// The watch address of what the player at `at` (the video's middle, in the sender's frame) plays,
// or "": a page without such a player, one too old for `world`, a page extensions may not read.
async function playingVideo(sender, at) {
  if (!at || !ext.scripting || !sender.tab) return "";
  try {
    const [found] = await withTimeout(
      ext.scripting.executeScript({
        target: { tabId: sender.tab.id, frameIds: [sender.frameId || 0] },
        world: "MAIN",
        func: playingAt,
        args: [Number(at.x) || 0, Number(at.y) || 0],
      }),
      3000,
    );
    const id = found && found.result;
    return /^[\w-]{6,32}$/.test(id || "") ? `https://www.youtube.com/watch?v=${id}` : "";
  } catch {
    return "";
  }
}

// Instant MAIN-world YouTube player metadata extractor: reads current video_id, title, duration,
// and streamingData formats directly from `.html5-video-player` without waiting for a network fetch.
function youtubeProbeAt(x, y) {
  let best = null;
  for (const v of document.querySelectorAll("video")) {
    const r = v.getBoundingClientRect();
    if (x < r.left || x > r.right || y < r.top || y > r.bottom) continue;
    if (!best || r.width * r.height < best.area) best = { v, area: r.width * r.height };
  }
  const player = best && best.v.closest(".html5-video-player");
  try {
    const vd = player && player.getVideoData ? player.getVideoData() : null;
    const id = String((vd && vd.video_id) || "");
    let pr = player && player.getPlayerResponse ? player.getPlayerResponse() : null;
    if (pr && pr.videoDetails && id && pr.videoDetails.videoId && pr.videoDetails.videoId !== id) pr = null;
    if (!pr && globalThis.ytInitialPlayerResponse) {
      const init = globalThis.ytInitialPlayerResponse;
      if (!id || (init.videoDetails && init.videoDetails.videoId === id)) pr = init;
    }
    const details = (pr && pr.videoDetails) || {};
    const title = String((vd && vd.title) || details.title || "");
    const vidDur = best && best.v && Number.isFinite(best.v.duration) ? best.v.duration : 0;
    const playerDur = player && player.getDuration ? Number(player.getDuration()) || 0 : 0;
    const duration = Number(details.lengthSeconds) || playerDur || vidDur || 0;
    const live = Boolean(details.isLive || (details.isLiveContent && !(duration > 0)));
    const sd = (pr && pr.streamingData) || {};
    const streams = [];
    const pushAll = (list, muxed) => {
      if (!Array.isArray(list)) return;
      for (const f of list) {
        if (!f || !f.mimeType) continue;
        const mime = String(f.mimeType);
        const cm = /codecs="([^"]+)"/i.exec(mime);
        const codecs = (cm ? cm[1] : "").split(",").map((s) => s.trim()).filter(Boolean);
        const isVideo = /^video\//i.test(mime);
        const isAudio = /^audio\//i.test(mime);
        const ext = /webm/i.test(mime) ? "webm" : isAudio && /mp4/i.test(mime) ? "m4a" : "mp4";
        // The average rate gives the size a whole file will have; the peak (`bitrate`) overstates it.
        const bitrate = Number(f.averageBitrate) || Number(f.bitrate) || 0;
        const fmtDur = Number(f.approxDurationMs) > 0 ? Number(f.approxDurationMs) / 1000 : duration;
        const bytes = Number(f.contentLength) || (fmtDur > 0 && bitrate > 0 ? Math.round((bitrate / 8) * fmtDur) : 0);
        // Some adaptive formats name their height only in the label ("2160p60 HDR", "1440p"):
        // read it there too, so 4K/2K/8K are never dropped as height 0.
        const qLabel = String(f.qualityLabel || "");
        const labelHeight = Number((/^(\d{3,4})p/i.exec(qLabel) || [])[1]) || 0;
        const labelFps = Number((/^\d{3,4}p(\d{2,3})/i.exec(qLabel) || [])[1]) || 0;
        const transfer = String((f.colorInfo && f.colorInfo.transferCharacteristics) || "");
        const hdr = isVideo && (/HDR/i.test(qLabel) || /SMPTEST2084|ARIB_STD_B67|HLG|PQ/i.test(transfer));
        streams.push({
          formatId: String(f.itag || ""),
          ext,
          height: Number(f.height) || labelHeight,
          width: Number(f.width) || 0,
          fps: Number(f.fps) || labelFps,
          ...(hdr ? { hdr: true } : {}),
          vcodec: isVideo ? codecs[0] || ext : "none",
          acodec: isVideo ? (muxed ? codecs[1] || "mp4a" : "none") : codecs[0] || ext,
          tbr: bitrate > 0 ? Math.round(bitrate / 1000) : 0,
          size: bytes,
        });
      }
    };
    pushAll(sd.formats, true);
    pushAll(sd.adaptiveFormats, false);
    const thumbs = (details.thumbnail && Array.isArray(details.thumbnail.thumbnails) && details.thumbnail.thumbnails) || [];
    const thumb = thumbs.length ? String(thumbs[thumbs.length - 1].url || "") : "";
    return {
      id: id || String(details.videoId || ""),
      title,
      duration,
      live,
      ...(details.author ? { channel: String(details.author) } : {}),
      ...(/^https:\/\//.test(thumb) ? { thumbnail: thumb } : {}),
      formats: streams,
    };
  } catch {
    return null;
  }
}

async function youtubePlayerProbe(sender, at) {
  if (!at || !ext.scripting || !sender.tab) return null;
  try {
    const [found] = await withTimeout(
      ext.scripting.executeScript({
        target: { tabId: sender.tab.id, frameIds: [sender.frameId || 0] },
        world: "MAIN",
        func: youtubeProbeAt,
        args: [Number(at.x) || 0, Number(at.y) || 0],
      }),
      2000,
    );
    return (found && found.result) || null;
  } catch {
    return null;
  }
}

// Synthesize immediate Tier-1 format entries from already-captured HLS/DASH/direct media items.
async function instantMediaProbe(sender, src, fallbackDuration = 0, fallbackHeight = 0) {
  const tab = sender.tab && media.get(sender.tab.id);
  const directDur = Number(fallbackDuration) > 0 ? Number(fallbackDuration) : 0;
  const directHeight = Number(fallbackHeight) > 0 ? Math.round(Number(fallbackHeight)) : 0;
  if (!tab || !tab.items.length) {
    return directDur > 0 ? { duration: directDur, live: false, formats: [] } : null;
  }
  const match =
    (src && tab.items.find((it) => it.url === src)) ||
    tab.items.find((it) => isStream(it) && it.frameId === sender.frameId && !(it.info && it.info.children && it.info.children.length === 0 && tab.items.some((p) => p !== it && p.info && p.info.children && p.info.children.includes(pathOf(it.url))))) ||
    tab.items.find((it) => isStream(it) && it.frameId === sender.frameId) ||
    tab.items.find(isStream) ||
    tab.items.find((it) => !it.piece && (it.kind === "video" || it.kind === "audio") && (!src || it.url === src)) ||
    tab.items.find((it) => (it.kind === "video" || it.kind === "audio") && (!src || it.url === src));
  if (!match) return directDur > 0 ? { duration: directDur, live: false, formats: [] } : null;
  if (needsProbe(match)) {
    match.info = await probe(match).catch(() => ({ failed: true, at: Date.now() }));
    keepMedia();
  }
  const info = match.info;
  if (info && !info.failed && Array.isArray(info.variants) && info.variants.length) {
    let duration = Math.max(Number(info.duration) || 0, directDur);
    if (!duration && !info.live && info.children && info.children.length) {
      const childItem = tab.items.find((it) => it !== match && info.children.includes(pathOf(it.url)) && it.info && it.info.duration > 0);
      if (childItem) duration = Number(childItem.info.duration) || 0;
    }
    const isVodHls = match.kind === "hls" && !info.live;
    const audioTracks = isVodHls && Array.isArray(info.audioTracks) ? info.audioTracks : [];
    const defaultAudio =
      audioTracks.find((a) => a && a.default && a.uri) ||
      audioTracks.find((a) => a && a.autoselect && a.uri) ||
      audioTracks.find((a) => a && a.uri) ||
      null;
    const segExt = info.segmentExt || "";
    const formats = info.variants
      .filter((v) => v.height > 0 || v.bandwidth > 0 || info.variants.length === 1)
      .map((v, idx) => {
        const forceMp4 = needsMp4Container(v.codecs);
        const h = Number(v.height) || (info.variants.length === 1 && directHeight > 0 ? directHeight : 0);
        return {
          formatId: isVodHls ? `hls-${h || idx}` : match.kind === "dash" ? `dash-${h || idx}` : `live-${h || idx}`,
          ext: isVodHls
            ? forceMp4
              ? "mp4"
              : segExt || (/mp4/i.test(v.mimeType || "") ? "mp4" : "ts")
            : /webm/i.test(v.mimeType || "")
              ? "webm"
              : "mp4",
          protocol: isVodHls ? "m3u8_native" : match.kind === "dash" ? "http_dash_segments" : "m3u8_live",
          height: h,
          fps: 0,
          vcodec: (v.codecs || "avc1").split(",")[0].trim(),
          acodec: isVodHls && info.hasSeparateAudio ? "none" : "mp4a",
          // The mean rate when the playlist says it, else the peak. Either way an estimate: no size
          // here, so the menu works it out from the rate and the duration and marks it "~" (a size
          // given here was shown as exact, and a peak made it up to twice the real file).
          tbr: (v.average || v.bandwidth) > 0 ? Math.round((v.average || v.bandwidth) / 1000) : 0,
          size: 0,
          ...(isVodHls && defaultAudio && defaultAudio.uri ? { hlsAudio: defaultAudio.uri } : {}),
        };
      });
    if (isVodHls && info.hasSeparateAudio) {
      formats.push({
        formatId: "hls-audio",
        ext: "m4a",
        protocol: "m3u8_native",
        height: 0,
        fps: 0,
        vcodec: "none",
        acodec: "mp4a",
        tbr: 128,
        size: duration > 0 ? Math.round((128000 / 8) * duration) : 0,
        ...(defaultAudio && defaultAudio.uri ? { hlsAudio: defaultAudio.uri } : {}),
      });
    }
    // A `piece` is one segment of this very stream, not a format beside it: the dropdown offers
    // the stream's own variants, and the popup's list is where the segments are shown.
    for (const d of tab.items.filter((it) => it !== match && !it.piece && (it.kind === "video" || it.kind === "audio"))) {
      const ext = extensionOf(d.url.split(/[?#]/)[0]) || mimeTypeExt(d.mime) || "mp4";
      formats.push({
        formatId: `direct-${ext}`,
        ext,
        height: 0,
        fps: 0,
        vcodec: d.kind === "audio" ? "none" : ext,
        acodec: "aac",
        tbr: 0,
        size: d.size || 0,
        url: d.url,
      });
    }
    return {
      duration,
      live: Boolean(info.live),
      formats,
      ...(audioTracks.length ? { audioTracks } : {}),
    };
  }
  if (match.kind === "video" || match.kind === "audio") {
    const ext = extensionOf(match.url.split(/[?#]/)[0]) || mimeTypeExt(match.mime) || "mp4";
    return {
      duration: directDur,
      live: false,
      formats:
        match.size > 0
          ? [{ formatId: "direct", ext, height: 0, fps: 0, vcodec: match.kind === "audio" ? "none" : ext, acodec: "aac", tbr: 0, size: match.size }]
          : [],
    };
  }
  return directDur > 0 ? { duration: directDur, live: false, formats: [] } : null;
}

// The address the app looks a video up by: the one video's, never a list's. On YouTube that is
// what the player plays; else the address the content script named (video.js videoPage), if it is
// this site's (www, m. and music. are the same site); else the page's. "" on a YouTube page where
// nothing names the video: its own address is a list (the home page, a channel) to yt-dlp.
async function videoAddress(m, page, sender) {
  const named = typeof m.url === "string" && /^https?:/i.test(m.url) ? m.url : "";
  if (isYouTube(page)) {
    const playing = await playingVideo(sender, m.at);
    return playing || (isYouTube(named) ? named : "");
  }
  const site = (u) => siteOf(u).replace(/^(www|m|music)\./, "");
  return named && site(named) === site(page) ? named : page;
}

function isGenericEmbedTitle(s, frameUrl = "") {
  const clean = String(s || "").trim();
  if (
    /\.(mp4|mkv|ts|m3u8|txt|webm|avi|mov|m4s|mpd)$/i.test(clean) ||
    /^https?:\/\//i.test(clean) ||
    /^(player|embed|video|iframe|index|stream|watch|play|hd\s*player|jw\s*player|video\.?js|plyr|dplayer|artplayer|clappr|hls\.?js|flowplayer)(\b|\s+player|\s+embed)*$/i.test(clean)
  ) {
    return true;
  }
  if (frameUrl && /^[A-Za-z0-9_-]{3,24}$/.test(clean) && frameUrl.toLowerCase().includes(clean.toLowerCase())) {
    return true;
  }
  return false;
}

function preferredVideoTitle(rawTitle, tabTitle, frameId, frameUrl = "") {
  const fromFrame = typeof rawTitle === "string" ? rawTitle.trim() : "";
  const fromTab = typeof tabTitle === "string" ? tabTitle.trim() : "";
  if (!fromFrame) return fromTab.slice(0, 300);
  if (frameId !== 0 && fromTab && isGenericEmbedTitle(fromFrame, frameUrl)) {
    return fromTab.slice(0, 300);
  }
  return fromFrame.slice(0, 300);
}

function videoRefExtras(src, own, page, tabUrl) {
  const captured = capturedRefererFor(src || own);
  const reqRef = captured || (src && page ? page : "");
  const reqOrigin = capturedOriginFor(src || own);
  const topPage = tabUrl && /^https?:/i.test(tabUrl) && (tabUrl !== own || Boolean(src) || Boolean(captured)) ? tabUrl : "";
  return {
    ...(reqRef ? { requestReferer: reqRef } : {}),
    ...(reqOrigin ? { requestOrigin: reqOrigin } : {}),
    ...(topPage ? { pageUrl: topPage } : {}),
  };
}

async function pageMessage(m, sender) {
  const page = senderPage(sender);
  const tab = sender.tab || {};
  if (m.type === "drm-hint") {
    const tabId = Number.isInteger(tab.id) ? tab.id : -1;
    if (tabId >= 0) {
      forget(drmFrames, DRM_FRAME_TTL);
      drmFrames.set(`${tabId}:${sender.frameId || 0}`, { at: Date.now() });
    }
    return { ok: true };
  }
  if (m.type === "manifest-hint" && typeof m.url === "string" && typeof m.text === "string") {
    if (!/^https?:\/\//i.test(m.url) || m.text.length === 0 || m.text.length > MANIFEST_MAX) {
      return { ok: false };
    }
    const kind = manifestKind(m.text);
    if (!kind) return { ok: false };
    const tabId = Number.isInteger(tab.id) ? tab.id : -1;
    const frameId = sender.frameId || 0;
    rememberSnapshot(m.url, m.text, kind, tabId, frameId);
    if (tabId >= 0) {
      await mediaBack;
      noteMedia(
        { tabId, frameId, url: m.url },
        {
          kind,
          mime:
            (typeof m.mime === "string" && m.mime) ||
            (kind === "hls" ? "application/vnd.apple.mpegurl" : "application/dash+xml"),
          size: m.text.length,
        },
      );
      keepMedia();
    }
    return { ok: true };
  }
  if (m.type === "panel") {
    const on = page ? await panelOn(page) : false;
    await ensureAppLocale();
    const messages = mergedCatalog(VIDEO_KEYS);
    const lang = appLang ? localeFolder(appLang) : "";
    return messages ? { on, dir: appDir(), ...(lang ? { lang } : {}), messages } : { on };
  }
  if (m.type === "link" && typeof m.url === "string") {
    const name = typeof m.name === "string" ? m.name.split(/[\\/]/).pop().slice(0, 255) : "";
    return link(m.url, page || "", Boolean(tab.incognito), name);
  }
  // Content scripts (`page.js`) ask when a page loads or returns to foreground so added/removed
  // file types in Options reach `storage.local` and `syncOpenedRule()` right away.
  if (m.type === "rules") {
    const r = await storedRules();
    if ((!r || Date.now() - r.at > RULES_TTL_MS) && Date.now() - rulesAskedAt > RULES_TTL_MS) {
      rulesAskedAt = Date.now();
      refreshRules().catch(() => {});
    }
    return { ok: true };
  }
  if (m.type === "bypass" && typeof m.url === "string") {
    bypass.set(m.url, { at: Date.now() });
    return { ok: true };
  }
  if (m.type === "video-probe" && page) {
    await mediaBack;
    const src = typeof m.src === "string" && /^https?:/i.test(m.src) ? m.src : await streamFor(sender);
    let yt = null;
    let named = "";
    if (!m.pollOnly && isYouTube(page)) {
      yt = await youtubePlayerProbe(sender, m.at);
      if (yt && /^[\w-]{6,32}$/.test(yt.id || "")) named = `https://www.youtube.com/watch?v=${yt.id}`;
    }
    if (!named) named = await videoAddress(m, page, sender);
    if (!named && !src) return { ok: false, why: "unnamed" };
    const own = named || page;
    const instant = !m.pollOnly
      ? yt && yt.formats && yt.formats.length
        ? yt
        : await instantMediaProbe(sender, src, m.duration, m.height)
      : null;
    const title = preferredVideoTitle(
      typeof m.title === "string" ? m.title : (yt && yt.title ? yt.title : ""),
      tab.title,
      sender.frameId || 0,
      page,
    );
    const instantWithTitle = instant && title && !instant.title ? { ...instant, title } : instant;
    const isNativeVodHls = Boolean(
      instantWithTitle &&
        Array.isArray(instantWithTitle.formats) &&
        instantWithTitle.formats.length &&
        instantWithTitle.formats.some((f) => f && f.protocol === "m3u8_native"),
    );
    const desktopPromise = (async () => {
      if (m.skipDesktop || isNativeVodHls || (await remoteTarget())) return null;
      const reply = await native({
        probe: true,
        url: own,
        ...(src ? { src } : {}),
        title,
        referrer: tab.url && tab.url !== own ? tab.url : "",
        ...videoRefExtras(src, own, page, tab.url),
        ...(await videoCookies(src || own, { incognito: tab.incognito, storeId: tab.cookieStoreId }, !src && tab.url && tab.url !== own ? tab.url : "")),
        ...(tab.incognito ? { private: true } : {}),
      });
      return reply && reply.probe ? reply.probe : null;
    })();
    const desktop = instantWithTitle && instantWithTitle.formats && instantWithTitle.formats.length ? await withTimeout(desktopPromise, 300).catch(() => null) : await desktopPromise;
    return { ok: true, own, ...(src ? { src } : {}), instant: instantWithTitle, desktop };
  }
  // The whole playlist or mix the page is in (video.js listRows): the app's window lists it with
  // the chosen quality picked.
  if (m.type === "video" && page && m.list === true) {
    const listAddress = youTubeListAddress(tab.url || page);
    if (!listAddress) return { ok: false, why: "unnamed" };
    const reply = await native({
      video: true,
      list: true,
      url: listAddress,
      title: preferredVideoTitle(m.title, tab.title, sender.frameId || 0),
      ...(typeof m.selector === "string" && m.selector ? { selector: m.selector.slice(0, 200) } : {}),
      referrer: tab.url || "",
      ...(await videoCookies(listAddress, { incognito: tab.incognito, storeId: tab.cookieStoreId })),
      ...(tab.incognito ? { private: true } : {}),
    });
    return { ok: Boolean(reply.ok), why: reply.why };
  }
  if (m.type === "video" && page) {
    await mediaBack;
    const src = typeof m.src === "string" && /^https?:/i.test(m.src) ? m.src : await streamFor(sender);
    // A playing stream or file of its own still goes as `src`, with the page for its Referer.
    const named = await videoAddress(m, page, sender);
    const site = (u) => siteOf(u).replace(/^(www|m|music)\./, "");
    const fallbackOwn =
      typeof m.own === "string" && /^https?:/i.test(m.own) && (!isYouTube(page) || isYouTube(m.own)) && site(m.own) === site(page)
        ? m.own
        : "";
    if (!named && !fallbackOwn && !src) return { ok: false, why: "unnamed" };
    const own = named || fallbackOwn || page;
    const title = preferredVideoTitle(m.title, tab.title, sender.frameId || 0, page);
    const tabMedia = sender.tab && media.get(sender.tab.id);
    const matchedStream = src && tabMedia && Array.isArray(tabMedia.items) ? tabMedia.items.find((it) => it.url === src) : null;
    const isLive = Boolean(m.live || (matchedStream && matchedStream.info && matchedStream.info.live));
    const isVodHls = Boolean(
      src &&
        matchedStream &&
        matchedStream.kind === "hls" &&
        matchedStream.info &&
        !matchedStream.info.failed &&
        !isLive,
    );
    const isDisguisedVodHls = Boolean(isVodHls && !MANIFEST.test(src.split(/[?#]/)[0]));
    let effectiveSelector = typeof m.selector === "string" && m.selector ? m.selector.slice(0, 200) : "";
    let effectiveContainer = typeof m.container === "string" && m.container ? m.container.slice(0, 32) : "";
    let effectiveHlsAudio = typeof m.hlsAudio === "string" && /^https?:/i.test(m.hlsAudio) ? m.hlsAudio.slice(0, URL_MAX) : "";
    if (isDisguisedVodHls) {
      if (!effectiveSelector && m.window !== true) {
        effectiveSelector = "hls-best";
        if (!effectiveContainer) effectiveContainer = hlsPreferredContainer(matchedStream.info, effectiveSelector);
      } else if (
        effectiveSelector &&
        !/\.txt$/i.test(pathOf(src)) &&
        !effectiveSelector.toLowerCase().startsWith("hls-") &&
        !/^(mp3|opus|flac):/i.test(effectiveSelector)
      ) {
        const hm = /height<=(\d+)/i.exec(effectiveSelector);
        const wantAvc = /vcodec\^=avc/i.test(effectiveSelector);
        if (wantAvc) {
          effectiveSelector = hm ? `hls-avc-${hm[1]}` : "hls-avc";
        } else {
          effectiveSelector = hm ? `hls-${hm[1]}` : effectiveSelector === "ba" ? "hls-audio" : "hls-best";
        }
      }
      if (
        effectiveContainer.toLowerCase() === "ts" &&
        effectiveSelector.toLowerCase() !== "hls-audio" &&
        hlsPreferredContainer(matchedStream.info, effectiveSelector) === "mp4"
      ) {
        effectiveContainer = "mp4";
      }
    }
    if (
      isVodHls &&
      !effectiveHlsAudio &&
      !/^(mp3|opus|flac):/i.test(effectiveSelector) &&
      Array.isArray(matchedStream.info.audioTracks)
    ) {
      const tracks = matchedStream.info.audioTracks;
      const defAud =
        tracks.find((a) => a && a.default && a.uri) ||
        tracks.find((a) => a && a.autoselect && a.uri) ||
        tracks.find((a) => a && a.uri) ||
        null;
      if (defAud && defAud.uri) effectiveHlsAudio = defAud.uri.slice(0, URL_MAX);
    }
    const reply = await native({
      video: true,
      url: own,
      ...(src ? { src } : {}),
      title,
      ...(effectiveSelector ? { selector: effectiveSelector } : {}),
      ...(effectiveContainer ? { container: effectiveContainer } : {}),
      ...(typeof m.size === "number" && m.size > 0 ? { size: Math.round(m.size) } : {}),
      ...(typeof m.duration === "number" && Number.isFinite(m.duration) && m.duration > 0 ? { duration: m.duration } : {}),
      ...(effectiveHlsAudio ? { hlsAudio: effectiveHlsAudio } : {}),
      ...(src ? hlsSnapshotExtras(src, Number.isInteger(tab.id) ? tab.id : -1) : {}),
      ...(isLive ? { live: true } : {}),
      ...(m.window === true && !effectiveSelector ? { window: true } : {}),
      referrer: tab.url && tab.url !== own ? tab.url : "",
      ...videoRefExtras(src, own, page, tab.url),
      // The cookies of what yt-dlp is given (the app files them under its host): the stream's
      // own server may not be the page's, and the page's cookies are not for it.
      ...(await videoCookies(src || own, { incognito: tab.incognito, storeId: tab.cookieStoreId }, !src && tab.url && tab.url !== own ? tab.url : "")),
      ...(tab.incognito ? { private: true } : {}),
    });
    return { ok: Boolean(reply.ok), why: reply.why };
  }
  if (m.type === "hide-site" && page) {
    const host = new URL(page).hostname;
    panelCache.set(host, { on: false, at: Date.now() });
    const reply = await native({ hideSite: host });
    return { ok: Boolean(reply.ok) };
  }
  if (m.type === "options") return { ok: Boolean((await native({ openOptions: true })).ok) };
  return { ok: false };
}

async function popupMessage(m) {
  if (m.type === "state") {
    const r = await refreshRules();
    await ensureAppLocale();
    const { paused } = await ext.storage.local.get({ paused: false });
    const s = await readStatus();
    const { declined } = await ext.storage.local.get({ declined: null });
    const remote = await remoteTarget();
    const messages = mergedCatalog();
    const localeInfo = messages ? { lang: localeFolder(appLang), dir: appDir(), messages } : {};
    if (remote) return { connected: true, why: null, remote: remote.url, version: VERSION, paused, declined, ...localeInfo };
    return { connected: Boolean(r) && (!s.why || s.why === "update-app"), why: s.why, app: s.app, version: VERSION, paused, declined, ...localeInfo };
  }
  if (m.type === "remote-pair" && typeof m.url === "string" && typeof m.code === "string") return remotePair(m.url, m.code);
  if (m.type === "remote-off") return remoteOff();
  if (m.type === "pause") {
    await ext.storage.local.set({ paused: Boolean(m.paused) });
    await syncOpenedRule();
    await showStatus();
    return { ok: true };
  }
  if (m.type === "exclude-site" && typeof m.host === "string" && m.host) {
    const reply = await native({ excludeSite: m.host });
    await refreshRules();
    return { ok: Boolean(reply.ok) };
  }
  if (m.type === "open-app") return { ok: Boolean((await native({ open: true })).ok) };
  if (m.type === "open-options") return { ok: Boolean((await native({ openOptions: true, page: "general" })).ok) };
  // The media the popup's tab loaded (#38), and one of them handed to the app.
  if (m.type === "media" && Number.isInteger(m.tab)) {
    const list = await mediaList(m.tab);
    const tab = await ext.tabs.get(m.tab).catch(() => null);
    return {
      media: list,
      title: tab && typeof tab.title === "string" ? tab.title : "",
      // Why the list is empty, when there's more to say than "nothing yet": this copy keeps the
      // video button off the site, or the player streams pieces (blob:) no address can name.
      keptOff: Boolean(tab && videoKeptOff(tab.url)),
      streamed: !list.length && (await playsStream(m.tab)),
    };
  }
  if (m.type === "media-clear" && Number.isInteger(m.tab)) {
    await mediaBack;
    clearDrmForTab(m.tab);
    const kept = media.get(m.tab);
    if (kept) {
      kept.items = [];
      kept.maybe = [];
      keepMedia();
    }
    return { ok: true };
  }
  if (m.type === "media-download" && Number.isInteger(m.tab) && typeof m.url === "string") {
    const reply = await mediaDownload(m.tab, m.url);
    return { ok: Boolean(reply.ok), why: reply.why };
  }
  if (m.type === "media-page" && Number.isInteger(m.tab)) {
    if (typeof m.url === "string" && m.url) {
      const reply = await mediaDownload(m.tab, m.url, true);
      return { ok: Boolean(reply.ok), why: reply.why };
    }
    const tab = await ext.tabs.get(m.tab).catch(() => null);
    if (!tab || !/^https?:/i.test(tab.url || "")) return { ok: false, why: "gone" };
    if (videoKeptOff(tab.url)) return { ok: false, why: "options" };
    const ctx = { incognito: Boolean(tab.incognito), storeId: tab.cookieStoreId };
    const reply = await native({
      video: true,
      explicit: true,
      window: true,
      url: tab.url,
      title: String(tab.title || "").slice(0, 300),
      ...(await videoCookies(tab.url, ctx)),
      ...(ctx.incognito ? { private: true } : {}),
    });
    return { ok: Boolean(reply.ok), why: reply.why };
  }
  return { ok: false };
}

ext.runtime.onMessage.addListener((m, sender, sendResponse) => {
  // Only our own scripts (pages can't message an extension without being allowed to).
  if (!m || typeof m.type !== "string" || sender.id !== ext.runtime.id) return false;
  // The popup, or its page opened in a tab (popup.html?page=), is one of the extension's own
  // pages; content scripts send from the web page they are in.
  const own = String(sender.url || "").startsWith(ext.runtime.getURL(""));
  const fromPage = Boolean(sender.tab) && !own;
  (fromPage ? pageMessage(m, sender) : popupMessage(m)).then(sendResponse, () => sendResponse({ ok: false }));
  return true; // answered later
});

showStatus();
// Rules kept from before, else asked now (a first start, storage cleared).
storedRules().then((r) => (r ? syncOpenedRule() : refreshRules()));
// Refresh Options' file types & exclusions whenever the user switches back to the browser window
// after editing Options in the desktop app.
if (ext.windows && ext.windows.onFocusChanged) {
  ext.windows.onFocusChanged.addListener((winId) => {
    if (winId === ext.windows.WINDOW_ID_NONE) return;
    storedRules().then((r) => {
      if (!r || Date.now() - r.at > 1500) refreshRules().catch(() => {});
    });
  });
}
