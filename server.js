import express from 'express';
import cors from 'cors';
import { createServer as createViteServer } from 'vite';

async function startServer() {
  const app = express();
  const PORT = process.env.PORT || 3000;

  app.use(cors());
  app.use(express.json());

  // SSRF and private network block
  function isSafeUrl(rawUrl) {
    try {
      const u = new URL(rawUrl);
      if (u.protocol !== 'http:' && u.protocol !== 'https:') return false;
      const h = u.hostname.toLowerCase();
      if (h === 'localhost' || h === '127.0.0.1' || h === '::1' || h.startsWith('10.') || h.startsWith('192.168.') || h.startsWith('169.254.')) {
        return false;
      }
      return true;
    } catch {
      return false;
    }
  }

  // Secret and token redaction
  function redactUrl(rawUrl) {
    try {
      const u = new URL(rawUrl);
      for (const [k] of u.searchParams) {
        const lk = k.toLowerCase();
        if (['sig', 'signature', 'lsig', 'token', 'key', 'auth', 'secret', 'bui', 'spc', 'xpc', 'ei', 'ip'].some(m => lk.includes(m))) {
          u.searchParams.set(k, 'REDACTED');
        }
      }
      return u.toString();
    } catch {
      return rawUrl;
    }
  }

  // API Routes
  app.post('/api/resolve', async (req, res) => {
    const { url } = req.body;
    if (!url || !isSafeUrl(url)) {
      return res.status(400).json({ ok: false, error: 'Geçersiz veya güvenli olmayan URL.' });
    }

    setTimeout(() => {
      res.json({
        ok: true,
        title: 'Saf Rust Media Stream (RuviTools v2.2.0)',
        sourceUrl: redactUrl(url),
        streams: [
          { id: 'itag-22', kind: 'video', container: 'mp4', resolution: '1280x720', fps: 30, codecs: 'avc1.64001f, mp4a.40.2', bandwidth_bps: 1850000 },
          { id: 'itag-137', kind: 'video', container: 'mp4', resolution: '1920x1080', fps: 30, codecs: 'avc1.640028, mp4a.40.2', bandwidth_bps: 4200000 },
          { id: 'itag-140', kind: 'audio', container: 'm4a', codecs: 'mp4a.40.2', bandwidth_bps: 128000, language: 'tr' }
        ],
        chapters: [
          { title: '01. Giriş & Mimari', startSec: 0, endSec: 45 },
          { title: '02. ruvid & ruvic Motorları', startSec: 45, endSec: 180 },
          { title: '03. Eklenti Entegrasyonu', startSec: 180, endSec: 320 }
        ]
      });
    }, 250);
  });

  app.post('/api/probe', async (req, res) => {
    const { path: filePath } = req.body;
    setTimeout(() => {
      res.json({
        ok: true,
        info: {
          path: filePath || 'sample.mp4',
          container: {
            format_name: 'mov,mp4,m4a,3gp,3g2,mj2',
            format_long_name: 'QuickTime / ISO-BMFF MP4 (Faststart moov atom at front)',
            duration_seconds: 245.8,
            size_bytes: 42891200,
            bit_rate: 1396000
          },
          streams: [
            { index: 0, kind: 'video', codec_name: 'h264', width: 1920, height: 1080, sample_rate: null },
            { index: 1, kind: 'audio', codec_name: 'aac', width: null, height: null, sample_rate: 44100 }
          ]
        }
      });
    }, 200);
  });

  app.post('/api/operation', async (req, res) => {
    const { operation, input, output } = req.body;
    setTimeout(() => {
      res.json({
        ok: true,
        result: {
          operation: operation || 'ConvertVideo',
          output: output || './downloads/output.mp4',
          output_size_bytes: 38400120,
          duration_ms: 980.5
        }
      });
    }, 400);
  });

  app.get('/api/github/release', async (req, res) => {
    const owner = req.query.owner || 'mtahiryilmazz';
    const repo = req.query.repo || 'RuviTools';
    res.json({
      ok: true,
      owner,
      repo,
      currentVersion: '2.4.0',
      latestVersion: '2.4.0',
      updateAvailable: false,
      releaseName: `${repo} v2.4.0 — Robust HTTP 403 Recovery & Native Speed Upgrades`,
      publishedAt: new Date().toISOString(),
      sha256sum: 'f8b2c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b888',
      downloadUrl: `https://github.com/${owner}/${repo}/releases/latest/download/RuviTube-v2.4.0-chrome.zip`,
      releaseNotes: '• HTTP 403 automatic token re-fetch & alternate client rotation\n• Zero-junk temp extraction into %LOCALAPPDATA%\\RuviTube\\extension\n• Enhanced RuviHub marketplace integration'
    });
  });

  app.post('/api/github/check-manifest-version', async (req, res) => {
    const { owner = 'mtahiryilmazz', repo = 'RuviTools', branch = 'main' } = req.body;
    const rawUrl = `https://raw.githubusercontent.com/${owner}/${repo}/${branch}/ruvitube/extension/manifest.json`;
    try {
      const response = await fetch(rawUrl);
      if (response.ok) {
        const manifest = await response.json();
        const remoteVersion = manifest.version || '2.4.0';
        const localVersion = '2.4.0';
        return res.json({
          ok: true,
          source: 'github-raw-manifest',
          manifestUrl: rawUrl,
          remoteVersion,
          localVersion,
          updateAvailable: remoteVersion !== localVersion,
          name: manifest.name,
          description: manifest.description,
          zipUrl: `https://github.com/${owner}/${repo}/archive/refs/heads/${branch}.zip`
        });
      }
    } catch (e) {
      console.error('Failed to fetch remote manifest:', e);
    }

    // Fallback if GitHub Raw unreachable
    res.json({
      ok: true,
      source: 'fallback-local',
      manifestUrl: rawUrl,
      remoteVersion: '2.4.0',
      localVersion: '2.4.0',
      updateAvailable: false,
      name: 'RuviTube',
      description: 'RuviTube Extension (Simulated Raw fallback)',
      zipUrl: `https://github.com/${owner}/${repo}/archive/refs/heads/${branch}.zip`
    });
  });

  app.post('/api/github/check-version', async (req, res) => {
    const { owner = 'mtahiryilmazz', repo = 'RuviTools' } = req.body;
    try {
      // Attempt live GitHub API check
      const ghRes = await fetch(`https://api.github.com/repos/${owner}/${repo}/releases/latest`, {
        headers: { 'User-Agent': 'RuviStudio-Updater' }
      });
      if (ghRes.ok) {
        const data = await ghRes.json();
        return res.json({
          ok: true,
          source: 'github-live',
          tagName: data.tag_name,
          name: data.name,
          publishedAt: data.published_at,
          htmlUrl: data.html_url,
          zipUrl: data.zipball_url || `https://github.com/${owner}/${repo}/releases/download/${data.tag_name}/extension.zip`,
          updateAvailable: data.tag_name !== 'v2.2.0'
        });
      }
    } catch {}

    // Fallback simulated response
    res.json({
      ok: true,
      source: 'simulated',
      tagName: 'v2.3.0',
      name: `${repo} v2.3.0 (RuviHub Release)`,
      publishedAt: new Date().toISOString(),
      htmlUrl: `https://github.com/${owner}/${repo}/releases/tag/v2.3.0`,
      zipUrl: `https://github.com/${owner}/${repo}/releases/download/v2.3.0/RuviTube-v2.3.0.zip`,
      updateAvailable: true,
      sha256: 'f8b2c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b888'
    });
  });

  app.post('/api/github/auto-update', async (req, res) => {
    const { owner = 'mtahiryilmazz', repo = 'RuviTools', version = 'v2.3.0' } = req.body;
    setTimeout(() => {
      res.json({
        ok: true,
        message: `Successfully updated ${repo} to ${version}`,
        tempPath: `%TEMP%\\ruvitube-update-${version}.zip`,
        installPath: `%LOCALAPPDATA%\\RuviTube\\extension`,
        steps: [
          `1. Fetched release asset from https://github.com/${owner}/${repo}/releases/download/${version}/RuviTube.zip`,
          `2. Downloaded and verified SHA-256 checksum in %TEMP% directory`,
          `3. Extracted cleanly and replaced %LOCALAPPDATA%\\RuviTube\\extension`,
          `4. Restarted Chrome Native Messaging host bridge`
        ],
        timestamp: new Date().toISOString()
      });
    }, 800);
  });

  let communityRepos = [
    { id: 1, owner: 'mtahiryilmazz', repo: 'RuviTools', description: 'Saf Rust YouTube Downloader & Chrome Extension', version: 'v2.3.0', stars: 42 },
    { id: 2, owner: 'erkamyaman', repo: 'safe-anything', description: 'Security Hardened Browser Toolkit & Vulnerability scanner', version: 'v1.4.2', stars: 28 },
    { id: 3, owner: 'storytold', repo: 'craft-anything', description: 'Modular Rust & GUI Crafting utilities', version: 'v1.0.5', stars: 19 }
  ];

  app.get('/api/ruvihub/repos', (req, res) => {
    res.json({ ok: true, repos: communityRepos });
  });

  app.post('/api/ruvihub/repos', (req, res) => {
    const { owner, repo, description } = req.body;
    if (!owner || !repo) {
      return res.status(400).json({ ok: false, error: 'Owner ve Repo zorunludur.' });
    }
    const newEntry = {
      id: Date.now(),
      owner,
      repo,
      description: description || 'Community RuviHub Extension',
      version: 'v1.0.0',
      stars: 1
    };
    communityRepos.unshift(newEntry);
    res.json({ ok: true, repo: newEntry });
  });

  let telemetryErrors = [
    { id: 1, timestamp: new Date(Date.now() - 3600000).toISOString(), url: 'https://rr5---sn-ogubqnsz.googlevideo.com/videoplayback', status: 403, error: 'HTTP 403 Forbidden: Signature Expired', resolution: 'Re-fetched signature via alternate Innertube client & resumed range 1248000' },
    { id: 2, timestamp: new Date(Date.now() - 7200000).toISOString(), url: 'https://rr1---sn-ogubqnsz.googlevideo.com/videoplayback', status: 429, error: 'HTTP 429 Too Many Requests: Rate Limited', resolution: 'Switched proxy IP pool & backed off for 1200ms' }
  ];

  app.get('/api/telemetry/errors', (req, res) => {
    res.json({ ok: true, errors: telemetryErrors });
  });



  // Vite middleware for development, static for production
  const vite = await createViteServer({
    server: { middlewareMode: true, host: '0.0.0.0', port: PORT },
    appType: 'spa'
  });
  app.use(vite.middlewares);

  app.listen(PORT, '0.0.0.0', () => {
    console.log(`RuviStudio Web Server running on http://0.0.0.0:${PORT}`);
  });
}

startServer();
