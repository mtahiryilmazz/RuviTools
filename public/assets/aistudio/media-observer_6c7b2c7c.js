// Passive MAIN-world manifest observer for Xenon Download Manager.
// Watches responses the page's own player already fetched (HLS #EXTM3U / DASH <MPD) without
// issuing any extra network requests — preserving single-use/short-lived verification tokens.
(() => {
  if (globalThis.__xdmMediaObserverInstalled) return;
  globalThis.__xdmMediaObserverInstalled = true;

  const MAX_BYTES = 1024 * 1024;
  let sent = 0;
  let windowStart = 0;
  let lastDrmHint = 0;

  function publishDrm() {
    const now = Date.now();
    if (now - lastDrmHint < 3000) return;
    lastDrmHint = now;
    try {
      window.postMessage(
        { channel: "xdm-drm-hint-v1" },
        location.origin === "null" ? "*" : location.origin,
      );
    } catch {
      try {
        window.postMessage({ channel: "xdm-drm-hint-v1" }, "*");
      } catch {
        // Ignore cross-origin postMessage restrictions.
      }
    }
  }

  function publish(url, mime, text) {
    const now = Date.now();
    if (now - windowStart >= 1000) {
      windowStart = now;
      sent = 0;
    }
    if (++sent > 32) return;
    try {
      window.postMessage(
        {
          channel: "xdm-media-hint-v1",
          url,
          mime: String(mime || "").slice(0, 128),
          text,
        },
        location.origin === "null" ? "*" : location.origin,
      );
    } catch {
      try {
        window.postMessage(
          {
            channel: "xdm-media-hint-v1",
            url,
            mime: String(mime || "").slice(0, 128),
            text,
          },
          "*",
        );
      } catch {
        // Ignore cross-origin postMessage restrictions.
      }
    }
  }

  function inspect(url, mime, value) {
    if (typeof url !== "string" || !/^https?:\/\//i.test(url)) return;
    let text = "";
    if (typeof value === "string") {
      if (value.length === 0 || value.length > MAX_BYTES) return;
      text = value;
    } else if (value && typeof value.byteLength === "number") {
      if (value.byteLength === 0 || value.byteLength > MAX_BYTES) return;
      try {
        const bytes = new Uint8Array(value);
        const head = new TextDecoder().decode(bytes.subarray(0, Math.min(bytes.length, 4096)));
        if (!/^\uFEFF?\s*(?:#EXTM3U|(?:<\?xml[^>]*>\s*)?(?:<!--[\s\S]*?-->\s*)*<(?:[\w-]+:)?MPD[\s>/])/i.test(head)) {
          return;
        }
        text = new TextDecoder().decode(bytes);
      } catch {
        return;
      }
    } else {
      return;
    }
    const trimmed = text.replace(/^\uFEFF/, "").trimStart();
    if (
      trimmed.startsWith("#EXTM3U") ||
      /^(?:<\?xml[^>]*>\s*)?(?:<!--[\s\S]*?-->\s*)*<(?:[\w-]+:)?MPD[\s>/]/i.test(trimmed)
    ) {
      publish(url, mime, trimmed);
    }
  }

  if (typeof Response === "function" && Response.prototype) {
    for (const method of ["text", "arrayBuffer"]) {
      const orig = Response.prototype[method];
      if (typeof orig !== "function") continue;
      Response.prototype[method] = function (...args) {
        const pending = Reflect.apply(orig, this, args);
        try {
          const url = this && typeof this.url === "string" ? this.url : "";
          if (/^https?:\/\//i.test(url)) {
            const mime = this.headers && typeof this.headers.get === "function" ? this.headers.get("content-type") || "" : "";
            Promise.resolve(pending).then(
              (val) => inspect(url, mime, val),
              () => {},
            );
          }
        } catch {
          // Never interfere with the page's own response consumption.
        }
        return pending;
      };
    }
  }

  if (typeof XMLHttpRequest === "function" && XMLHttpRequest.prototype) {
    const origOpen = XMLHttpRequest.prototype.open;
    if (typeof origOpen === "function") {
      XMLHttpRequest.prototype.open = function (...args) {
        try {
          this.addEventListener(
            "load",
            () => {
              try {
                const url = typeof this.responseURL === "string" ? this.responseURL : "";
                if (!/^https?:\/\//i.test(url)) return;
                const mime = typeof this.getResponseHeader === "function" ? this.getResponseHeader("content-type") || "" : "";
                if (this.responseType === "" || this.responseType === "text") {
                  inspect(url, mime, this.responseText);
                } else if (this.responseType === "arraybuffer" && this.response) {
                  inspect(url, mime, this.response);
                }
              } catch {
                // Ignore XHR read errors.
              }
            },
            { once: true },
          );
        } catch {
          // Ignore listener errors.
        }
        return Reflect.apply(origOpen, this, args);
      };
    }
  }
  if (typeof HTMLMediaElement === "function" && HTMLMediaElement.prototype) {
    const origSetMediaKeys = HTMLMediaElement.prototype.setMediaKeys;
    if (typeof origSetMediaKeys === "function") {
      HTMLMediaElement.prototype.setMediaKeys = function (keys, ...rest) {
        try {
          if (keys) publishDrm();
        } catch {
          // Never interfere with media playback.
        }
        return Reflect.apply(origSetMediaKeys, this, [keys, ...rest]);
      };
    }
  }
})();
