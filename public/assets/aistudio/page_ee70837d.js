// Xenon Download Manager's link watcher (a content script, in every page and frame):
//  - a link clicked with Alt held is the browser's to download (background.js leaves its download
//    alone), the key the app's help names for it;
//  - a direct click on a link whose type Options capture is held here immediately and handed to
//    background.js (`link`) before the browser opens a connection or waits for headers; if the app
//    doesn't take it, the click is re-dispatched as if never held.
(() => {
  // One watcher per page: a copy cut off from the extension (after an update) gives way.
  const prev = globalThis.__mdmPage;
  if (prev && typeof prev === "object" && prev.alive()) return;
  if (prev && typeof prev.stop === "function") prev.stop();
  const ext = globalThis.browser ?? globalThis.chrome;
  const OPENED_SET = new Set([
    "PDF", "MP3", "MP4", "M4A", "M4V", "OGG", "OGV", "WAV", "AAC",
    "MOV", "TIF", "TIFF", "WMA", "WMV", "AVI", "MKV",
    "WEBM", "FLV", "F4V", "TS", "MPEG", "MPG", "3GP", "3G2", "ASF", "FLAC", "OPUS", "WEBA",
  ]);
  const CODE_VIEW_HOSTS = /^(github\.com|gitlab\.com|bitbucket\.org|codeberg\.org)$/i;
  const isCodeViewerLink = (href) => {
    try {
      const u = new URL(href);
      return CODE_VIEW_HOSTS.test(u.hostname) && /\/(blob|tree|src\/branch|src\/commit)\//.test(u.pathname);
    } catch {
      return false;
    }
  };

  const tell = (message) => {
    try {
      return ext.runtime.sendMessage(message).catch(() => null);
    } catch {
      // The extension was updated or removed since this page loaded: nothing to tell.
      return Promise.resolve(null);
    }
  };

  // ---- Instant click capture rules (Options, as background.js keeps them) ---------------------

  let rules = null;
  let paused = false;
  let patterns = [];
  let siteExcludes = [];
  let addressExcludes = [];

  const wildcard = (pattern, text) => {
    const re = new RegExp(
      "^" + pattern.toLowerCase().split("*").map((s) => s.replace(/[.+?^${}()|[\]\\]/g, "\\$&")).join(".*") + "$",
    );
    return re.test(text.toLowerCase());
  };
  const hostMatches = (host, pattern) => {
    let p = pattern.trim().toLowerCase().replace(/^[a-z]+:\/\//, "").split("/")[0];
    if (p.startsWith("*.")) {
      p = p.slice(2);
      return host === p || host.endsWith("." + p);
    }
    return p !== "" && host === p;
  };
  const addressMatches = (pattern, url) => {
    const p = String(pattern || "").trim();
    if (!p) return false;
    return wildcard(p, p.includes("://") ? url : url.replace(/^[a-z][a-z0-9+.-]*:\/\//i, ""));
  };
  const isExcluded = (url) => {
    if (!url || (!siteExcludes.length && !addressExcludes.length)) return false;
    let host = "";
    try {
      host = new URL(url).hostname.toLowerCase();
    } catch {
      return false;
    }
    if (siteExcludes.some((p) => hostMatches(host, p))) return true;
    return addressExcludes.some((p) => addressMatches(p, url));
  };

  const learn = (nextRules) => {
    rules = nextRules || null;
    // As background.js typePatterns reads the list: space/comma/semicolon separated, optional
    // leading `*.` or `.` stripped, `*` any run.
    patterns =
      rules && rules.capture !== false
        ? String(rules.types || "")
            .split(/[\s,;]+/)
            .map((raw) => {
              const t = raw.trim();
              if (!t) return "";
              return t === "*" ? "*" : t.replace(/^\*\./, "").replace(/^\.+/, "");
            })
            .filter(Boolean)
            .map((t) => new RegExp("^" + t.replace(/[.+?^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*") + "$", "i"))
        : [];
    siteExcludes = rules && rules.excludedSites ? String(rules.excludedSites).split(/\s+/).filter(Boolean) : [];
    addressExcludes = rules && Array.isArray(rules.excludedAddresses) ? rules.excludedAddresses : [];
  };
  const onStorage = (changes, area) => {
    if (area !== "local") return;
    if (changes.rules) learn(changes.rules.newValue);
    if (changes.paused) paused = Boolean(changes.paused.newValue);
  };
  ext.storage.local.get({ rules: null, paused: false }).then(
    (v) => {
      paused = Boolean(v && v.paused);
      learn(v && v.rules);
    },
    () => {},
  );
  ext.storage.onChanged.addListener(onStorage);
  // Ask background.js to sync Options' rules when the page loads or returns to foreground so
  // added/removed file types in Options take effect immediately across all browsers.
  tell({ type: "rules" });
  const onVis = () => {
    if (typeof document !== "undefined" && document.visibilityState === "visible") tell({ type: "rules" });
  };
  if (typeof document !== "undefined" && document.addEventListener) {
    document.addEventListener("visibilitychange", onVis);
  }

  // The file type a link leads to: its `download` name's, else its path's last name's.
  const typeOf = (a) => {
    let name = a.getAttribute("download") || "";
    if (!name) {
      try {
        name = new URL(a.href).pathname;
      } catch {
        return "";
      }
    }
    const base = name.split("/").pop() || "";
    const dot = base.lastIndexOf(".");
    return dot > 0 ? base.slice(dot + 1) : "";
  };

  const hold = (e, a) => {
    e.preventDefault();
    e.stopPropagation?.();
    // The `download` name, when the link has one: the app's rules decide by it, as page.js did.
    const name = a.getAttribute("download") || "";
    tell({ type: "link", url: a.href, ...(name ? { name } : {}) }).then((reply) => {
      if (reply && reply.taken) return;
      // Not the app's (Options, an excluded site, XDM not running): the click as it was, its
      // Cmd/Shift/Ctrl too (a new tab or window stays one). This one isn't trusted, so `note`
      // leaves it alone.
      const keys = { ctrlKey: e.ctrlKey, metaKey: e.metaKey, shiftKey: e.shiftKey };
      try {
        a.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, button: 0, ...keys }));
      } catch {
        a.click();
      }
    });
  };

  const note = (e) => {
    if (!e.isTrusted || !(e.target instanceof Element)) return;
    const a = e.target.closest("a[href]");
    if (!a || !/^https?:/i.test(a.href)) return;
    if (e.altKey) {
      tell({ type: "bypass", url: a.href });
      return;
    }
    if (!e.defaultPrevented && !paused && e.type === "click" && e.button === 0 && patterns.length) {
      const type = typeOf(a);
      if (
        type &&
        !isCodeViewerLink(a.href) &&
        patterns.some((p) => p.test(type)) &&
        !(rules && rules.opened === false && OPENED_SET.has(type.toUpperCase())) &&
        !isExcluded(a.href) &&
        !isExcluded(globalThis.location?.href || "")
      ) {
        hold(e, a);
      }
    }
  };
  addEventListener("click", note, true);
  addEventListener("auxclick", note, true);
  globalThis.__mdmPage = {
    alive: () => Boolean(ext.runtime?.id),
    stop: () => {
      removeEventListener("click", note, true);
      removeEventListener("auxclick", note, true);
      if (typeof document !== "undefined" && document.removeEventListener) {
        document.removeEventListener("visibilitychange", onVis);
      }
      ext.storage.onChanged.removeListener(onStorage);
    },
  };
})();
