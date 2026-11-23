/*
 * RuviTube — MAIN world passive YouTube player probe.
 * Reads streamingData & metadata already loaded by YouTube's player without
 * issuing extra network requests.
 */
(() => {
  'use strict';

  if (globalThis.__ruvitubePlayerProbeInstalled) return;
  globalThis.__ruvitubePlayerProbeInstalled = true;

  const ORIGIN = location.origin === 'null' ? '*' : location.origin;

  function readPlayerData() {
    try {
      const player = document.getElementById('movie_player') || document.querySelector('.html5-video-player');
      const vd = player && typeof player.getVideoData === 'function' ? player.getVideoData() : null;
      const vidId = String((vd && vd.video_id) || '');

      let pr = player && typeof player.getPlayerResponse === 'function' ? player.getPlayerResponse() : null;
      if (pr && pr.videoDetails && vidId && pr.videoDetails.videoId && pr.videoDetails.videoId !== vidId) {
        pr = null;
      }
      if (!pr && globalThis.ytInitialPlayerResponse) {
        const init = globalThis.ytInitialPlayerResponse;
        if (!vidId || (init.videoDetails && init.videoDetails.videoId === vidId)) {
          pr = init;
        }
      }

      const details = (pr && pr.videoDetails) || {};
      const finalId = vidId || String(details.videoId || '');
      if (!finalId) return null;

      const videoEl = document.querySelector('#movie_player video, video');
      const domDur = videoEl && Number.isFinite(videoEl.duration) ? videoEl.duration : 0;
      const playerDur = player && typeof player.getDuration === 'function' ? Number(player.getDuration()) || 0 : 0;
      const duration = Number(details.lengthSeconds) || playerDur || domDur || 0;
      const isLive = Boolean(details.isLive || (details.isLiveContent && !(duration > 0)));
      const title = String((vd && vd.title) || details.title || '');
      const channel = String(details.author || '');

      const thumbs = (details.thumbnail && Array.isArray(details.thumbnail.thumbnails) && details.thumbnail.thumbnails) || [];
      const lastThumb = thumbs.length ? String(thumbs[thumbs.length - 1].url || '') : '';
      const thumbnail = /^https:\/\//.test(lastThumb)
        ? lastThumb
        : `https://i.ytimg.com/vi/${finalId}/mqdefault.jpg`;

      const sd = (pr && pr.streamingData) || {};
      const formats = [];
      const pushList = (list, muxed) => {
        if (!Array.isArray(list)) return;
        for (const f of list) {
          if (!f || !f.mimeType) continue;
          const mime = String(f.mimeType);
          const cm = /codecs="([^"]+)"/i.exec(mime);
          const codecs = (cm ? cm[1] : '').split(',').map(s => s.trim()).filter(Boolean);
          const isVideo = /^video\//i.test(mime);
          const isAudio = /^audio\//i.test(mime);
          const ext = /webm/i.test(mime) ? 'webm' : (isAudio && /mp4/i.test(mime) ? 'm4a' : 'mp4');
          const bitrate = Number(f.averageBitrate) || Number(f.bitrate) || 0;
          const fmtDur = Number(f.approxDurationMs) > 0 ? Number(f.approxDurationMs) / 1000 : duration;
          const totalBytes = Number(f.contentLength) || (fmtDur > 0 && bitrate > 0 ? Math.round((bitrate / 8) * fmtDur) : 0);
          const qLabel = String(f.qualityLabel || '');
          const labelHeight = Number((/^(\d{3,4})p/i.exec(qLabel) || [])[1]) || 0;
          const labelFps = Number((/^\d{3,4}p(\d{2,3})/i.exec(qLabel) || [])[1]) || 0;
          const transfer = String((f.colorInfo && f.colorInfo.transferCharacteristics) || '');
          const hdr = isVideo && (/HDR/i.test(qLabel) || /SMPTEST2084|ARIB_STD_B67|HLG|PQ/i.test(transfer));

          formats.push({
            id: f.itag ? `itag-${f.itag}` : '',
            kind: isVideo ? (muxed ? 'videoaudio' : 'video') : 'audio',
            container: ext,
            width: Number(f.width) || 0,
            height: Number(f.height) || labelHeight,
            fps: Number(f.fps) || labelFps,
            hdr,
            vcodec: isVideo ? (codecs[0] || ext) : 'none',
            acodec: isVideo ? (muxed ? (codecs[1] || 'mp4a') : 'none') : (codecs[0] || ext),
            tbr: bitrate > 0 ? Math.round(bitrate / 1000) : 0,
            total_bytes: totalBytes
          });
        }
      };

      pushList(sd.formats, true);
      pushList(sd.adaptiveFormats, false);

      function parseTimeSec(raw) {
        const s = String(raw || '').trim();
        const m = /^(?:(\d{1,3}):)?(\d{1,2}):(\d{2})$/.exec(s);
        if (!m) return null;
        const h = m[1] ? Number(m[1]) : 0;
        const min = Number(m[2]);
        const sec = Number(m[3]);
        if (min >= 60 || sec >= 60) return null;
        return h * 3600 + min * 60 + sec;
      }

      function parseChaptersFromDescription(descText) {
        if (!descText || typeof descText !== 'string') return [];
        const lines = descText.split(/\r?\n/);
        const out = [];
        const tsPattern = /(?:^|[\s([\-–—|])((?:\d{1,3}:)?\d{1,2}:\d{2})(?=$|[\s)\]\-–—|:.,])/;
        for (const rawLine of lines) {
          const line = rawLine.trim();
          if (!line || line.length > 220) continue;
          const m = tsPattern.exec(line);
          if (!m) continue;
          const tsStr = m[1];
          const sec = parseTimeSec(tsStr);
          if (sec === null || !Number.isFinite(sec) || sec < 0) continue;
          const idx = line.indexOf(tsStr);
          const before = line.slice(0, idx).replace(/[\s([\-–—|:]+$/, '').trim();
          const after = line.slice(idx + tsStr.length).replace(/^[\s)\]\-–—|:]+/, '').trim();
          let label = '';
          if (before && after) {
            label = `${before} ${after}`.replace(/\s+/g, ' ').trim();
          } else {
            label = (after || before || '').trim();
          }
          label = label.replace(/^[-–—:|•·]+\s*/, '').replace(/\s*[-–—:|•·]+$/, '').trim();
          if (label) {
            out.push({ startSec: sec, title: label });
          }
        }
        return out;
      }

      function extractInitialDataChapters(vid) {
        const initData = globalThis.ytInitialData;
        if (!initData || typeof initData !== 'object') return [];
        const found = [];
        try {
          const panels = initData.engagementPanels || [];
          for (const p of panels) {
            const r = p && p.engagementPanelSectionListRenderer;
            const list = r && r.content && r.content.macroMarkersListRenderer && r.content.macroMarkersListRenderer.contents;
            if (!Array.isArray(list)) continue;
            for (const item of list) {
              const m = item && item.macroMarkersListItemRenderer;
              if (!m) continue;
              const tText = (m.title && (m.title.simpleText || (Array.isArray(m.title.runs) && m.title.runs.map(x => x.text).join('')))) || '';
              const timeText = (m.timeDescription && (m.timeDescription.simpleText || (Array.isArray(m.timeDescription.runs) && m.timeDescription.runs.map(x => x.text).join('')))) || '';
              const tapSec = m.onTap && m.onTap.watchEndpoint && Number(m.onTap.watchEndpoint.startTimeSeconds);
              const sec = Number.isFinite(tapSec) ? tapSec : parseTimeSec(timeText);
              if (sec !== null && Number.isFinite(sec) && tText.trim()) {
                found.push({ startSec: sec, title: tText.trim() });
              }
            }
          }
        } catch (_) {}
        if (found.length > 0) return found;
        try {
          const markersMap = initData.playerOverlays
            && initData.playerOverlays.playerOverlayRenderer
            && initData.playerOverlays.playerOverlayRenderer.decoratedPlayerBarRenderer
            && initData.playerOverlays.playerOverlayRenderer.decoratedPlayerBarRenderer.decoratedPlayerBarRenderer
            && initData.playerOverlays.playerOverlayRenderer.decoratedPlayerBarRenderer.decoratedPlayerBarRenderer.playerBar
            && initData.playerOverlays.playerOverlayRenderer.decoratedPlayerBarRenderer.decoratedPlayerBarRenderer.playerBar.multiMarkersPlayerBarRenderer
            && initData.playerOverlays.playerOverlayRenderer.decoratedPlayerBarRenderer.decoratedPlayerBarRenderer.playerBar.multiMarkersPlayerBarRenderer.markersMap;
          if (Array.isArray(markersMap)) {
            for (const entry of markersMap) {
              const chList = entry && entry.value && entry.value.chapters;
              if (!Array.isArray(chList)) continue;
              for (const c of chList) {
                const cr = c && c.chapterRenderer;
                if (!cr) continue;
                const tText = (cr.title && (cr.title.simpleText || (Array.isArray(cr.title.runs) && cr.title.runs.map(x => x.text).join('')))) || '';
                const ms = Number(cr.timeRangeStartMillis);
                if (Number.isFinite(ms) && tText.trim()) {
                  found.push({ startSec: Math.round(ms / 100) / 10, title: tText.trim() });
                }
              }
            }
          }
        } catch (_) {}
        return found;
      }

      const shortDescription = String(details.shortDescription || '');
      let chapters = extractInitialDataChapters(finalId);
      if (!chapters.length && player && typeof player.getChapters === 'function') {
        try {
          const rawCh = player.getChapters();
          if (Array.isArray(rawCh) && rawCh.length > 0) {
            chapters = rawCh.map((ch, i) => ({
              startSec: Number(ch.time || ch.start_time || 0),
              title: String(ch.title || '').trim() || `Bölüm ${i + 1}`
            }));
          }
        } catch (_) {}
      }
      if (!chapters.length && shortDescription) {
        chapters = parseChaptersFromDescription(shortDescription);
      }

      let activeAudioLang = '';
      try {
        if (player && typeof player.getAudioTrack === 'function') {
          const at = player.getAudioTrack();
          if (at && typeof at === 'object') {
            const rawId = String(at.id || (at.audioTrack && at.audioTrack.id) || at.languageCode || '').trim();
            if (rawId) {
              activeAudioLang = rawId.split('.')[0].trim();
            }
          }
        }
      } catch (_) {}

      return {
        id: finalId,
        title,
        channel,
        duration,
        isLive,
        thumbnail,
        description: shortDescription,
        activeAudioLang,
        formats,
        chapters
      };
    } catch (_) {
      return null;
    }
  }

  function postProbe(reqId) {
    const data = readPlayerData();
    try {
      window.postMessage(
        { channel: 'ruvitube-probe-res-v1', reqId: reqId || 'auto', data },
        ORIGIN
      );
    } catch (_) {}
  }

  window.addEventListener('message', ev => {
    if (ev.source !== window || !ev.data) return;
    if (ev.data.channel === 'ruvitube-probe-req-v1') {
      postProbe(ev.data.reqId);
    }
  });

  for (const evName of ['yt-navigate-finish', 'yt-page-data-updated', 'yt-player-updated']) {
    window.addEventListener(evName, () => {
      setTimeout(() => postProbe('nav'), 120);
    }, { passive: true });
  }
})();
