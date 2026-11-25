// Xenon Download Manager's download button over videos (a content script, in every page and
// frame). Our own design: a small pill at the video's edge that appears while the pointer is
// over the video (or it has the keyboard's focus) and fades when the pointer rests or leaves.
// Clicking it opens the app's Download video window for this page (an embedded player's own
// address, which yt-dlp understands), with the video's own address when it has a plain one. It can be dragged along the video's edge, and stays where
// it was put on that site. Its menu hides it on this video, keeps it off the site (the app's
// Options → General → Download panels lists those sites), or opens those settings. The app
// decides where it may show at all (the native host: integration on, this browser captured,
// the button on, the site not excluded).
(() => {
  // One button per page: a copy cut off from the extension (after an update) gives way to the
  // new one.
  const prev = globalThis.__mdmVideoButton;
  if (prev && typeof prev === "object" && prev.alive()) return;
  if (prev && typeof prev.teardown === "function") prev.teardown();
  const ext = globalThis.browser ?? globalThis.chrome;

  // A video worth a button: big enough to be content, not a short muted looping clip (an
  // animated picture or a background). A sound player (<audio> with controls) gets one too.
  const MIN_W = 200;
  const MIN_H = 112;
  const MIN_AUDIO_H = 24;
  const CLIP_S = 15;
  // Shown this long after the pointer last moved over the video, kept while it is on the button.
  const IDLE_MS = 2600;
  const INSET = 10;
  const DRAG_PX = 4;
  // Our app icon (extension/icons/icon-48.png, as data: no extension file is exposed to pages).
  const LOGO = "data:image/png;base64,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";

  let appMessages = null;
  let appDir = null;
  let appLang = "";
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
  const t = (key, ...subs) => formatCatalog(key, subs) || ext.i18n.getMessage(key, subs) || FALLBACK[key] || key;
  const uiLang = () => (appLang || ext.i18n.getUILanguage?.() || "en").split(/[-_]/)[0].toLowerCase();
  const isTr = () => uiLang() === "tr";
  const uiText = (en, tr) => (isTr() ? tr : en);
  const FALLBACK = {
    video_download: "Download video",
    video_more: "More options",
    video_hide_here: "Hide on this video",
    video_settings: "Button settings...",
    video_sent: "Sent to Xenon Download Manager",
    video_failed: "Xenon Download Manager isn't installed or didn't start",
    video_off: "The download button is off on this site",
    video_drm: "Protected video: it can't be saved",
    video_unnamed: "Can't tell which video this is: open its own page and try there",
    video_drag: "Drag to move it along the video's edge",
  };
  // To the background. After the extension was updated or removed, this page's copy of the script
  // is cut off from it (calls throw at once): then it takes itself away.
  const send = (m) => {
    try {
      if (!ext.runtime?.id) throw new Error("gone");
      return ext.runtime.sendMessage(m).catch(() => ({ ok: false }));
    } catch {
      teardown();
      return Promise.resolve({ ok: false, gone: true });
    }
  };
  const host = location.hostname;
  // Relay passive MAIN-world manifest captures (`media-observer.js`) to the background worker so
  // single-use tokenized HLS/DASH manifests are cached without re-fetching them.
  const onManifestHint = (ev) => {
    if (ev.source !== window || !ev.data) return;
    if (ev.data.channel === "xdm-drm-hint-v1") {
      send({ type: "drm-hint" });
      return;
    }
    if (ev.data.channel !== "xdm-media-hint-v1") return;
    if (typeof ev.data.url !== "string" || typeof ev.data.text !== "string") return;
    send({
      type: "manifest-hint",
      url: ev.data.url,
      mime: typeof ev.data.mime === "string" ? ev.data.mime : "",
      text: ev.data.text,
    });
  };
  window.addEventListener("message", onManifestHint);
  // The address the app looks the video up by — the video's, not the page's. Most pages that play
  // a video are not the video's page: YouTube's home, a search, a channel or a playlist carries a
  // list in its address ("recommended", the results, the channel's videos), and yt-dlp reads such
  // an address as the list, so the app offered the whole feed instead of the video the button was
  // on. A watch page opened from a mix or a playlist has the same problem one level down
  // (…&list=…&index=…): its own page is watch?v=<id>, and nothing else.
  //
  // Null when the page plays a video but its DOM doesn't name it: YouTube's channel masthead player
  // has no video-id attribute, no link to the video inside the player and a blob: source. The page
  // itself never goes to the app then (a channel is a whole list to yt-dlp): the background asks
  // the player what it plays, in the page's own world (background.js playingVideo), and this
  // answer is only what it falls back on.
  function videoPage(v) {
    const watch = (id) =>
      /^[\w-]+$/.test(id || "") ? `https://www.youtube.com/watch?v=${id}` : null;
    try {
      const url = new URL(location.href);
      const name = url.hostname.replace(/^(www|m|music)\./, "");
      if (name === "youtu.be") {
        const id = url.pathname.split("/")[1];
        if (watch(id)) return `https://youtu.be/${id}`;
      }
      if (name === "youtube.com" || name === "youtube-nocookie.com") {
        // An embed's own page is the video's (…/embed/<id>).
        if (url.pathname.startsWith("/embed/")) {
          const id = watch(url.pathname.split("/")[2]);
          if (id) return id;
        }
        // A Short's or a live stream's own page (/shorts/<id>, /live/<id>). yt-dlp reads these as
        // the same one video as watch?v=<id>.
        const [, kind, pathId] = url.pathname.split("/");
        if (kind === "shorts" || kind === "live") {
          const id = watch(pathId);
          if (id) return id;
        }
        // The page is this video's own: a watch page, with or without a list on it.
        const own = watch(url.searchParams.get("v"));
        if (own && url.pathname.startsWith("/watch")) return own;
        const shown = watch(document.querySelector("ytd-watch-flexy")?.getAttribute("video-id"));
        if (shown && url.pathname.startsWith("/watch")) return shown;
        const player = watch(document.querySelector("#movie_player")?.getAttribute("video-id"));
        if (player) return player;
        // A player inside the card it belongs to (a feed, a search, a playlist): the card links to
        // the video. Only a container that names exactly one video counts — walking on into the
        // page's column would find some other card's link and send the wrong video, which is worse
        // than sending none.
        for (let node = v, i = 0; node && i < 12; i += 1, node = node.parentElement) {
          const ids = new Set();
          for (const a of node.querySelectorAll?.('a[href*="/watch?"], a[href*="youtu.be/"]') || []) {
            const link = new URL(a.href, location.href);
            const id = link.hostname.endsWith("youtu.be")
              ? link.pathname.split("/")[1]
              : link.searchParams.get("v");
            if (watch(id)) ids.add(id);
          }
          if (ids.size === 1) return watch([...ids][0]);
          if (ids.size > 1) break;
        }
        // A page that plays a video and names none: not the video's page, so not the app's.
        return null;
      }
    } catch {
      // Not an address this page can reason about: its own follows.
    }
    return location.href;
  }

  // The address this page first loaded at (`download`: a page that moves in place keeps its first
  // page's og:title).
  const landedAt = location.href;
  const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;

  // "Hide on this video": the video's address (players reuse one element for the next video),
  // else the element.
  const dismissed = new WeakSet();
  const dismissedSrc = new Set();
  const isDismissed = (v) => dismissed.has(v) || (v.currentSrc && dismissedSrc.has(v.currentSrc));
  let allowed = null; // asked once per page: may it show here?
  let place = { x: 1, y: 0 }; // where on the edge: fractions of the room, top right by default
  let chosen = false; // the user put it where it is (dragged it, on this site): theirs, never moved for a page's control
  let auto = { v: null, at: null, t: 0 }; // the spot found for this video, kept for a moment
  const AUTO_MS = 400;
  let video = null; // the video it is over
  let lastMove = 0;
  let pointer = { x: -1, y: -1 };
  let menuOpen = false;
  let dragging = null;
  let busy = false;
  let flashing = false;
  let raf = 0;
  let scan = 0;
  // Kept on screen while the keyboard is on it (not after a mouse click: that leaves focus too),
  // or on the video's player (its own controls).
  const keyboardOn = () =>
    Boolean(shadow.activeElement && shadow.activeElement.matches(":focus-visible")) || playerFocused();
  let focusedPlayer = null;
  const playerFocused = () =>
    Boolean(video && focusedPlayer && document.activeElement === focusedPlayer && focusedPlayer.contains(video));

  let idleLabel = t("video_download");
  let activeTitle = "";
  let probeState = { own: "", src: "", data: null, loading: false, status: "" };
  let probeSeq = 0;
  let pollTimer = 0;
  let lastRowSig = "";
  let menuPressing = false;
  let pendingRender = false;
  const probeCache = new Map();
  const isYouTubePage = () => /(?:^|\.)(youtube\.com|youtube-nocookie\.com|youtu\.be)$/i.test(host);

  function cleanVideoTitle(raw) {
    return String(raw || "")
      .replace(/^\(\d+\)\s+/, "")
      .replace(/\s+[-–|]\s*YouTube$/i, "")
      .trim();
  }

  function currentPageTitle() {
    const og = location.href === landedAt ? document.querySelector('meta[property="og:title"]')?.content : "";
    const ytHeading = document.querySelector("h1.ytd-watch-metadata yt-formatted-string, #title h1 yt-formatted-string")?.textContent || "";
    const doc = ytHeading || og || document.title || "";
    return cleanVideoTitle(doc);
  }

  function activeDuration() {
    const pd = Number(probeState.data && probeState.data.duration) || 0;
    if (pd > 0) return pd;
    return mediaMeta(video).duration;
  }

  function formatDuration(sec) {
    const total = Math.round(Number(sec) || 0);
    if (total <= 0) return "";
    const h = Math.floor(total / 3600);
    const m = Math.floor((total % 3600) / 60);
    const s = total % 60;
    const pad = (n) => String(n).padStart(2, "0");
    return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
  }

  function refreshPillLabel() {
    if (flashing || pill.classList.contains("ok") || pill.classList.contains("err")) return;
    const title = activeTitle || currentPageTitle();
    const prefix = isTr() ? "İndir" : "Download";
    label.textContent = title ? `${prefix}: ${title}` : idleLabel;
  }

  function formatSize(bytes, approx = false) {
    const b = Number(bytes) || 0;
    if (b <= 0) return "";
    const pfx = approx ? "~" : "";
    if (b >= 1024 * 1024 * 1024) return `${pfx}${(b / (1024 * 1024 * 1024)).toFixed(2)} GB`;
    if (b >= 1024 * 1024) return `${pfx}${(b / (1024 * 1024)).toFixed(1)} MB`;
    return `${pfx}${Math.max(1, Math.round(b / 1024))} KB`;
  }

  function formatBytesOf(f, duration) {
    if (!f) return { size: 0, approx: false };
    const exact = Number(f.size) || Number(f.filesize) || 0;
    if (exact > 0) return { size: exact, approx: false };
    const approxBytes = Number(f.filesize_approx) || 0;
    if (approxBytes > 0) return { size: approxBytes, approx: true };
    const tbr = Number(f.tbr) || 0;
    if (duration > 0 && tbr > 0) return { size: Math.round(tbr * 125 * duration), approx: true };
    return { size: 0, approx: false };
  }

  // A height as people name it: the tier and its lines above 1080p ("4K (2160p)"), the lines
  // alone below.
  function resolutionLabel(h) {
    if (h >= 4320) return "8K (4320p)";
    if (h >= 2880) return "5K (2880p)";
    if (h >= 2160) return "4K (2160p)";
    if (h >= 1440) return "2K (1440p)";
    return `${h}p`;
  }

  // The short badge on a row: 8K, 5K, 4K, 2K, else the lines (1080p ... 144p).
  function resolutionBadge(h) {
    if (h >= 4320) return "8K";
    if (h >= 2880) return "5K";
    if (h >= 2160) return "4K";
    if (h >= 1440) return "2K";
    return h > 0 ? `${h}p` : "";
  }

  // HD, Full HD, UHD: what the picture is, under the badge.
  function resolutionClass(h) {
    if (h >= 4320) return "UHD-2";
    if (h >= 2160) return "UHD";
    if (h >= 1440) return "QHD";
    if (h >= 1080) return "Full HD";
    if (h >= 720) return "HD";
    return h > 0 ? "SD" : "";
  }

  // A codec as people name it.
  function codecName(vcodec) {
    const v = String(vcodec || "").toLowerCase();
    if (!v || v === "none") return "";
    if (v.startsWith("av01") || v === "av1") return "AV1";
    if (v.startsWith("vp09") || v.startsWith("vp9")) return "VP9";
    if (v.startsWith("vp8")) return "VP8";
    if (v.startsWith("avc") || v.startsWith("h264")) return "H.264";
    if (v.startsWith("hev") || v.startsWith("hvc") || v.startsWith("h265")) return "HEVC";
    return "";
  }

  // High dynamic range: said by the site (YouTube's label, yt-dlp's dynamic_range) or by the
  // codec (VP9 profile 2, 10-bit AV1 with a PQ/HLG transfer is named by the site).
  function isHdr(f) {
    if (!f) return false;
    if (f.hdr === true) return true;
    const dr = String(f.dynamicRange || f.dynamic_range || "");
    if (dr && !/^SDR$/i.test(dr)) return true;
    return /^vp09\.02\./i.test(String(f.vcodec || ""));
  }

  // What the menu lists, in one column, in the order a person reads it: the best choice, then
  // every quality the site really offers (its own type and size on the row), then a `sep` and the
  // sound on its own. No tabs: a tab per container showed the same qualities again and hid what
  // was actually there. The container each file is written in is yt-dlp's own choice; the XDM
  // window ("More options...") is where a different one is picked.
  function audioRows(
    duration,
    bestAudioInfo,
    apart,
    hlsTracks = [],
    defaultHlsAudio = "",
    bestM4aInfo = null,
    bestAudioExt = "",
  ) {
    const est = (kbps) => (duration > 0 ? Math.round(kbps * 125 * duration) : bestAudioInfo.size);
    const origRow = [];
    if (apart) {
      if (defaultHlsAudio) {
        origRow.push({
          label: uiText("Original Audio", "Orijinal Ses"),
          selector: "hls-audio",
          container: "ts",
          hlsAudio: defaultHlsAudio,
          size: bestAudioInfo.size,
          approx: bestAudioInfo.approx,
        });
      } else {
        const ext = String(bestAudioExt || "").toLowerCase();
        if (bestM4aInfo) {
          origRow.push({
            label: "M4A",
            selector: "ba[ext=m4a]",
            container: null,
            size: bestM4aInfo.size,
            approx: bestM4aInfo.approx,
          });
        }
        if (!bestM4aInfo || (ext && ext !== "m4a")) {
          const origLabel =
            ext === "webm" || ext === "opus"
              ? uiText("Original Audio (WEBM/Opus)", "Orijinal Ses (WEBM/Opus)")
              : ext
                ? `${uiText("Original Audio", "Orijinal Ses")} (${ext.toUpperCase()})`
                : uiText("Original Audio", "Orijinal Ses");
          origRow.push({
            label: origLabel,
            selector: "ba",
            container: null,
            size: bestAudioInfo.size,
            approx: bestAudioInfo.approx,
          });
        }
      }
      if (hlsTracks.length > 1) {
        for (const [idx, tr] of hlsTracks.entries()) {
          origRow.push({
            label: `${uiText("Original Audio", "Orijinal Ses")} · ${tr.name || tr.language || `#${idx + 1}`}`,
            selector: "hls-audio",
            container: "ts",
            hlsAudio: tr.uri,
            size: bestAudioInfo.size,
            approx: bestAudioInfo.approx,
          });
        }
      }
    }
    return [
      { sep: uiText("Audio only", "Yalnız ses") },
      ...origRow,
      { label: "MP3 (320 kbps)", selector: "mp3:320", container: null, size: est(320), approx: duration > 0 },
      { label: "MP3 (192 kbps)", selector: "mp3:192", container: null, size: est(192), approx: duration > 0 },
      { label: "MP3 (VBR)", selector: "mp3:0", container: null, size: est(245), approx: duration > 0 },
      { label: "OPUS", selector: "opus:", container: null, size: est(160), approx: duration > 0 },
      { label: "FLAC", selector: "flac:", container: null, size: est(900), approx: duration > 0 },
    ];
  }

  // Writing the file in another container, the same set the app's own video window offers.
  // mp4, mkv, mov (and ts/flv when an H.264/avc1 stream exists) are repackaged via -c copy;
  // webm, avi (and ts/flv only when no compatible H.264 stream is offered) fall back to ffmpeg re-encoding.
  const CONVERT = [
    { id: "mp4", label: () => "MP4" },
    { id: "mkv", label: () => "MKV" },
    { id: "mov", label: () => "MOV" },
    { id: "ts", label: () => "TS" },
    { id: "webm", label: () => "WebM", recode: true },
    { id: "avi", label: () => "AVI", recode: true },
    { id: "flv", label: () => "FLV" },
  ];

  function bestAvcFormat(videos) {
    const avc = videos.filter((f) => Number(f.height) > 0 && /^(avc|h264)/i.test(String(f.vcodec || "")));
    if (!avc.length) return null;
    return avc.reduce((a, b) => {
      const taller = Number(b.height) - Number(a.height);
      if (taller !== 0) return taller > 0 ? b : a;
      return (Number(b.tbr) || 0) > (Number(a.tbr) || 0) ? b : a;
    });
  }

  function realFormatId(f) {
    const id = String((f && (f.formatId || f.id)) || "");
    return id && !id.startsWith("hls-") && !id.startsWith("direct-") && !id.startsWith("dash-") && !id.startsWith("live-") ? id : "";
  }

  // H.264 video with AAC sound in an MP4 is what Windows Media Player, old televisions and most
  // editors play without a codec pack; a site's WebM/VP9 or Opus they often don't. This one is a
  // choice among the formats the site already has, so it costs no more than any other row.
  function h264Row(videos, totalOf, defaultHlsAudio = "") {
    const best = bestAvcFormat(videos);
    if (!best) return [];
    const h = Number(best.height);
    const avcId = realFormatId(best);
    const info = totalOf(best, "mp4");
    const fAudio = best.hlsAudio || defaultHlsAudio;
    return [
      {
        label: "MP4 · H.264 + AAC",
        hint: uiText("Plays in Windows Media Player and on older devices", "Windows Media Player ve eski cihazlarda açılır"),
        selector: `${avcId ? `${avcId}+ba[ext=m4a]/${avcId}+ba/` : ""}bv*[height<=${h}][vcodec^=avc]+ba[ext=m4a]/bv*[height<=${h}][ext=mp4]+ba[ext=m4a]/b[height<=${h}][ext=mp4]/bv*[height<=${h}]+ba/b`,
        container: "mp4",
        ...(fAudio ? { hlsAudio: fAudio } : {}),
        size: info.size,
        approx: info.approx,
      },
    ];
  }

  function convertRows(videos, duration, canMerge, bestSel, bestInfo, bestMp4Info, totalOf, defaultHlsAudio = "", top = null) {
    if (!canMerge) return [];
    const rows = [{ sep: uiText("Save as (best quality)", "Şu biçimde yaz (en iyi kalite)") }];
    const topId = realFormatId(top);
    const mp4BestSel = topId
      ? `${topId}+ba[ext=m4a]/${topId}+ba/bv*+ba[ext=m4a]/bv*[ext=mp4]+ba[ext=m4a]/bv*+ba/b`
      : "bv*+ba[ext=m4a]/bv*[ext=mp4]+ba[ext=m4a]/bv*+ba/b";
    const fullBestSel = topId ? `${topId}+ba/${bestSel}` : bestSel;
    const bestAvc = bestAvcFormat(videos);
    const avcFirstSel = "bv*[vcodec^=avc]+ba[ext=m4a]/bv*[vcodec^=avc]+ba/bv*[ext=mp4]+ba[ext=m4a]/bv*+ba[ext=m4a]/bv*+ba/b";
    const hasAvc = Boolean(bestAvc);
    const avcInfo = bestAvc ? totalOf(bestAvc, "mp4") : bestMp4Info;
    rows.push({
      label: "MP4",
      selector: mp4BestSel,
      hint: uiText("Best quality with AAC audio", "AAC sesli en iyi kalite"),
      container: "mp4",
      ...(defaultHlsAudio ? { hlsAudio: defaultHlsAudio } : {}),
      size: bestMp4Info.size,
      approx: bestMp4Info.approx,
    });
    rows.push(...h264Row(videos, totalOf, defaultHlsAudio));
    for (const c of CONVERT) {
      if (c.id === "mp4") continue;
      const useAvc = c.id === "mov" || c.id === "ts" || c.id === "flv";
      const recode = Boolean(c.recode) || ((c.id === "ts" || c.id === "flv") && !hasAvc);
      const rowInfo = useAvc && hasAvc && avcInfo.size ? avcInfo : c.id === "mov" ? bestMp4Info : bestInfo;
      rows.push({
        label: recode ? `${c.label()} · ${uiText("re-encoded", "yeniden kodlanır")}` : c.label(),
        selector: useAvc ? avcFirstSel : fullBestSel,
        hint: recode
          ? uiText("ffmpeg encodes it again: on a long film this takes a while", "ffmpeg baştan kodlar: uzun bir filmde bu uzun sürer")
          : uiText("Repackaged, the picture untouched", "Görüntüye dokunulmadan yeniden paketlenir"),
        container: c.id,
        ...(!recode && defaultHlsAudio ? { hlsAudio: defaultHlsAudio } : {}),
        size: rowInfo.size,
        approx: Boolean(rowInfo.size) && (recode || rowInfo.approx),
      });
    }
    return rows;
  }

  // The type shown on a row: what the site serves that format as.
  function typeLabel(f) {
    const ext = String((f && f.ext) || "").replace(/^\./, "").toUpperCase();
    if (ext) return ext === "M3U8" ? "HLS" : ext;
    const v = String((f && f.vcodec) || "");
    if (/^(vp9|vp8)/i.test(v)) return "WEBM";
    if (/^(avc|h264|av01|hev)/i.test(v)) return "MP4";
    return "";
  }

  // A YouTube page that plays a video of a playlist or a mix (`…&list=…`): the whole list, in one
  // quality, as the app's playlist window offers it (playlistdlg.rs picks: the same selectors).
  // The app lists it with that quality picked and every video ticked; Download adds them all.
  function youTubeList() {
    try {
      const url = new URL(location.href);
      const host = url.hostname.replace(/^(www|m|music)\./, "");
      if (host !== "youtube.com") return "";
      const list = url.searchParams.get("list") || "";
      // Watch later and Liked videos are the person's own; the rest are lists anyone can open.
      return /^[\w-]{2,64}$/.test(list) && list !== "WL" && list !== "LL" ? list : "";
    } catch {
      return "";
    }
  }

  function listRows() {
    const list = youTubeList();
    if (!list) return [];
    const mix = /^RD/.test(list);
    const head = mix ? uiText("The whole mix", "Mix'in tamamı") : uiText("The whole playlist", "Oynatma listesinin tamamı");
    const hint = uiText("Lists its videos in Xenon with this quality picked", "Videoları bu kalite seçili olarak Xenon'da listelenir");
    // Each video in the best it has up to the tier picked (no ceiling: a list of 4K videos is
    // saved in 4K); H.264 is the one that plays everywhere (YouTube makes it up to 1080p).
    const tier = (h) => ({
      label: resolutionLabel(h),
      badge: resolutionBadge(h),
      sub: uiText("Each video up to this, best audio", "Her video en fazla bu, en iyi ses"),
      selector: `bv*[height<=${h}]+ba/b[height<=${h}]/bv*+ba/b`,
      list: true,
      hint,
    });
    return [
      { sep: head },
      { label: uiText("Best quality", "En iyi kalite"), badge: "MAX", sub: uiText("Highest each video has", "Her videonun en yükseği"), selector: "bv*+ba/b", list: true, hint },
      tier(2160),
      tier(1440),
      tier(1080),
      tier(720),
      {
        label: "MP4 · H.264 + AAC",
        badge: "MP4",
        sub: uiText("Plays everywhere", "Her yerde açılır"),
        selector: "bv*[vcodec^=avc]+ba[ext=m4a]/bv*[ext=mp4]+ba[ext=m4a]/b[ext=mp4]/bv*+ba/b",
        container: "mp4",
        list: true,
        hint,
      },
      { label: "MP3 (VBR)", badge: "MP3", selector: "mp3:0", list: true, hint },
    ];
  }

  function computeRows(data) {
    const rawFormats = Array.isArray(data && data.formats) ? data.formats : [];
    const meta = typeof video !== "undefined" && typeof mediaMeta === "function" ? mediaMeta(video) : { duration: 0, height: 0 };
    const domDur = meta.duration || 0;
    const domHeight = meta.height || 0;
    const videoOnlyCount = rawFormats.filter((f) => f && f.vcodec !== "none").length;
    const formats =
      domHeight > 0 && videoOnlyCount === 1
        ? rawFormats.map((f) => (f && f.vcodec !== "none" && !(Number(f.height) > 0) ? { ...f, height: domHeight } : f))
        : rawFormats;
    const duration = Number(data && data.duration) || domDur || 0;
    const canMerge = data && data.canMerge === false ? false : true;
    const videos = formats.filter((f) => f && Number(f.height) > 0 && f.vcodec !== "none");
    const audios = formats.filter((f) => f && f.acodec && f.acodec !== "none" && (!f.vcodec || f.vcodec === "none"));
    const audioTracks = Array.isArray(data && data.audioTracks) ? data.audioTracks.filter((a) => a && a.uri) : [];
    const defaultAudioTrack = audioTracks.find((a) => a.default) || audioTracks.find((a) => a.autoselect) || audioTracks[0] || null;
    const biggest = (list) => list.reduce((a, b) => (formatBytesOf(b, duration).size > formatBytesOf(a, duration).size ? b : a), null);
    const m4aAudios = audios.filter((f) => String(f.ext || "").toLowerCase() === "m4a");
    const bestM4a = m4aAudios.length ? biggest(m4aAudios) : null;
    const bestAudio = audios.length ? biggest(audios) : null;
    const defaultHlsAudio = (defaultAudioTrack && defaultAudioTrack.uri) || (bestAudio && bestAudio.hlsAudio) || "";
    const audioInfo = formatBytesOf(bestAudio, duration);
    const m4aInfo = bestM4a ? formatBytesOf(bestM4a, duration) : null;
    const sound = formats.length
      ? audioRows(
          duration,
          audioInfo,
          audios.length > 0 || audioTracks.length > 0,
          audioTracks,
          defaultHlsAudio,
          m4aInfo,
          bestAudio ? bestAudio.ext : "",
        )
      : [];

    // A format's own id is what picks it exactly; the ids the extension makes up for an HLS
    // variant (`hls-1080`) are not yt-dlp's, so those fall back to a height.
    const realId = realFormatId;
    const rowContainer = (f) => {
      const t = typeLabel(f);
      if (t === "TS") return "ts";
      if (t === "MP4") return "mp4";
      if (t === "WEBM") return "webm";
      if (t === "MKV") return "mkv";
      if (t === "MOV") return "mov";
      return null;
    };
    const heightSel = (h, c) => {
      if (!canMerge) return h ? `b[height<=${h}]/b` : "b";
      if (!c || c === "mp4" || c === "mov") {
        return h ? `bv*[height<=${h}]+ba[ext=m4a]/bv*[height<=${h}]+ba/b[height<=${h}]/b` : "bv*+ba[ext=m4a]/bv*+ba/b";
      }
      if (c === "webm") {
        return h ? `bv*[height<=${h}]+ba[ext=webm]/bv*[height<=${h}]+ba/b[height<=${h}]/b` : "bv*+ba[ext=webm]/bv*+ba/b";
      }
      return h ? `bv*[height<=${h}]+ba/b[height<=${h}]/b` : "bv*+ba/b";
    };
    const selectorFor = (f, h) => {
      const id = realId(f);
      const c = rowContainer(f);
      if (!id) return heightSel(h, c);
      const muxed = f.acodec && f.acodec !== "none";
      if (muxed || !canMerge) return `${id}/${heightSel(h, c)}`;
      if (c === "mp4" || c === "mov") return `${id}+ba[ext=m4a]/${id}+ba/${heightSel(h, c)}`;
      if (c === "webm") return `${id}+ba[ext=webm]/${id}+ba/${heightSel(h, c)}`;
      return `${id}+ba/${heightSel(h, c)}`;
    };
    const totalOf = (v, c = null) => {
      if (!v) return { size: 0, approx: false };
      const vi = formatBytesOf(v, duration);
      if (!vi.size) return { size: 0, approx: false };
      const muxed = v.acodec && v.acodec !== "none";
      const ai = !c || c === "mp4" || c === "mov" ? (m4aInfo || audioInfo) : audioInfo;
      return { size: muxed ? vi.size : vi.size + ai.size, approx: vi.approx || (!muxed && ai.approx) };
    };

    if (!videos.length) {
      // A stream that names no height: its formats by bitrate, else plain heights to ask for.
      const byRate = formats.filter((f) => f && f.vcodec !== "none" && (f.tbr > 0 || f.size > 0));
      if (byRate.length) {
        const sorted = [...byRate].sort((a, b) => (Number(b.tbr) || Number(b.size) || 0) - (Number(a.tbr) || Number(a.size) || 0));
        const topCont = sorted[0] && sorted[0].protocol === "m3u8_native" ? rowContainer(sorted[0]) : null;
        const rows = [
          {
            label: uiText("Best quality", "En iyi kalite"),
            selector: heightSel(0, topCont),
            container: topCont,
            ...(defaultHlsAudio ? { hlsAudio: defaultHlsAudio } : {}),
            ...formatBytesOf(sorted[0], duration),
          },
        ];
        const seen = new Set();
        for (const f of sorted) {
          const type = typeLabel(f) || "MP4";
          const lbl = f.tbr > 0 ? `${type} · ${Math.round(f.tbr)} kbps` : type;
          if (seen.has(lbl)) continue;
          seen.add(lbl);
          const info = formatBytesOf(f, duration);
          const fAudio = f.hlsAudio || defaultHlsAudio;
          rows.push({
            label: lbl,
            selector: selectorFor(f, 0),
            container: rowContainer(f),
            ...(fAudio ? { hlsAudio: fAudio } : {}),
            size: info.size,
            approx: info.approx,
          });
        }
        const bi = formatBytesOf(sorted[0], duration);
        return rows.concat(convertRows(byRate, duration, canMerge, "bv*+ba/b", bi, bi, totalOf, defaultHlsAudio), sound);
      }
      const fallbackVideos = formats.filter((f) => f && f.vcodec !== "none");
      const topFallback = fallbackVideos[0] || formats[0];
      const fallbackHls = topFallback && topFallback.protocol === "m3u8_native" ? rowContainer(topFallback) : null;
      const rows = [
        {
          label: uiText("Best quality", "En iyi kalite"),
          selector: heightSel(0, fallbackHls),
          container: fallbackHls,
          ...(defaultHlsAudio ? { hlsAudio: defaultHlsAudio } : {}),
          size: 0,
          approx: false,
        },
      ];
      if (fallbackVideos.length) {
        const seen = new Set();
        for (const f of fallbackVideos) {
          const type = typeLabel(f);
          if (!type || seen.has(type)) continue;
          seen.add(type);
          const fAudio = f.hlsAudio || defaultHlsAudio;
          rows.push({
            label: type,
            selector: selectorFor(f, 0),
            container: rowContainer(f),
            ...(fAudio ? { hlsAudio: fAudio } : {}),
            size: 0,
            approx: false,
          });
        }
        const zero = { size: 0, approx: false };
        return rows.concat(convertRows(fallbackVideos, duration, canMerge, "bv*+ba/b", zero, zero, totalOf, defaultHlsAudio), sound);
      }
      return rows.concat(sound);
    }

    // One row per quality-and-type the site offers: the same height in two types is two rows, the
    // same height and type twice is one (the richer of them).
    // A high frame rate (50/60 fps) is a quality of its own: 1080p60 and 1080p30 are two rows.
    const fpsClass = (f) => (Number(f.fps) >= 48 ? Math.round(Number(f.fps)) : 0);
    const best = new Map();
    for (const f of videos) {
      const key = `${Number(f.height)}|${fpsClass(f)}|${typeLabel(f)}|${isHdr(f) ? "hdr" : ""}`;
      const old = best.get(key);
      if (!old || (Number(f.tbr) || 0) > (Number(old.tbr) || 0) || totalOf(f).size > totalOf(old).size) best.set(key, f);
    }
    // Highest first: the picture's lines, then its frame rate, then its rate (8K ... 144p).
    const picked = [...best.values()].sort(
      (a, b) =>
        Number(b.height) - Number(a.height) ||
        (Number(b.fps) || 0) - (Number(a.fps) || 0) ||
        (Number(b.tbr) || 0) - (Number(a.tbr) || 0),
    );
    const bestCont = picked[0] && picked[0].protocol === "m3u8_native" ? rowContainer(picked[0]) : null;
    const bestInfo = totalOf(picked[0], "mkv");
    const bestMp4Info = totalOf(picked[0], "mp4");
    const top = picked[0];
    const topH = Number(top.height) || 0;
    const topCont = bestCont || rowContainer(top);
    const topInfo = totalOf(top, topCont);
    const topSelector = realId(top)
      ? `${selectorFor(top, 0)}/${heightSel(0, bestCont)}`
      : heightSel(0, bestCont);
    const rows = [
      {
        label: uiText("Best quality", "En iyi kalite"),
        badge: resolutionBadge(topH) || "HD",
        sub: [resolutionLabel(topH), typeLabel(top), codecName(top.vcodec), fpsClass(top) ? `${fpsClass(top)}fps` : "", isHdr(top) ? "HDR" : ""]
          .filter(Boolean)
          .join(" · "),
        best: true,
        selector: topSelector,
        container: bestCont,
        ...(defaultHlsAudio ? { hlsAudio: defaultHlsAudio } : {}),
        ...topInfo,
      },
    ];
    for (const f of picked) {
      const h = Number(f.height);
      const type = typeLabel(f);
      const tbr = Number(f.tbr) || 0;
      const fps = fpsClass(f);
      const codec = codecName(f.vcodec);
      const hdr = isHdr(f);
      const fAudio = f.hlsAudio || defaultHlsAudio;
      const rc = rowContainer(f);
      const info = totalOf(f, rc);
      // The label reads in one line (and the tests read it); the menu draws the same as a
      // resolution badge, then type, codec, frame rate and HDR.
      const shape = (extra) => {
        const parts = [resolutionLabel(h), fps ? `${fps}fps` : "", type, codec, hdr ? "HDR" : "", extra, tbr > 0 ? `${Math.round(tbr)} kbps` : ""];
        return {
          label: parts.filter(Boolean).join(" · "),
          badge: resolutionBadge(h),
          sub: [resolutionClass(h), type, codec, fps ? `${fps}fps` : "", extra].filter(Boolean).join(" · "),
          ...(hdr ? { hdr: true } : {}),
        };
      };
      if (audioTracks.length > 1 && f.protocol === "m3u8_native") {
        for (const [idx, tr] of audioTracks.entries()) {
          const langLabel = tr.name || tr.language || `#${idx + 1}`;
          rows.push({
            ...shape(langLabel),
            selector: selectorFor(f, h),
            container: rc,
            hlsAudio: tr.uri,
            size: info.size,
            approx: info.approx,
          });
        }
      } else {
        rows.push({
          ...shape(""),
          selector: selectorFor(f, h),
          container: rc,
          ...(fAudio ? { hlsAudio: fAudio } : {}),
          size: info.size,
          approx: info.approx,
        });
      }
    }
    return rows.concat(convertRows(videos, duration, canMerge, "bv*+ba/b", bestInfo, bestMp4Info, totalOf, defaultHlsAudio, top), sound);
  }

  // ---- the button: a closed shadow tree the page's styles and scripts can't reach ----------
  const root = document.createElement("mdm-video-button");
  // Enable subtle frosted glass (~92-95% opacity + blur) ONLY when CSS backdrop-filter is
  // supported, reduced-transparency/forced-colors are off, and GPU hardware acceleration is active;
  // otherwise render on 100% solid matte obsidian so underlying video/text never bleeds through.
  let gpuChecked = false;
  function detectHardwareGlassSupport() {
    try {
      if (typeof CSS === "undefined" || typeof CSS.supports !== "function") return false;
      if (!CSS.supports("(backdrop-filter: blur(18px)) or (-webkit-backdrop-filter: blur(18px))")) return false;
      if (window.matchMedia?.("(prefers-reduced-transparency: reduce)")?.matches) return false;
      if (window.matchMedia?.("(forced-colors: active)")?.matches) return false;
      if (window === window.top) {
        const c = document.createElement("canvas");
        c.width = 1;
        c.height = 1;
        const gl = c.getContext("webgl", { failIfMajorPerformanceCaveat: true })
          || c.getContext("experimental-webgl", { failIfMajorPerformanceCaveat: true });
        if (!gl) return false;
        const dbg = gl.getExtension("WEBGL_debug_renderer_info");
        const renderer = String(
          (dbg && gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL)) || gl.getParameter(gl.RENDERER) || "",
        ).toLowerCase();
        gl.getExtension("WEBGL_lose_context")?.loseContext();
        if (/swiftshader|basic render|llvmpipe|softpipe|software|warp/i.test(renderer)) return false;
      }
      return true;
    } catch {
      return false;
    }
  }
  function ensureGlassAttr() {
    if (gpuChecked) return;
    gpuChecked = true;
    if (detectHardwareGlassSupport()) root.setAttribute("data-glass", "1");
    else root.removeAttribute("data-glass");
  }
  // Each with !important: a page's own rules can't hide or move it.
  for (const [k, v] of Object.entries({
    all: "initial", position: "fixed", "z-index": "2147483647", left: "0", top: "0", display: "none",
    contain: "layout style", opacity: "1", visibility: "visible", "pointer-events": "auto",
  })) root.style.setProperty(k, v, "important");
  const shadow = root.attachShadow({ mode: "closed" });
  const CSS_TEXT = `
:host { all: initial; }
* { box-sizing: border-box; }
/* Xenon obsidian: 100% solid matte dark base on software/non-blur GPUs, upgrading to 92-95%
   high-opacity frosted glass with backdrop-filter only on verified hardware-accelerated GPUs. */
.pill-wrap {
  position: relative; display: inline-flex; align-items: center;
  --cyan: #38bdf8; --indigo: #6366f1; --glass: #121622; --edge: rgba(255, 255, 255, .14);
}
.pill {
  display: flex; align-items: stretch; height: 32px; border-radius: 16px;
  background: linear-gradient(135deg, #171c28, #0d1018);
  color: #f1f5f9; border: 1px solid rgba(255, 255, 255, .16);
  box-shadow: 0 8px 24px rgba(0, 0, 0, .46), inset 0 1px 0 rgba(255, 255, 255, .16), inset 0 -1px 0 rgba(0, 0, 0, .28);
  font: 600 12px/1 "Segoe UI Variable Text", "Segoe UI", system-ui, -apple-system, Roboto, sans-serif;
  letter-spacing: -0.01em;
  opacity: 0; transform: translateY(-3px) scale(.96);
  transition: opacity .18s cubic-bezier(.2, .8, .2, 1), transform .28s cubic-bezier(.34, 1.56, .64, 1), background .2s, box-shadow .2s;
  user-select: none; -webkit-user-select: none; touch-action: none; white-space: nowrap;
}
:host([data-glass="1"]) .pill {
  background: linear-gradient(135deg, rgba(23, 28, 40, .93), rgba(11, 13, 19, .91));
  backdrop-filter: blur(18px) saturate(165%); -webkit-backdrop-filter: blur(18px) saturate(165%);
}
.pill.shown { opacity: 1; transform: none; }
.pill:hover { box-shadow: 0 10px 30px rgba(0, 0, 0, .48), 0 0 0 1px rgba(56, 189, 248, .28), 0 0 22px rgba(56, 189, 248, .18), inset 0 1px 0 rgba(255, 255, 255, .18); }
.pill.dragging { cursor: grabbing; box-shadow: 0 14px 34px rgba(0, 0, 0, .58); transition: none; }
.pill.dragging .go { cursor: grabbing; }
.pill.ok { background: linear-gradient(135deg, #064e3b, #062e24); border-color: rgba(52, 211, 153, .55); color: #d1fae5; }
.pill.err { background: linear-gradient(135deg, #7f1d1d, #450a0a); border-color: rgba(251, 113, 133, .55); color: #ffe4e6; }
:host([data-glass="1"]) .pill.ok { background: linear-gradient(135deg, rgba(6, 78, 59, .94), rgba(6, 46, 36, .92)); }
:host([data-glass="1"]) .pill.err { background: linear-gradient(135deg, rgba(127, 29, 29, .94), rgba(69, 10, 10, .92)); }
button {
  all: unset; box-sizing: border-box; display: flex; align-items: center; gap: 6px;
  color: inherit; cursor: pointer; font: inherit;
}
.go { padding: 0 12px 0 5px; border-radius: 16px; cursor: grab; max-width: 260px; gap: 8px; }
.go .dl-ico {
  width: 22px; height: 22px; flex: none; padding: 4px; border-radius: 50%; color: #fff;
  background: linear-gradient(135deg, var(--cyan), var(--indigo));
  box-shadow: 0 0 12px rgba(56, 189, 248, .55), inset 0 1px 0 rgba(255, 255, 255, .35);
  transition: transform .25s cubic-bezier(.34, 1.56, .64, 1), box-shadow .2s;
}
.go:hover .dl-ico { transform: translateY(1px) scale(1.06); box-shadow: 0 0 18px rgba(56, 189, 248, .8), inset 0 1px 0 rgba(255, 255, 255, .4); }
.go .label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 200px; }
.go .caret { width: 10px; height: 10px; flex: none; opacity: .7; transition: transform .2s cubic-bezier(.2, .8, .2, 1); }
.pill.menu-on .go .caret { transform: rotate(180deg); }
.dismiss {
  position: absolute; top: -6px; right: -6px; width: 17px; height: 17px; border-radius: 50%;
  background: #141824; color: #cbd5e1;
  border: 1px solid rgba(255, 255, 255, .22);
  box-shadow: 0 2px 8px rgba(0, 0, 0, .5);
  justify-content: center; opacity: 0; transform: scale(.8);
  transition: opacity .14s, transform .2s cubic-bezier(.34, 1.56, .64, 1), background .12s, color .12s;
}
.pill-wrap:hover .dismiss, .dismiss:focus-visible, .pill.menu-on + .dismiss { opacity: 1; transform: scale(1); }
.dismiss:hover { background: #f43f5e; color: #fff; border-color: #fb7185; }
.dismiss svg { width: 9px; height: 9px; }
.pill[dir="rtl"] .dismiss { right: auto; left: -6px; }
.go:hover { background: rgba(255, 255, 255, .07); }
.go:active { background: rgba(255, 255, 255, .13); }
button:focus-visible { outline: 2px solid var(--cyan, #38bdf8); outline-offset: -2px; }
.compact .label, .compact .caret { display: none; }
.compact .go { padding: 0 5px; }
.menu {
  --cyan: #38bdf8; --indigo: #6366f1;
  position: absolute; width: 344px; border-radius: 16px; overflow: hidden;
  background: linear-gradient(160deg, #171c28, #0d1018);
  color: #e2e8f0; border: 1px solid rgba(255, 255, 255, .14);
  box-shadow: 0 24px 60px rgba(0, 0, 0, .6), 0 0 0 1px rgba(0, 0, 0, .35), inset 0 1px 0 rgba(255, 255, 255, .12);
  font: 400 12px/1.3 "Segoe UI Variable Text", "Segoe UI", system-ui, -apple-system, Roboto, sans-serif;
  display: none; transform-origin: top center;
}
:host([data-glass="1"]) .menu {
  background: linear-gradient(160deg, rgba(23, 28, 40, .95), rgba(11, 13, 19, .93));
  backdrop-filter: blur(22px) saturate(165%); -webkit-backdrop-filter: blur(22px) saturate(165%);
}
.menu.open { display: flex; flex-direction: column; animation: menu-in .22s cubic-bezier(.2, .8, .2, 1); }
@keyframes menu-in { from { opacity: 0; transform: translateY(-6px) scale(.97); } to { opacity: 1; transform: none; } }
/* The video: its picture, length, title, site and resolution. */
.meta-bar {
  display: grid; grid-template-columns: 128px minmax(0, 1fr); gap: 11px; flex: none; padding: 11px;
  align-items: center;
  background: linear-gradient(180deg, rgba(56, 189, 248, .08), rgba(99, 102, 241, .03));
  border-bottom: 1px solid rgba(255, 255, 255, .07);
}
.thumb {
  position: relative; width: 128px; height: 72px; border-radius: 10px; overflow: hidden;
  justify-self: start;
  background: linear-gradient(135deg, #1e293b, #0f172a);
  box-shadow: 0 4px 14px rgba(0, 0, 0, .45), inset 0 0 0 1px rgba(255, 255, 255, .08);
}
.thumb canvas, .thumb img { width: 100%; height: 100%; object-fit: cover; display: block; }
.thumb .ph { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; color: rgba(148, 163, 184, .6); }
.thumb .ph svg { width: 26px; height: 26px; }
.thumb .dur {
  position: absolute; right: 5px; bottom: 5px; padding: 2px 5px; border-radius: 5px;
  background: rgba(0, 0, 0, .72); color: #fff; font: 600 10.5px/1.1 inherit; font-variant-numeric: tabular-nums;
}
.thumb .dur.live { background: #e11d48; letter-spacing: .04em; }
.thumb .dur.live::before { content: ""; display: inline-block; width: 6px; height: 6px; margin-inline-end: 4px; border-radius: 50%; background: #fff; vertical-align: 1px; }
.v-info { min-width: 0; display: flex; flex-direction: column; align-items: flex-end; text-align: end; gap: 6px; justify-content: center; }
.v-title {
  width: 100%; font-size: 12.5px; font-weight: 600; line-height: 1.32; color: #f8fafc; text-align: end;
  white-space: normal; overflow-wrap: anywhere; word-break: break-word;
}
.v-tags { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 4px; width: 100%; }
.v-tag {
  padding: 2px 7px; border-radius: 10px; font-size: 10.5px; font-weight: 600; line-height: 1.4;
  background: rgba(255, 255, 255, .07); color: #cbd5e1; border: 1px solid rgba(255, 255, 255, .08);
  max-width: 100%; white-space: normal; overflow-wrap: anywhere; word-break: break-word; text-align: end;
}
.v-tag.res { background: linear-gradient(135deg, rgba(56, 189, 248, .22), rgba(99, 102, 241, .22)); color: #e0f2fe; border-color: rgba(56, 189, 248, .35); }
.q-sep {
  position: sticky; top: -4px; z-index: 1; display: flex; align-items: center; gap: 7px;
  margin: 0 -4px; padding: 9px 13px 5px; font-size: 10px; font-weight: 700;
  letter-spacing: .07em; text-transform: uppercase; color: rgba(148, 163, 184, .92);
  background: #121621;
}
:host([data-glass="1"]) .q-sep {
  background: rgba(16, 20, 30, .95); backdrop-filter: blur(12px); -webkit-backdrop-filter: blur(12px);
}
.q-sep::after { content: ""; flex: 1; height: 1px; background: linear-gradient(90deg, rgba(255, 255, 255, .12), transparent); }
.q-list {
  flex: 1 1 auto; min-height: 0; max-height: 330px; overflow-y: auto; padding: 4px;
  scrollbar-width: thin; scrollbar-color: rgba(255, 255, 255, .22) transparent;
}
.q-list::-webkit-scrollbar { width: 5px; }
.q-list::-webkit-scrollbar-thumb { background: rgba(255, 255, 255, .2); border-radius: 4px; }
.q-row {
  width: 100%; display: grid; grid-template-columns: 44px 1fr auto; align-items: center; gap: 10px;
  padding: 6px 9px; border-radius: 10px; font-size: 12px; text-align: start;
  border: 1px solid transparent; transition: background .12s, border-color .12s;
}
.q-row:hover, .q-row:focus-visible {
  background: linear-gradient(90deg, rgba(56, 189, 248, .16), rgba(99, 102, 241, .10));
  border-color: rgba(56, 189, 248, .28); color: #fff; outline: none;
}
.q-row.best { background: linear-gradient(90deg, rgba(56, 189, 248, .10), rgba(99, 102, 241, .06)); border-color: rgba(99, 102, 241, .28); }
.q-badge {
  display: flex; align-items: center; justify-content: center; height: 24px; border-radius: 7px;
  font-size: 11px; font-weight: 800; letter-spacing: .02em; color: #e2e8f0;
  background: rgba(148, 163, 184, .14); border: 1px solid rgba(148, 163, 184, .2);
}
.q-badge[data-tier="uhd"] { color: #fff; background: linear-gradient(135deg, #f59e0b, #e11d48); border-color: rgba(251, 191, 36, .5); box-shadow: 0 0 12px rgba(244, 63, 94, .35); }
.q-badge[data-tier="qhd"] { color: #fff; background: linear-gradient(135deg, #8b5cf6, #6366f1); border-color: rgba(167, 139, 250, .5); }
.q-badge[data-tier="fhd"] { color: #fff; background: linear-gradient(135deg, #0ea5e9, #6366f1); border-color: rgba(56, 189, 248, .5); }
.q-badge[data-tier="hd"] { color: #e0f2fe; background: rgba(14, 165, 233, .22); border-color: rgba(56, 189, 248, .35); }
.q-badge[data-tier="audio"] { color: #d1fae5; background: rgba(16, 185, 129, .2); border-color: rgba(52, 211, 153, .35); }
.q-badge[data-tier="fmt"] { color: #ede9fe; background: rgba(99, 102, 241, .2); border-color: rgba(129, 140, 248, .35); }
.q-text { min-width: 0; display: flex; flex-direction: column; gap: 2px; }
.q-name { font-weight: 600; white-space: normal; overflow-wrap: anywhere; word-break: break-word; line-height: 1.25; color: #f1f5f9; }
.q-sub { font-size: 10.5px; color: rgba(148, 163, 184, .95); white-space: normal; overflow-wrap: anywhere; word-break: break-word; line-height: 1.3; }
.q-hdr { display: inline-block; margin-inline-start: 5px; padding: 0 4px; border-radius: 4px; font-size: 9px; font-weight: 800; color: #1c1917; background: linear-gradient(135deg, #fde68a, #f59e0b); vertical-align: 1px; }
.q-size {
  font-size: 10.5px; font-weight: 600; color: #bae6fd; font-variant-numeric: tabular-nums; flex: none;
  padding: 2px 7px; border-radius: 999px; background: rgba(56, 189, 248, .1); border: 1px solid rgba(56, 189, 248, .18);
}
.q-size.none { color: rgba(148, 163, 184, .7); background: transparent; border-color: transparent; }
.probe-note {
  flex: none; padding: 6px 13px 7px; font-size: 10.5px; color: rgba(148, 163, 184, .9);
  display: flex; align-items: center; gap: 7px; border-top: 1px solid rgba(255, 255, 255, .05);
}
.probe-note::before {
  content: ""; width: 10px; height: 10px; flex: none; border-radius: 50%;
  border: 2px solid rgba(56, 189, 248, .25); border-top-color: var(--cyan); animation: spin .8s linear infinite;
}
.probe-note.still::before { display: none; }
@keyframes spin { to { transform: rotate(360deg); } }
.m-foot {
  flex: none; border-top: 1px solid rgba(255, 255, 255, .08); padding: 6px;
  display: flex; flex-direction: row; gap: 4px; background: rgba(0, 0, 0, .18);
}
.m-foot button {
  padding: 7px 9px; border-radius: 9px; font-size: 11.5px; color: rgba(226, 232, 240, .9);
  cursor: pointer; text-align: start; transition: background .12s, color .12s;
}
.m-foot button[data-act="download"] { flex: 1; min-width: 0; font-weight: 600; }
.m-foot button.icon-only { flex: none; width: 32px; justify-content: center; padding: 7px 0; }
.m-foot button.icon-only .mi-label { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
.m-foot button:hover, .m-foot button:focus-visible { background: rgba(255, 255, 255, .09); color: #fff; outline: none; }
.m-foot button[data-act="never"]:hover { color: #fda4af; }
.m-foot img, .m-foot svg { width: 14px; height: 14px; flex: none; opacity: .9; border-radius: 3px; }
.m-foot .mi-label { flex: 1; min-width: 0; white-space: normal; overflow-wrap: anywhere; word-break: break-word; line-height: 1.25; }
@media (prefers-reduced-motion: reduce) {
  .pill, .dismiss, .go .dl-ico, .menu.open { transition: none; transform: none; animation: none; }
  .probe-note::before { animation: none; }
}
`;
  // A constructed sheet: a page whose Content-Security-Policy refuses inline styles can't strip
  // it (Firefox applies the page's policy to what a content script adds).
  try {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(CSS_TEXT);
    shadow.adoptedStyleSheets = [sheet];
  } catch {
    const style = document.createElement("style");
    style.textContent = CSS_TEXT;
    shadow.appendChild(style);
  }
  // Build the shadow tree strictly via DOM APIs (never innerHTML) so pages enforcing
  // `require-trusted-types-for 'script'` (e.g. Google Drive, embedded Google players) never throw.
  const SVG_NS = "http://www.w3.org/2000/svg";
  const el = (tag, attrs = {}, ...children) => {
    const node = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs)) {
      if (v !== undefined && v !== null) node.setAttribute(k, String(v));
    }
    for (const ch of children) if (ch) node.appendChild(ch);
    return node;
  };
  const svgEl = (attrs, shapes) => {
    const s = document.createElementNS(SVG_NS, "svg");
    for (const [k, v] of Object.entries(attrs)) s.setAttribute(k, String(v));
    for (const [tag, sa] of shapes) {
      const sh = document.createElementNS(SVG_NS, tag);
      for (const [k, v] of Object.entries(sa)) sh.setAttribute(k, String(v));
      s.appendChild(sh);
    }
    return s;
  };
  const label = el("span", { class: "label" });
  const go = el(
    "button",
    { class: "go", type: "button", "aria-haspopup": "menu", "aria-expanded": "false" },
    svgEl(
      {
        class: "dl-ico",
        viewBox: "0 0 16 16",
        fill: "none",
        stroke: "currentColor",
        "stroke-width": "1.9",
        "stroke-linecap": "round",
        "stroke-linejoin": "round",
        "aria-hidden": "true",
      },
      [["path", { d: "M8 2.2v8.1m0 0L4.7 7m3.3 3.3L11.3 7M3 13.5h10" }]],
    ),
    label,
    svgEl(
      {
        class: "caret",
        viewBox: "0 0 12 12",
        fill: "none",
        stroke: "currentColor",
        "stroke-width": "1.6",
        "stroke-linecap": "round",
        "stroke-linejoin": "round",
        "aria-hidden": "true",
      },
      [["path", { d: "M3 4.5 6 7.5l3-3" }]],
    ),
  );
  const pill = el("div", { class: "pill", part: "pill" }, go);
  const dismiss = el(
    "button",
    { class: "dismiss", type: "button", "data-act": "hide" },
    svgEl(
      {
        viewBox: "0 0 12 12",
        fill: "none",
        stroke: "currentColor",
        "stroke-width": "1.8",
        "stroke-linecap": "round",
        "aria-hidden": "true",
      },
      [["path", { d: "M3 3l6 6M9 3L3 9" }]],
    ),
  );
  const pillWrap = el("div", { class: "pill-wrap" }, pill, dismiss);
  // The video's preview: a frame of it (else its poster or the site's picture), its length, its
  // title, the site and the resolution playing.
  const thumbEl = el("div", { class: "thumb" });
  const vTitleEl = el("div", { class: "v-title" });
  const vChipsEl = el("div", { class: "v-tags" });
  const metaBarEl = el("div", { class: "meta-bar" }, thumbEl, el("div", { class: "v-info" }, vTitleEl, vChipsEl));
  const qListEl = el("div", { class: "q-list" });
  const probeNoteEl = el("div", { class: "probe-note", style: "display:none" });
  const dlImg = el("img", { alt: "", draggable: "false" });
  dlImg.src = LOGO;
  const foot = el(
    "div",
    { class: "m-foot" },
    el("button", { role: "menuitem", "data-act": "download" }, dlImg, el("span", { class: "mi-label" })),
    el(
      "button",
      { role: "menuitem", "data-act": "never", class: "icon-only" },
      svgEl(
        {
          viewBox: "0 0 16 16",
          fill: "none",
          stroke: "currentColor",
          "stroke-width": "1.4",
          "stroke-linecap": "round",
          "aria-hidden": "true",
        },
        [
          ["circle", { cx: "8", cy: "8", r: "5.5" }],
          ["path", { d: "M4.2 4.2l7.6 7.6" }],
        ],
      ),
      el("span", { class: "mi-label" }),
    ),
    el(
      "button",
      { role: "menuitem", "data-act": "settings", class: "icon-only" },
      svgEl(
        {
          viewBox: "0 0 16 16",
          fill: "none",
          stroke: "currentColor",
          "stroke-width": "1.4",
          "stroke-linecap": "round",
          "aria-hidden": "true",
        },
        [["path", { d: "M2.5 4.5h11M5.5 2.5v4M2.5 8h11M10.5 6v4M2.5 11.5h11M6.5 9.5v4" }]],
      ),
      el("span", { class: "mi-label" }),
    ),
  );
  const menu = el("div", { class: "menu", role: "menu" }, metaBarEl, qListEl, probeNoteEl, foot);
  shadow.append(pillWrap, menu);
  const item = (act) => menu.querySelector(`[data-act="${act}"] .mi-label`);

  // A row's badge: its resolution tier, else what it is (a format, a sound, the whole list).
  function rowBadge(r, section) {
    const t = String(r.badge || "");
    const m = /^(\d{3,4})p$/.exec(t);
    if (t === "8K" || t === "5K" || t === "4K") return { text: t, tier: "uhd" };
    if (t === "2K") return { text: t, tier: "qhd" };
    if (m) return { text: t, tier: Number(m[1]) >= 1080 ? "fhd" : Number(m[1]) >= 720 ? "hd" : "sd" };
    if (t) return { text: t, tier: /^(MP3|OPUS|FLAC|M4A|AAC)$/.test(t) ? "audio" : "fmt" };
    const label = String(r.label || "");
    if (/^(MP3|OPUS|FLAC)/.test(label) || r.selector === "ba" || r.selector === "hls-audio") {
      return { text: (/^(MP3|OPUS|FLAC)/.exec(label) || ["AUD"])[0], tier: "audio" };
    }
    const fmt = /^(MP4|MKV|MOV|TS|WebM|AVI|FLV)\b/i.exec(label);
    if (fmt) return { text: fmt[1].toUpperCase(), tier: "fmt" };
    const kb = /^([A-Z0-9]{2,4}) ·/.exec(label);
    if (kb) return { text: kb[1], tier: "fmt" };
    return { text: section && /audio|ses/i.test(section) ? "AUD" : "HD", tier: "fmt" };
  }

  // A quality row's name: its resolution in words ("4K (2160p)"), the rest in its second line.
  function resolutionLabelOf(r) {
    return String(r.label || "").split(" · ")[0];
  }

  // The preview: drawn when the menu opens (a frame from the video when it can be read), the
  // title, the site, the resolution playing; length or LIVE over the picture.
  let previewFor = null;
  function renderPreview(dur, isLive) {
    const meta = mediaMeta(video);
    const data = probeState.data || {};
    vTitleEl.textContent = activeTitle || cleanVideoTitle(data.title || "") || currentPageTitle() || host;
    vTitleEl.title = vTitleEl.textContent;
    vChipsEl.textContent = "";
    const chip = (text, cls) => {
      if (!text) return;
      const c = document.createElement("span");
      c.className = cls ? `v-tag ${cls}` : "v-tag";
      c.textContent = text;
      c.title = text;
      vChipsEl.appendChild(c);
    };
    const playing = Number(meta.height) || 0;
    const top = Array.isArray(data.formats)
      ? data.formats.reduce((m, f) => Math.max(m, f && f.vcodec !== "none" ? Number(f.height) || 0 : 0), 0)
      : 0;
    const h = top || playing;
    if (h) chip([resolutionBadge(h), resolutionClass(h)].filter(Boolean).join(" "), "res");
    chip(String(data.channel || ""), "");
    chip(host.replace(/^(www|m)\./, ""), "");
    // The picture: official cover/thumbnail/poster first; if none or loading fails, a live frame from the video.
    const coverSrc = previewImage(meta, data);
    const previewKey = video ? `${video.currentSrc || ""}|${location.href}|${coverSrc}` : coverSrc;
    if (previewFor !== previewKey) {
      previewFor = previewKey;
      thumbEl.textContent = "";
      const placeholder = () => {
        const ph = el(
          "div",
          { class: "ph" },
          svgEl({ viewBox: "0 0 24 24", fill: "currentColor", "aria-hidden": "true" }, [["path", { d: "M8 5.5v13a.8.8 0 0 0 1.2.7l10.4-6.5a.8.8 0 0 0 0-1.4L9.2 4.8A.8.8 0 0 0 8 5.5z" }]]),
        );
        thumbEl.prepend(ph);
      };
      const liveFrameOrPlaceholder = () => {
        if (!frameInto(thumbEl, meta)) placeholder();
      };
      if (coverSrc) {
        const img = document.createElement("img");
        img.alt = "";
        img.referrerPolicy = "no-referrer";
        img.onload = () => {
          if (img.naturalWidth > 0 && (img.naturalWidth < 48 || img.naturalHeight < 36)) {
            img.remove();
            liveFrameOrPlaceholder();
          }
        };
        img.onerror = () => {
          img.remove();
          liveFrameOrPlaceholder();
        };
        img.src = coverSrc;
        thumbEl.prepend(img);
      } else {
        liveFrameOrPlaceholder();
      }
    }
    let badge = thumbEl.querySelector(".dur");
    if (!badge) {
      badge = document.createElement("span");
      thumbEl.appendChild(badge);
    }
    badge.className = isLive ? "dur live" : "dur";
    badge.textContent = isLive ? uiText("LIVE", "CANLI") : dur > 0 ? formatDuration(dur) : "";
    badge.style.display = badge.textContent ? "" : "none";
  }

  // A frame of the playing video on a canvas (shown as it is: a picture from another site can be
  // drawn though not read back). Not for a protected video (it draws black) or one in a frame.
  function frameInto(box, meta) {
    try {
      if (!video || meta.isFrame || meta.drm || !(video instanceof HTMLVideoElement)) return false;
      if (video.readyState < 2 || !(video.videoWidth > 0)) return false;
      const c = document.createElement("canvas");
      c.width = 256;
      c.height = 144;
      const g = c.getContext("2d");
      if (!g) return false;
      const vr = video.videoWidth / video.videoHeight;
      let sw = video.videoWidth;
      let sh = video.videoHeight;
      if (vr > 16 / 9) sw = sh * (16 / 9);
      else sh = sw / (16 / 9);
      g.drawImage(video, (video.videoWidth - sw) / 2, (video.videoHeight - sh) / 2, sw, sh, 0, 0, 256, 144);
      box.prepend(c);
      return true;
    } catch {
      return false;
    }
  }

  // Official cover image / poster first: the player's poster, probe thumbnail, YouTube's cover, or the page's og/twitter image.
  function previewImage(meta, data) {
    const resolveHttp = (u) => {
      if (typeof u !== "string" || !u.trim()) return "";
      try {
        const abs = new URL(u.trim(), location.href).href;
        return /^https?:\/\//i.test(abs) ? abs : "";
      } catch {
        return "";
      }
    };
    if (video && !meta.isFrame) {
      const posterAttr = typeof video.getAttribute === "function" ? video.getAttribute("poster") || "" : "";
      const poster = resolveHttp(posterAttr || video.poster || "");
      if (poster) return poster;
    }
    const probeThumb = resolveHttp(data.thumbnail || "");
    if (probeThumb) return probeThumb;
    let ytId = String(data.id || "");
    if (!ytId && isYouTubePage()) {
      ytId =
        new URLSearchParams(location.search).get("v") ||
        ((/\/(?:shorts|live|embed)\/([\w-]{6,32})/.exec(location.pathname) || [])[1]) ||
        "";
    }
    if (isYouTubePage() && /^[\w-]{6,32}$/.test(ytId)) return `https://i.ytimg.com/vi/${ytId}/mqdefault.jpg`;
    const metaSelectors = [
      'meta[property="og:image"]',
      'meta[name="og:image"]',
      'meta[property="twitter:image"]',
      'meta[name="twitter:image"]',
      'link[rel="image_src"]',
    ];
    for (const sel of metaSelectors) {
      const node = document.querySelector(sel);
      const raw = node?.getAttribute?.("content") || node?.getAttribute?.("href") || node?.content || node?.href || "";
      const url = resolveHttp(raw);
      if (url) return url;
    }
    return "";
  }

  function renderRows(force = false) {
    if (menuPressing && !force) {
      pendingRender = true;
      return;
    }
    pendingRender = false;
    const dur = activeDuration();
    const isLive = Boolean((probeState.data && probeState.data.live) || mediaMeta(video).live);
    renderPreview(dur, isLive);
    // The qualities under a heading of their own (the whole-list rows come with theirs).
    const listed = listRows();
    const own = computeRows(probeState.data);
    const rows = [
      ...listed,
      ...(own.length && !own[0].sep ? [{ sep: uiText("Video quality · highest first", "Video kalitesi · en yüksekten") }] : []),
      ...own,
    ];

    const sig = JSON.stringify(rows.map((r) => [r.label || r.sep, r.badge || "", r.sub || "", r.selector, r.container || "", r.hlsAudio || "", r.hint || "", r.size, Boolean(r.approx), Boolean(r.list)]));
    if (sig !== lastRowSig || force) {
      lastRowSig = sig;
      qListEl.textContent = "";
      let section = "";
      for (const r of rows) {
        if (r.sep) {
          section = r.sep;
          const head = document.createElement("div");
          head.className = "q-sep";
          head.textContent = r.sep;
          qListEl.appendChild(head);
          continue;
        }
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = r.best ? "q-row best" : "q-row";
        btn.setAttribute("role", "menuitem");
        btn.dataset.selector = r.selector;
        if (r.hint) btn.title = r.hint;
        if (r.container) btn.dataset.container = r.container;
        if (r.hlsAudio) btn.dataset.hlsAudio = r.hlsAudio;
        if (r.size > 0) btn.dataset.size = String(r.size);
        if (r.list) btn.dataset.list = "1";
        const badge = document.createElement("span");
        badge.className = "q-badge";
        const b = rowBadge(r, section);
        badge.textContent = b.text;
        badge.dataset.tier = b.tier;
        const text = document.createElement("span");
        text.className = "q-text";
        const nameSpan = document.createElement("span");
        nameSpan.className = "q-name";
        nameSpan.textContent = r.badge && !r.best && !r.list && r.sub ? resolutionLabelOf(r) : r.label;
        if (r.hdr) {
          const hdr = document.createElement("span");
          hdr.className = "q-hdr";
          hdr.textContent = "HDR";
          nameSpan.appendChild(hdr);
        }
        text.appendChild(nameSpan);
        const subText = r.sub || r.hint || "";
        if (subText) {
          const sub = document.createElement("span");
          sub.className = "q-sub";
          sub.textContent = subText;
          text.appendChild(sub);
        }
        const sizeSpan = document.createElement("span");
        const sizeText = r.list ? uiText("Playlist", "Liste") : r.size > 0 ? formatSize(r.size, r.approx) : isLive ? uiText("Live", "Canlı") : "";
        sizeSpan.className = sizeText ? "q-size" : "q-size none";
        sizeSpan.textContent = sizeText || "—";
        btn.append(badge, text, sizeSpan);
        qListEl.appendChild(btn);
      }
    }
    probeNoteEl.classList.toggle("still", !probeState.loading);
    if (probeState.loading) {
      probeNoteEl.style.display = "flex";
      probeNoteEl.textContent = own.length
        ? uiText("Looking for more qualities (4K, 8K)...", "Daha fazla kalite aranıyor (4K, 8K)...")
        : uiText("Fetching format details...", "Format boyutları alınıyor...");
    } else if (probeState.status === "unavailable") {
      probeNoteEl.style.display = "flex";
      probeNoteEl.textContent = uiText("Open XDM dialog for full options", "Tüm seçenekler için XDM penceresini açın");
    } else {
      probeNoteEl.style.display = "none";
    }
    if (menuOpen) position();
  }

  function applyLocale() {
    pill.dir = appDir || ext.i18n.getMessage("@@bidi_dir") || "ltr";
    menu.dir = pill.dir;
    idleLabel = t("video_download");
    refreshPillLabel();
    go.title = `${idleLabel} — ${t("video_drag")}`;
    go.setAttribute("aria-label", idleLabel);
    dismiss.title = t("video_hide_here");
    dismiss.setAttribute("aria-label", t("video_hide_here"));
    item("download").textContent = `${t("video_more")}...`;
    const neverText = formatCatalog("video_never_site", [host]) || ext.i18n.getMessage("video_never_site", [host]) || `Never on ${host}`;
    item("never").textContent = neverText;
    item("never").parentElement.title = neverText;
    item("settings").textContent = t("video_settings");
    item("settings").parentElement.title = t("video_settings");
    renderRows();
  }
  applyLocale();

  // ---- which video ----------------------------------------------------------------------------
  const videos = document.getElementsByTagName("video");
  const audios = document.getElementsByTagName("audio");
  const iframes = document.getElementsByTagName("iframe");
  // Child iframes that reported an active <video> inside them: lets the parent document show the
  // floating download button on top of custom overlays that intercept pointer events over an iframe
  // (e.g. a parent-frame control layer sitting above a cross-origin video iframe).
  const frameVideos = new WeakMap();
  const frameHosts = new Set();
  const framePingAt = new WeakMap();

  function mediaMeta(v) {
    if (!v) return { duration: 0, height: 0, src: "", drm: false, live: false, isFrame: false, url: "" };
    if (typeof HTMLIFrameElement === "function" && v instanceof HTMLIFrameElement) {
      const fv = frameVideos.get(v);
      return {
        duration: fv && Number(fv.duration) > 0 ? Number(fv.duration) : 0,
        height: fv && Number(fv.height) > 0 ? Math.round(Number(fv.height)) : 0,
        src: fv && typeof fv.src === "string" && /^https?:/i.test(fv.src) ? fv.src : "",
        drm: Boolean(fv && fv.drm),
        live: Boolean(fv && fv.live),
        isFrame: true,
        url: (fv && typeof fv.url === "string" && fv.url) || "",
      };
    }
    return {
      duration: Number.isFinite(v.duration) && v.duration > 0 ? v.duration : 0,
      height: Number(v.videoHeight) > 0 ? Math.round(Number(v.videoHeight)) : 0,
      src: /^https?:/i.test(v.currentSrc || "") ? v.currentSrc : "",
      drm: Boolean(v.mediaKeys),
      live: v.duration === Infinity,
      isFrame: false,
      url: "",
    };
  }

  // Videos and iframes inside open shadow roots (players built as web components), looked for
  // again every few seconds at most.
  let shadowed = [];
  let shadowedIframes = [];
  let shadowedAt = 0;
  function refreshShadows() {
    if (Date.now() - shadowedAt < 3000) return;
    shadowedAt = Date.now();
    const found = [];
    const foundFrames = [];
    const walk = (root, depth) => {
      if (depth > 4) return;
      const w = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
      for (let n = w.nextNode(), seen = 0; n && seen < 20000; n = w.nextNode(), seen++) {
        if (n.shadowRoot) {
          found.push(...n.shadowRoot.querySelectorAll("video"));
          foundFrames.push(...n.shadowRoot.querySelectorAll("iframe"));
          walk(n.shadowRoot, depth + 1);
        }
      }
    };
    walk(document.documentElement, 0);
    shadowed = found;
    shadowedIframes = foundFrames;
  }
  function shadowVideos() {
    refreshShadows();
    return shadowed;
  }
  function allIframes() {
    refreshShadows();
    return [...iframes, ...shadowedIframes];
  }
  function activeFrameHosts() {
    for (const f of frameHosts) {
      if (!f.isConnected || !frameVideos.has(f)) frameHosts.delete(f);
    }
    return [...frameHosts];
  }

  function eligible(v) {
    if (!v.isConnected || isDismissed(v)) return false;
    const r = v.getBoundingClientRect();
    if (typeof HTMLIFrameElement === "function" && v instanceof HTMLIFrameElement) {
      const fv = frameVideos.get(v);
      if (!fv || !fv.active || fv.childShown) return false;
      if (r.width < MIN_W || r.height < MIN_H) return false;
      const s = getComputedStyle(v);
      return s.visibility !== "hidden" && s.display !== "none" && Number(s.opacity) > 0.05;
    }
    const audio = v instanceof HTMLAudioElement;
    if (audio ? !v.controls || r.width < MIN_W || r.height < MIN_AUDIO_H : r.width < MIN_W || r.height < MIN_H) {
      return false;
    }
    // A short muted loop without controls: a moving picture, not something to save.
    if (v.muted && v.loop && !v.controls && !(v.duration > CLIP_S)) return false;
    const s = getComputedStyle(v);
    return s.visibility !== "hidden" && s.display !== "none" && Number(s.opacity) > 0.05;
  }

  function pingIframesAt(x, y) {
    const now = Date.now();
    for (const f of allIframes()) {
      if (!f.isConnected || frameVideos.has(f)) continue;
      const r = f.getBoundingClientRect();
      if (r.width < MIN_W || r.height < MIN_H) continue;
      if (x < r.left || x > r.right || y < r.top || y > r.bottom) continue;
      const prevAt = framePingAt.get(f) || 0;
      if (now - prevAt < 1500) continue;
      framePingAt.set(f, now);
      try {
        f.contentWindow?.postMessage({ channel: "xdm-frame-video-ping-v1" }, "*");
      } catch {
        // Ignore cross-origin postMessage restrictions.
      }
    }
  }

  // The topmost eligible video under (x, y): players lay their controls over the video, so the
  // video itself rarely gets the pointer's events. Falls back to child iframes that reported an
  // active video when a parent-document overlay sits on top of the iframe.
  function videoAt(x, y) {
    let best = null;
    for (const v of [...videos, ...audios, ...shadowVideos()]) {
      const r = v.getBoundingClientRect();
      if (x < r.left || x > r.right || y < r.top || y > r.bottom || !eligible(v)) continue;
      if (!best || r.width * r.height < best.r.width * best.r.height) best = { v, r };
    }
    if (best) return best.v;
    for (const f of activeFrameHosts()) {
      const r = f.getBoundingClientRect();
      if (x < r.left || x > r.right || y < r.top || y > r.bottom || !eligible(f)) continue;
      if (!best || r.width * r.height < best.r.width * best.r.height) best = { v: f, r };
    }
    if (!best) pingIframesAt(x, y);
    return best && best.v;
  }

  // ---- placing it -------------------------------------------------------------------------
  // `place` is where it sits in the room the video leaves around it: x and y from 0 to 1, one of
  // them 0 or 1 (on an edge).
  function rects() {
    const r = video.getBoundingClientRect();
    const w = pill.offsetWidth || 150;
    const h = pill.offsetHeight || 28;
    return { r, w, h, roomX: Math.max(0, r.width - w - 2 * INSET), roomY: Math.max(0, r.height - h - 2 * INSET) };
  }

  // Is a control of the page under this spot (a viewport rectangle)? What counts is something
  // on top of the video that the user clicks and that is much smaller than the video: a button,
  // a small link, a watch-later or menu button on a preview (#421); a link or layer over the
  // whole video isn't one. On the video itself, and below it, is nothing to cover.
  function onControl(b, video, videoRect) {
    const area = (x) => Math.max(0, x.width) * Math.max(0, x.height);
    const small = (el) => area(el.getBoundingClientRect()) < area(videoRect) / 5;
    const points = [
      [(b.left + b.right) / 2, (b.top + b.bottom) / 2],
      [b.left + 4, b.top + 4],
      [b.right - 4, b.top + 4],
      [b.left + 4, b.bottom - 4],
      [b.right - 4, b.bottom - 4],
    ];
    for (const [x, y] of points) {
      for (const el of document.elementsFromPoint(x, y)) {
        if (el === root) continue;
        if (el === video || el.contains(video)) break;
        const control = el.closest("button, [role=button], a[href], input, select, [onclick]");
        if (control && !control.contains(video) && small(control)) return true;
        if (small(el) && getComputedStyle(el).cursor === "pointer") return true;
      }
    }
    return false;
  }

  // Where it goes now: the place the user gave it, else the default corner, first stepping
  // vertically below top-edge controls (like YouTube's (i) card icon) before trying other corners.
  function spot(r, w, h, roomX, roomY) {
    if (chosen) return place;
    const now = Date.now();
    if (auto.v === video && now - auto.t < AUTO_MS) return auto.at;
    let at = place;
    const candidates = [place];
    if (place.y === 0 && roomY > 0) {
      for (let step = 1; step <= 3; step++) {
        const y = (step * (h + 6)) / roomY;
        const top = r.top + INSET + y * roomY;
        if (y <= 0.4 && top + h <= r.top + r.height * 0.45) {
          candidates.push({ x: place.x, y });
        }
      }
    }
    for (const c of [{ x: 0, y: 0 }, { x: 0, y: 1 }, { x: 1, y: 1 }]) {
      if (c.x !== place.x || c.y !== place.y) candidates.push(c);
    }
    for (const c of candidates) {
      const left = r.left + INSET + c.x * roomX;
      const top = r.top + INSET + c.y * roomY;
      if (!onControl({ left, top, right: left + w, bottom: top + h }, video, r)) {
        at = c;
        break;
      }
    }
    auto = { v: video, at, t: now };
    return at;
  }

  function position() {
    if (!video) return;
    const { r, w, roomX, roomY } = rects();
    // A narrow video gets the icon only.
    pill.classList.toggle("compact", r.width < 360);
    const at = dragging ? dragging.at : spot(r, w, pill.offsetHeight || 28, roomX, roomY);
    const left = r.left + INSET + at.x * roomX;
    // In the part of the video on screen: one scrolled half out keeps its button in view.
    const h = pill.offsetHeight || 28;
    const visTop = Math.max(r.top, 0) + INSET;
    const visBottom = Math.min(r.bottom, innerHeight) - INSET - h;
    const top = Math.min(Math.max(r.top + INSET + at.y * roomY, visTop), Math.max(visTop, visBottom));
    root.style.setProperty("transform", `translate(${Math.round(left)}px, ${Math.round(top)}px)`, "important");
    if (menuOpen) placeMenu(top, left, w);
  }

  // The menu opens below the button, or above it near the bottom of the window; aligned with
  // whichever of its sides is nearer the video's middle, clamped within the viewport.
  function placeMenu(top, left, w) {
    const pillH = pill.offsetHeight || 30;
    const gap = 5;
    const pad = 8;
    const maxW = Math.max(200, Math.floor(innerWidth - pad * 2));
    menu.style.maxWidth = `${maxW}px`;
    menu.style.maxHeight = "";
    const naturalH = menu.offsetHeight || 280;
    const mw = Math.min(menu.offsetWidth || 312, maxW);
    const spaceBelow = Math.max(0, innerHeight - (top + pillH + gap) - pad);
    const spaceAbove = Math.max(0, top - gap - pad);
    const below = spaceBelow >= naturalH || spaceBelow >= spaceAbove;
    const availH = Math.max(120, Math.floor(below ? spaceBelow : spaceAbove));
    menu.style.maxHeight = `${availH}px`;
    const h = Math.min(naturalH, availH);
    let menuTop = below ? pillH + gap : -h - gap;
    if (top + menuTop < pad) menuTop = pad - top;
    if (top + menuTop + h > innerHeight - pad) {
      menuTop = Math.max(pad - top, innerHeight - pad - h - top);
    }
    menu.style.top = `${Math.round(menuTop)}px`;
    const { r } = rects();
    const rightHalf = left + w / 2 > r.left + r.width / 2;
    let menuLeft = rightHalf ? w - mw : 0;
    if (left + menuLeft + mw > innerWidth - pad) menuLeft = innerWidth - pad - left - mw;
    if (left + menuLeft < pad) menuLeft = pad - left;
    menu.style.left = `${Math.round(menuLeft)}px`;
    menu.style.right = "";
  }

  // Snap a free spot (fractions, after a drag) to the nearest edge.
  function snap(at) {
    const d = { left: at.x, right: 1 - at.x, top: at.y, bottom: 1 - at.y };
    const edge = Object.keys(d).reduce((a, b) => (d[a] <= d[b] ? a : b));
    // Along the edge it stays where it was dropped, but close to a corner it goes into it.
    const c = (n) => (n < 0.08 ? 0 : n > 0.92 ? 1 : n);
    if (edge === "left") return { x: 0, y: c(at.y) };
    if (edge === "right") return { x: 1, y: c(at.y) };
    if (edge === "top") return { x: c(at.x), y: 0 };
    return { x: c(at.x), y: 1 };
  }

  // ---- showing and hiding ---------------------------------------------------------------------
  function attach() {
    // Over the video in fullscreen: inside the fullscreen element (a fullscreen <video> itself
    // can't hold it: then it hides).
    const fs = document.fullscreenElement;
    const parent = fs && fs !== video && fs.contains(video) ? fs : document.documentElement;
    if (root.parentNode !== parent) parent.appendChild(root);
    return !(fs && (fs === video || !fs.contains(video)));
  }

  const warmedUrls = new Map();

  const isNativeHlsData = (d) =>
    Boolean(d && Array.isArray(d.formats) && d.formats.some((f) => f && f.protocol === "m3u8_native"));

  function warmProbe(v) {
    if (!v || !allowed || menuOpen) return;
    const prev = video;
    video = v;
    const ctx = videoContext();
    video = prev;
    if (!ctx) return;
    const key = ctx.url || ctx.src || location.href;
    const now = Date.now();
    if (probeCache.has(key)) return;
    const warmedAt = warmedUrls.get(key) || 0;
    if (warmedAt && now - warmedAt < 2500) return;
    warmedUrls.set(key, now);
    send({ type: "video-probe", ...ctx, ...(!isYouTubePage() ? { skipDesktop: true } : {}) }).then((r) => {
      if (!r || r.gone) return;
      const src = (typeof r.src === "string" && r.src) || ctx.src || "";
      if (isNativeHlsData(r.instant)) {
        probeCache.set(key, { data: r.instant, src, status: "ready", at: Date.now() });
      } else if (r.desktop && r.desktop.status === "ready") {
        probeCache.set(key, { data: r.desktop, src, status: "ready", at: Date.now() });
      } else if (r.instant && Array.isArray(r.instant.formats) && r.instant.formats.length) {
        probeCache.set(key, { data: r.instant, src, status: "instant", at: Date.now() });
      } else {
        warmedUrls.delete(key);
        if (src) {
          probeState.src = probeState.src || src;
        }
      }
    }).catch(() => {
      warmedUrls.delete(key);
    });
  }

  function show(v) {
    if (video !== v) {
      video = v;
      activeTitle = "";
      probeState = { own: "", src: "", data: null, loading: false, status: "" };
      closeMenu();
    }
    refreshPillLabel();
    if (!attach()) return hide(true);
    ensureGlassAttr();
    root.style.setProperty("display", "block", "important");
    position();
    if (!pill.classList.contains("shown")) requestAnimationFrame(() => pill.classList.add("shown"));
    if (!raf) raf = requestAnimationFrame(tick);
    if (typeof HTMLIFrameElement === "function" && v instanceof HTMLIFrameElement) {
      try {
        v.contentWindow?.postMessage({ channel: "xdm-frame-video-hide-v1" }, "*");
      } catch {
        // Ignore cross-origin postMessage restrictions.
      }
    } else {
      reportFrameVideo(false, true);
    }
    warmProbe(v);
  }

  // `now`: at once and whatever has the focus (dismissed, the video gone).
  function hide(now) {
    if (!now && (menuOpen || dragging || keyboardOn())) return;
    if (now) {
      closeMenu();
      shadow.activeElement?.blur();
    }
    pill.classList.remove("shown");
    cancelAnimationFrame(raf);
    raf = 0;
    const was = video;
    if (!(typeof HTMLIFrameElement === "function" && was instanceof HTMLIFrameElement)) {
      reportFrameVideo(false, false);
    }
    setTimeout(() => {
      if (!pill.classList.contains("shown") && video === was) root.style.setProperty("display", "none", "important");
    }, now || reduced ? 0 : 180);
  }

  // While shown: follow the video (scrolling, layout) and fade when the pointer rests.
  function tick() {
    raf = 0;
    if (!video || !video.isConnected || !eligible(video)) {
      video = null;
      return hide(true);
    }
    const r = video.getBoundingClientRect();
    const over =
      pointer.x >= r.left && pointer.x <= r.right && pointer.y >= r.top && pointer.y <= r.bottom;
    const onButton = root.matches(":hover");
    if (!onButton && !menuOpen && !dragging && !keyboardOn() && (!over || Date.now() - lastMove > IDLE_MS)) {
      return hide();
    }
    position();
    raf = requestAnimationFrame(tick);
  }

  // At most one look per frame, however fast the pointer moves.
  function onMove(e) {
    pointer = { x: e.clientX, y: e.clientY };
    lastMove = Date.now();
    if (!scan) scan = requestAnimationFrame(look);
  }

  function look() {
    scan = 0;
    if (dragging || allowed === false) return;
    const v = videoAt(pointer.x, pointer.y);
    if (!v) return;
    if (allowed === null) return void ask().then(() => allowed && look());
    show(v);
  }

  async function ask() {
    if (allowed !== null) return;
    allowed = false; // until answered: one question per page
    const r = await send({ type: "panel" });
    if (r && r.messages) {
      appMessages = r.messages;
      appDir = r.dir || null;
      appLang = r.lang || "";
      applyLocale();
    }
    allowed = Boolean(r && r.on);
    if (allowed) {
      try {
        const { positions } = await ext.storage.local.get({ positions: {} });
        if (positions[host]) {
          place = snap(positions[host]);
          chosen = true;
        }
      } catch {
        // Default spot.
      }
    }
  }

  function videoContext() {
    if (!video) return null;
    const meta = mediaMeta(video);
    const src = meta.src;
    const title = currentPageTitle();
    const box = video.getBoundingClientRect();
    const at = { x: box.left + box.width / 2, y: box.top + box.height / 2 };
    const duration = meta.duration;
    const height = meta.height;
    return {
      src,
      title,
      url: meta.isFrame ? location.href : videoPage(video),
      at,
      ...(duration > 0 ? { duration } : {}),
      ...(height > 0 ? { height } : {}),
      ...(probeState.own ? { own: probeState.own } : {}),
    };
  }

  function stopProbePoll() {
    if (pollTimer) {
      clearTimeout(pollTimer);
      pollTimer = 0;
    }
  }

  // How many times the menu asks for the desktop's (yt-dlp's) list while it's open: ~65 s.
  const PROBE_POLLS = 70;

  async function runProbe(ctx, seq, pollOnly, pollsLeft) {
    if (!ctx || !menuOpen || seq !== probeSeq) return;
    const r = await send({ type: "video-probe", ...ctx, ...(pollOnly ? { pollOnly: true } : {}) });
    if (!menuOpen || seq !== probeSeq || !r || r.gone) return;
    const own = r.own || ctx.url || location.href;
    const src = (typeof r.src === "string" && r.src) || probeState.src || ctx.src || "";
    if (src) probeState.src = src;
    let updated = false;
    if (r.instant && Array.isArray(r.instant.formats) && r.instant.formats.length) {
      probeState.data = r.instant;
      if (r.instant.title) {
        activeTitle = cleanVideoTitle(r.instant.title);
        refreshPillLabel();
      }
      if (isNativeHlsData(r.instant)) {
        stopProbePoll();
        probeState = { own, src, data: r.instant, loading: false, status: "ready" };
        probeCache.set(own, { data: r.instant, src, status: "ready", at: Date.now() });
        renderRows();
        return;
      }
      updated = true;
    }
    const desk = r.desktop;
    if (desk && desk.status === "ready") {
      stopProbePoll();
      probeState = { own, src, data: desk, loading: false, status: "ready" };
      if (desk.title) {
        activeTitle = cleanVideoTitle(desk.title);
        refreshPillLabel();
      }
      probeCache.set(own, { data: desk, src, status: "ready", at: Date.now() });
      renderRows();
      return;
    }
    if (desk && desk.status === "pending" && pollsLeft > 0) {
      probeState.own = own;
      probeState.loading = true;
      probeState.status = "pending";
      if (updated || pollOnly === false) renderRows();
      // yt-dlp's full list (every 4K/8K format) can take half a minute on a long video: keep
      // asking (twice a second, then once) and upgrade the menu when it comes.
      pollTimer = setTimeout(() => runProbe(ctx, seq, true, pollsLeft - 1), pollsLeft > PROBE_POLLS - 10 ? 500 : 1000);
      return;
    }
    stopProbePoll();
    probeState.own = own;
    probeState.loading = false;
    probeState.status = (desk && desk.status) || (probeState.data ? "instant" : "unavailable");
    renderRows();
  }

  function startProbe() {
    stopProbePoll();
    const ctx = videoContext();
    if (!ctx) return;
    const key = ctx.url || ctx.src || location.href;
    const kept = probeCache.get(key);
    const fresh = kept && Date.now() - kept.at < 120000 ? kept : null;
    const ready = Boolean(fresh && fresh.status === "ready");
    const cachedSrc = (fresh && fresh.src) || probeState.src || ctx.src || "";
    probeState = { own: key, src: cachedSrc, data: fresh ? fresh.data : null, loading: !ready, status: ready ? "ready" : fresh ? "instant" : "" };
    renderRows();
    const seq = ++probeSeq;
    if (!ready) runProbe(ctx, seq, false, PROBE_POLLS);
  }

  // ---- acting ---------------------------------------------------------------------------------
  async function download(pick) {
    if (busy || !video) return;
    busy = true;
    try {
      closeMenu();
      const meta = mediaMeta(video);
      // Encrypted (DRM): no tool can save it; say so rather than send it.
      if (meta.drm) return flash("err", t("video_drm"));
      const ctx = videoContext();
      if (!ctx) return;
      const key = ctx.url || ctx.src || location.href;
      const cached = probeCache.get(key);
      const resolvedSrc = ctx.src || probeState.src || (cached && cached.src) || "";
      const dur = activeDuration();
      const isLive = Boolean((probeState.data && probeState.data.live) || meta.live);
      const payload = {
        type: "video",
        ...ctx,
        ...(resolvedSrc ? { src: resolvedSrc } : {}),
        ...(dur > 0 && !ctx.duration ? { duration: dur } : {}),
        ...(isLive ? { live: true } : {}),
      };
      if (pick && pick.selector) {
        payload.selector = pick.selector;
        if (pick.container) payload.container = pick.container;
        if (pick.hlsAudio) payload.hlsAudio = pick.hlsAudio;
        // Only a size the site said: an estimate (rate x duration) went into the list as the
        // file's size and was wrong until the download corrected it.
        if (pick.size > 0 && !pick.approx && !pick.list) payload.size = Number(pick.size);
        if (pick.list) payload.list = true;
        label.textContent = uiText("Starting download...", "İndirme başlatılıyor...");
      } else {
        // "More options...": the app's Download video window (else its card asks the quality).
        payload.window = true;
      }
      // Asked now, not kept: one <video> plays many videos at the same address (a feed's previews).
      const r = await send(payload);
      if (r.gone) return;
      flash(r.ok ? "ok" : "err", t(r.ok ? "video_sent" : r.why === "options" ? "video_off" : r.why === "unnamed" ? "video_unnamed" : "video_failed"));
    } finally {
      busy = false;
    }
  }

  // A short confirmation on the button itself, then back to normal.
  function flash(kind, text) {
    flashing = true;
    pill.classList.remove("ok", "err");
    pill.classList.add(kind);
    label.textContent = text;
    lastMove = Date.now();
    position();
    setTimeout(() => {
      flashing = false;
      pill.classList.remove(kind);
      refreshPillLabel();
      if (video) position();
    }, 2200);
  }

  function openMenu() {
    menuOpen = true;
    // A fresh frame for the preview each time it opens.
    previewFor = null;
    pill.classList.add("menu-on");
    menu.classList.add("open");
    go.setAttribute("aria-expanded", "true");
    startProbe();
    position();
    (qListEl.querySelector("button") || menu.querySelector("button"))?.focus();
  }

  function closeMenu() {
    if (!menuOpen) return;
    menuOpen = false;
    stopProbePoll();
    probeSeq++;
    pill.classList.remove("menu-on");
    menu.classList.remove("open");
    go.setAttribute("aria-expanded", "false");
  }

  dismiss.addEventListener("click", (e) => {
    if (!e.isTrusted) return;
    e.stopPropagation();
    run("hide");
  });

  menu.addEventListener("pointerdown", (e) => {
    menuPressing = true;
    e.stopPropagation();
  });
  const endMenuPress = () => {
    if (!menuPressing) return;
    menuPressing = false;
    if (pendingRender && menuOpen) renderRows();
  };
  menu.addEventListener("pointerup", endMenuPress);
  menu.addEventListener("pointercancel", endMenuPress);

  menu.addEventListener("click", (e) => {
    endMenuPress();
    if (!e.isTrusted) return;
    e.stopPropagation();
    const qBtn = e.target.closest(".q-row");
    if (qBtn && qBtn.dataset.selector) {
      download({
        selector: qBtn.dataset.selector,
        container: qBtn.dataset.container || "",
        hlsAudio: qBtn.dataset.hlsAudio || "",
        size: Number(qBtn.dataset.size) || 0,
        list: qBtn.dataset.list === "1",
      });
      return;
    }
    const act = e.target.closest("[data-act]")?.dataset.act;
    if (act) run(act);
  });

  async function run(act) {
    if (act === "download") return download();
    closeMenu();
    if (act === "hide") {
      if (video) {
        dismissed.add(video);
        if (video.currentSrc) dismissedSrc.add(video.currentSrc);
      }
      video = null;
      return hide(true);
    }
    if (act === "never") {
      allowed = false;
      video = null;
      hide(true);
      return send({ type: "hide-site" });
    }
    if (act === "settings") send({ type: "options" });
  }

  // Keys: Escape closes the menu, arrows move through its items.
  shadow.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && menuOpen) {
      closeMenu();
      go.focus();
      e.stopPropagation();
    } else if (menuOpen && (e.key === "Enter" || e.key === " ") && menu.contains(shadow.activeElement)) {
      const el = shadow.activeElement;
      if (el.dataset.selector) {
        download({
          selector: el.dataset.selector,
          container: el.dataset.container || "",
          hlsAudio: el.dataset.hlsAudio || "",
          size: Number(el.dataset.size) || 0,
          list: el.dataset.list === "1",
        });
      } else if (el.dataset.act) {
        run(el.dataset.act);
      }
      e.preventDefault();
    } else if (menuOpen && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      const items = [...menu.querySelectorAll("button")];
      if (!items.length) return;
      const i = items.indexOf(shadow.activeElement);
      items[(i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length].focus();
      e.preventDefault();
    }
  });

  // The main part: a click opens format/quality dropdown (Shift+click sends Best immediately),
  // a drag moves it along the edge (and is not a click).
  // Only the user's own clicks and keys (never events a page script makes up).
  go.addEventListener("pointerdown", (e) => {
    if (e.button !== 0 || !video || !e.isTrusted) return;
    const { r, roomX, roomY } = rects();
    dragging = {
      id: e.pointerId,
      startX: e.clientX,
      startY: e.clientY,
      // Where in the button it was taken.
      dx: e.clientX - (r.left + INSET + (chosen ? place : auto.at || place).x * roomX),
      dy: e.clientY - (r.top + INSET + (chosen ? place : auto.at || place).y * roomY),
      at: chosen ? place : auto.at || place,
      moved: false,
    };
    go.setPointerCapture(e.pointerId);
    e.preventDefault();
  });
  go.addEventListener("pointermove", (e) => {
    if (!dragging || e.pointerId !== dragging.id) return;
    if (!dragging.moved && Math.hypot(e.clientX - dragging.startX, e.clientY - dragging.startY) < DRAG_PX) return;
    if (!dragging.moved) {
      dragging.moved = true;
      closeMenu();
      pill.classList.add("dragging");
    }
    const { r, roomX, roomY } = rects();
    const c = (n) => Math.min(1, Math.max(0, n));
    dragging.at = {
      x: roomX ? c((e.clientX - dragging.dx - r.left - INSET) / roomX) : 0,
      y: roomY ? c((e.clientY - dragging.dy - r.top - INSET) / roomY) : 0,
    };
    position();
  });
  const endDrag = (e) => {
    if (!dragging || e.pointerId !== dragging.id) return;
    const d = dragging;
    dragging = null;
    pill.classList.remove("dragging");
    if (!d.moved) {
      if (e.type === "pointerup") {
        if (e.shiftKey) download({ selector: "bv*+ba/b" });
        else if (menuOpen) closeMenu();
        else openMenu();
      }
      return;
    }
    place = snap(d.at);
    chosen = true;
    position();
    lastMove = Date.now();
    // Remembered per site, not in a private window (it would keep the site's name), and the
    // 200 sites used last only.
    if (ext.extension?.inIncognitoContext) return;
    try {
      ext.storage.local
        .get({ positions: {} })
        .then(({ positions }) => {
          delete positions[host];
          positions[host] = place;
          const sites = Object.keys(positions);
          for (const s of sites.slice(0, Math.max(0, sites.length - 200))) delete positions[s];
          return ext.storage.local.set({ positions });
        })
        .catch(() => {});
    } catch {
      teardown();
    }
  };
  go.addEventListener("pointerup", endDrag);
  go.addEventListener("pointercancel", endDrag);
  // Keyboard: Enter / Space (a real button's click without a pointer).
  go.addEventListener("click", (e) => {
    if (e.detail === 0 && e.isTrusted) {
      if (menuOpen) closeMenu();
      else openMenu();
    }
  });

  // ---- cross-frame video coordination -----------------------------------------------------------
  // When a parent page layers custom controls or a transparent overlay on top of a cross-origin
  // video <iframe>, pointer events land on the parent document instead of the iframe. Child
  // frames report active/cued videos upward so the parent can host the download pill above its
  // overlay, while direct hover inside an uncovered iframe hides any duplicate parent pill.
  const isChildFrame = (() => {
    try {
      return Boolean(window.parent && window.parent !== window);
    } catch {
      return false;
    }
  })();
  let lastFrameSig = "";
  let frameMutTimer = 0;
  let frameObserver = null;

  function findChildPlayerMedia() {
    const list = [...videos, ...audios, ...shadowVideos()];
    const direct = list.find(eligible);
    if (direct) return direct;
    if (innerWidth >= MIN_W && innerHeight >= MIN_H) {
      for (const v of list) {
        if (!v || !v.isConnected || isDismissed(v) || v instanceof HTMLAudioElement) continue;
        if (v.muted && v.loop && !v.controls && !(v.duration > CLIP_S)) continue;
        if (
          v.readyState > 0 ||
          Boolean(v.currentSrc) ||
          Boolean(v.closest?.(".html5-video-player, .video-js, .jwplayer, .plyr"))
        ) {
          return v;
        }
      }
    }
    const nested = activeFrameHosts().find(eligible);
    return nested || null;
  }

  function reportFrameVideo(force = false, shownOverride = null) {
    if (!isChildFrame) return;
    const found = findChildPlayerMedia();
    const childShown =
      shownOverride !== null
        ? Boolean(shownOverride)
        : Boolean(video && !(typeof HTMLIFrameElement === "function" && video instanceof HTMLIFrameElement) && pill.classList.contains("shown"));
    let payload;
    if (found) {
      const m = mediaMeta(found);
      payload = {
        channel: "xdm-frame-video-v1",
        active: true,
        duration: m.duration,
        height: m.height,
        width: Number(found.videoWidth) || 0,
        src: m.src,
        url: m.isFrame ? m.url : videoPage(found) || location.href,
        drm: m.drm,
        live: m.live,
        childShown,
      };
    } else {
      payload = { channel: "xdm-frame-video-v1", active: false, childShown: false };
    }
    const sig = JSON.stringify([
      payload.active,
      Math.round(payload.duration || 0),
      payload.height || 0,
      payload.src || "",
      Boolean(payload.drm),
      Boolean(payload.live),
      Boolean(payload.childShown),
    ]);
    if (!force && sig === lastFrameSig) return;
    lastFrameSig = sig;
    try {
      window.parent.postMessage(payload, "*");
    } catch {
      // Ignore restricted parent postMessage.
    }
  }

  function onFrameVideoMessage(ev) {
    const d = ev && ev.data;
    if (!d || typeof d !== "object") return;
    if (d.channel === "xdm-frame-video-ping-v1") {
      if (isChildFrame) reportFrameVideo(true);
      return;
    }
    if (d.channel === "xdm-frame-video-hide-v1") {
      if (isChildFrame && video) hide(true);
      return;
    }
    if (d.channel !== "xdm-frame-video-v1" || !ev.source || ev.source === window) return;
    const hostFrame = allIframes().find((f) => {
      try {
        return f.contentWindow === ev.source;
      } catch {
        return false;
      }
    });
    if (!hostFrame) return;
    if (d.active) {
      frameVideos.set(hostFrame, {
        active: true,
        duration: Number(d.duration) > 0 ? Number(d.duration) : 0,
        height: Number(d.height) > 0 ? Math.round(Number(d.height)) : 0,
        width: Number(d.width) > 0 ? Math.round(Number(d.width)) : 0,
        src: typeof d.src === "string" ? d.src : "",
        url: typeof d.url === "string" ? d.url : "",
        drm: Boolean(d.drm),
        live: Boolean(d.live),
        childShown: Boolean(d.childShown),
        at: Date.now(),
      });
      frameHosts.add(hostFrame);
      if (d.childShown && video === hostFrame) {
        hide(true);
      } else if (!d.childShown && Date.now() - lastMove <= IDLE_MS && !scan) {
        scan = requestAnimationFrame(look);
      }
    } else {
      frameVideos.delete(hostFrame);
      frameHosts.delete(hostFrame);
      if (video === hostFrame) hide(true);
    }
    if (isChildFrame) reportFrameVideo();
  }
  window.addEventListener("message", onFrameVideoMessage);

  // ---- wiring -----------------------------------------------------------------------------------
  // The extension updated or removed under this page: the button and its listeners go (a fresh
  // copy of this script comes with the new extension).
  function teardown() {
    cancelAnimationFrame(raf);
    cancelAnimationFrame(scan);
    stopProbePoll();
    if (frameMutTimer) {
      clearTimeout(frameMutTimer);
      frameMutTimer = 0;
    }
    if (frameObserver) {
      frameObserver.disconnect();
      frameObserver = null;
    }
    removeEventListener("pointermove", onMove, true);
    removeEventListener("keydown", onTab, true);
    removeEventListener("message", onManifestHint);
    removeEventListener("message", onFrameVideoMessage);
    root.remove();
    allowed = false;
    video = null;
  }
  globalThis.__mdmVideoButton = { alive: () => Boolean(ext.runtime?.id), teardown };
  // Nothing done on the button is the page's business. The button sits over the player, and a
  // player takes a click on itself for play/pause and a double click for fullscreen (#38: picking
  // a quality on YouTube put the browser in fullscreen), so every pointer event is stopped once
  // it has left the button's own tree. These listen on the host and bubble, so the button's own
  // handlers inside the shadow tree have already run; the page's capturing listeners (the one
  // above that closes the menu among them) are untouched, as capture runs before any of this.
  for (const type of ["pointerdown", "pointerup", "mousedown", "mouseup", "click", "dblclick", "auxclick", "contextmenu", "wheel"]) {
    root.addEventListener(type, (e) => e.stopPropagation());
  }
  // The second half of a double click (#38, again). A pick closes the menu at the first click
  // (the close button, the whole button), and the second click then lands on what was under it:
  // on YouTube the player, which takes it for pause and the double click for fullscreen. People
  // double-click a list's rows as often as not, so a press right where a click on the button
  // just was, within the double-click time, is swallowed whole once the button isn't under it
  // any more. The time is the longest Windows lets the double-click speed be set to; the place,
  // a few of its pixels, generously. Clicks elsewhere, and any later, are the page's as always.
  const AFTER_MS = 900;
  const AFTER_PX = 10;
  let after = null;
  root.addEventListener("click", (e) => {
    // Capturing at the host: the menu's own handler stops the click inside the tree.
    if (e.detail > 0) after = { x: e.clientX, y: e.clientY, until: e.timeStamp + AFTER_MS };
  }, true);
  for (const type of ["pointerdown", "mousedown", "pointerup", "mouseup", "click", "dblclick"]) {
    addEventListener(type, (e) => {
      if (!after) return;
      if (e.timeStamp > after.until) {
        after = null;
        return;
      }
      if (Math.abs(e.clientX - after.x) > AFTER_PX || Math.abs(e.clientY - after.y) > AFTER_PX) return;
      if (e.composedPath().includes(root)) return;
      e.stopImmediatePropagation();
      e.preventDefault();
    }, true);
  }
  addEventListener("pointermove", onMove, { capture: true, passive: true });
  addEventListener("pointerdown", (e) => {
    if (menuOpen && !e.composedPath().includes(root)) closeMenu();
  }, { capture: true, passive: true });
  // A video with the keyboard's focus, or a player around one (its controls), gets the button
  // too, kept while the focus stays there; Tab goes from there to the button.
  addEventListener("focusin", (e) => {
    if (!(e.target instanceof Element) || e.composedPath().includes(root)) return;
    const t = e.target;
    const v =
      t instanceof HTMLMediaElement ? t : [...t.querySelectorAll("video, audio, iframe")].find(eligible) || null;
    if (!v || !eligible(v)) return;
    focusedPlayer = t;
    ask().then(() => {
      if (!allowed) return;
      lastMove = Date.now();
      show(v);
    });
  }, true);
  // Tab on the focused player moves into the button first; Tab from the button goes back to
  // the player and on from there (its own controls next), as if the button sat at its start.
  let tabOn = 0;
  function onTab(e) {
    if (e.key !== "Tab" || e.shiftKey) return;
    // The Tab that just left the button: on from the player, as the browser goes.
    if (Date.now() - tabOn < 100) {
      tabOn = 0;
      return;
    }
    if (video && playerFocused() && root.style.getPropertyValue("display") === "block") {
      go.focus();
      e.preventDefault();
    }
  }
  addEventListener("keydown", onTab, true);
  go.addEventListener("keydown", (e) => {
    if (e.key === "Tab" && !e.shiftKey && focusedPlayer && focusedPlayer.isConnected) {
      tabOn = Date.now();
      focusedPlayer.focus();
    }
  });
  function prime() {
    reportFrameVideo();
    const list = [...videos, ...audios, ...shadowVideos()];
    const v = list.find(eligible);
    if (!v) return;
    ask().then(() => {
      if (!allowed) return;
      const ctx = {
        src: /^https?:/i.test(v.currentSrc || "") ? v.currentSrc : "",
        title: currentPageTitle(),
        url: videoPage(v),
        ...(Number.isFinite(v.duration) && v.duration > 0 ? { duration: v.duration } : {}),
        ...(Number(v.videoHeight) > 0 ? { height: Math.round(Number(v.videoHeight)) } : {}),
      };
      const key = ctx.url || ctx.src || location.href;
      if (probeCache.has(key)) return;
      send({ type: "video-probe", ...ctx, pollOnly: false, ...(!isYouTubePage() ? { skipDesktop: true } : {}) }).then((r) => {
        if (!r || r.gone) return;
        const src = (typeof r.src === "string" && r.src) || ctx.src || "";
        if (r.instant && Array.isArray(r.instant.formats) && r.instant.formats.length) {
          probeCache.set(key, {
            data: r.instant,
            src,
            status: isNativeHlsData(r.instant) ? "ready" : "instant",
            at: Date.now(),
          });
        } else if (src && !probeState.src) {
          probeState.src = src;
        }
      }).catch(() => {});
    });
  }
  if (document.readyState === "loading") addEventListener("DOMContentLoaded", prime, { once: true });
  else prime();
  addEventListener("load", prime, { once: true });
  document.addEventListener("play", prime, { capture: true, passive: true });
  document.addEventListener("playing", () => reportFrameVideo(), { capture: true, passive: true });
  document.addEventListener("loadedmetadata", prime, { capture: true, passive: true });
  document.addEventListener("loadeddata", () => reportFrameVideo(), { capture: true, passive: true });
  document.addEventListener("canplay", () => reportFrameVideo(), { capture: true, passive: true });
  document.addEventListener("durationchange", () => reportFrameVideo(), { capture: true, passive: true });
  if (isChildFrame) {
    addEventListener("resize", () => reportFrameVideo(), { passive: true });
    if (typeof MutationObserver === "function" && document.documentElement) {
      frameObserver = new MutationObserver(() => {
        if (frameMutTimer) return;
        frameMutTimer = setTimeout(() => {
          frameMutTimer = 0;
          reportFrameVideo();
        }, 250);
      });
      frameObserver.observe(document.documentElement, { childList: true, subtree: true });
    }
  }
  setTimeout(prime, 800);
  setTimeout(prime, 2500);

  document.addEventListener("fullscreenchange", () => video && show(video));
  addEventListener("blur", () => hide());
})();
