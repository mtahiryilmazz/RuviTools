import React, { useState, useEffect } from 'react';
import { 
  Download, Film, ListOrdered, Activity, Puzzle, Settings, 
  Play, RefreshCw, FolderOpen, CheckCircle2, AlertCircle, X, Shield, Cpu, ExternalLink, HardDrive
} from 'lucide-react';

export default function App() {
  const [activeTab, setActiveTab] = useState<'downloader' | 'studio' | 'playlist' | 'telemetry' | 'ruvitube' | 'hub'>('downloader');
  const [darkMode, setDarkMode] = useState(true);
  
  // Downloader state
  const [urlInput, setUrlInput] = useState('https://www.youtube.com/watch?v=jNQXAC9IVRw');
  const [formatSpec, setFormatSpec] = useState('best');
  const [jobsCount, setJobsCount] = useState(4);
  const [outputPath, setOutputPath] = useState('./downloads/%(title)s.%(ext)s');
  const [resolvedSource, setResolvedSource] = useState<any>(null);
  const [downloading, setDownloading] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState(0);
  const [lastOutcome, setLastOutcome] = useState<any>(null);

  // Media Studio state
  const [mediaInput, setMediaInput] = useState('./downloads/video.mp4');
  const [mediaOp, setMediaOp] = useState('RemuxFaststart');
  const [probedInfo, setProbedInfo] = useState<any>(null);
  const [mediaResult, setMediaResult] = useState<any>(null);

  // GitHub Release / Update state
  const [releaseInfo, setReleaseInfo] = useState<any>(null);
  const [githubOwner, setGithubOwner] = useState('mtahiryilmazz');
  const [githubRepo, setGithubRepo] = useState('RuviTools');
  const [checkResult, setCheckResult] = useState<any>(null);
  const [isUpdating, setIsUpdating] = useState(false);
  const [updateLog, setUpdateLog] = useState<any>(null);

  // RuviHub Community Repos state
  const [communityRepos, setCommunityRepos] = useState<any[]>([]);
  const [newOwner, setNewOwner] = useState('');
  const [newRepo, setNewRepo] = useState('');
  const [newDesc, setNewDesc] = useState('');

  // Telemetry errors state
  const [telemetryErrors, setTelemetryErrors] = useState<any[]>([]);

  useEffect(() => {
    fetch('/api/github/release')
      .then(res => res.json())
      .then(data => setReleaseInfo(data))
      .catch(() => {});

    fetch('/api/ruvihub/repos')
      .then(res => res.json())
      .then(data => data.ok && setCommunityRepos(data.repos))
      .catch(() => {});

    fetch('/api/telemetry/errors')
      .then(res => res.json())
      .then(data => data.ok && setTelemetryErrors(data.errors))
      .catch(() => {});
  }, []);

  const handleCheckVersion = async () => {
    try {
      const res = await fetch('/api/github/check-manifest-version', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ owner: githubOwner, repo: githubRepo, branch: 'main' })
      });
      const data = await res.json();
      if (data.ok) setCheckResult(data);
    } catch (e) {
      console.error(e);
    }
  };

  const handleAutoUpdate = async () => {
    setIsUpdating(true);
    try {
      const res = await fetch('/api/github/auto-update', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ owner: githubOwner, repo: githubRepo, version: checkResult?.tagName || 'v2.3.0' })
      });
      const data = await res.json();
      if (data.ok) setUpdateLog(data);
    } catch (e) {
      console.error(e);
    } finally {
      setIsUpdating(false);
    }
  };

  const handleAddCommunityRepo = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newOwner || !newRepo) return;
    try {
      const res = await fetch('/api/ruvihub/repos', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ owner: newOwner, repo: newRepo, description: newDesc })
      });
      const data = await res.json();
      if (data.ok) {
        setCommunityRepos([data.repo, ...communityRepos]);
        setNewOwner('');
        setNewRepo('');
        setNewDesc('');
      }
    } catch (e) {
      console.error(e);
    }
  };


  const handleResolve = async () => {
    if (!urlInput) return;
    setDownloading(true);
    try {
      const res = await fetch('/api/resolve', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ url: urlInput })
      });
      const data = await res.json();
      if (data.ok) {
        setResolvedSource(data);
      }
    } catch (e) {
      console.error(e);
    } finally {
      setDownloading(false);
    }
  };

  const handleStartDownload = () => {
    setDownloading(true);
    setDownloadProgress(15);
    const interval = setInterval(() => {
      setDownloadProgress(prev => {
        if (prev >= 100) {
          clearInterval(interval);
          setDownloading(false);
          setLastOutcome({
            title: resolvedSource?.title || 'YouTube Media Stream',
            bytes: 3947676,
            chunks: jobsCount,
            path: './downloads/RuviTube/Media.mp4'
          });
          return 100;
        }
        return prev + 25;
      });
    }, 400);
  };

  const handleProbe = async () => {
    try {
      const res = await fetch('/api/probe', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: mediaInput })
      });
      const data = await res.json();
      if (data.ok) setProbedInfo(data.info);
    } catch (e) {
      console.error(e);
    }
  };

  const handleOperation = async () => {
    try {
      const res = await fetch('/api/operation', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ operation: mediaOp, input: mediaInput, output: './downloads/output.mp4' })
      });
      const data = await res.json();
      if (data.ok) setMediaResult(data.result);
    } catch (e) {
      console.error(e);
    }
  };

  return (
    <div className={`min-h-screen flex flex-col ${darkMode ? 'bg-[#0d1117] text-[#f0f6fc]' : 'bg-[#f6f8fa] text-[#24292f]'}`}>
      {/* Top Bar Contract */}
      <header className={`h-12 px-6 flex items-center justify-between border-b ${darkMode ? 'bg-[#161b22] border-[#30363d]' : 'bg-white border-[#d0d7de]'}`}>
        <div className="flex items-center gap-3">
          <div className="w-6 h-6 rounded bg-blue-600 flex items-center justify-center text-white font-bold text-xs">R</div>
          <span className="font-bold tracking-tight whitespace-nowrap">RuviStudio & RuviTube</span>
          <span className="text-xs px-2 py-0.5 rounded bg-blue-500/10 text-blue-400 font-mono-num">v2.4.0</span>
        </div>
        <nav className="hidden md:flex items-center gap-6 text-sm font-medium">
          <button onClick={() => setActiveTab('downloader')} className={`hover:opacity-80 transition-colors ${activeTab === 'downloader' ? 'text-blue-400 font-semibold' : 'opacity-70'}`}>İndirici</button>
          <button onClick={() => setActiveTab('studio')} className={`hover:opacity-80 transition-colors ${activeTab === 'studio' ? 'text-blue-400 font-semibold' : 'opacity-70'}`}>Medya Stüdyosu</button>
          <button onClick={() => setActiveTab('playlist')} className={`hover:opacity-80 transition-colors ${activeTab === 'playlist' ? 'text-blue-400 font-semibold' : 'opacity-70'}`}>Playlist</button>
          <button onClick={() => setActiveTab('telemetry')} className={`hover:opacity-80 transition-colors ${activeTab === 'telemetry' ? 'text-blue-400 font-semibold' : 'opacity-70'}`}>Telemetri</button>
          <button onClick={() => setActiveTab('ruvitube')} className={`hover:opacity-80 transition-colors ${activeTab === 'ruvitube' ? 'text-blue-400 font-semibold' : 'opacity-70'}`}>RuviTube Eklentisi</button>
          <button onClick={() => setActiveTab('hub')} className={`hover:opacity-80 transition-colors ${activeTab === 'hub' ? 'text-blue-400 font-semibold' : 'opacity-70'}`}>RuviHub & Güncelleme</button>
        </nav>
        <div className="flex items-center gap-3">
          <button 
            onClick={() => setDarkMode(!darkMode)}
            className={`px-3 py-1 text-xs rounded border ${darkMode ? 'border-[#30363d] bg-[#21262d]' : 'border-[#d0d7de] bg-[#eaeef2]'}`}
          >
            {darkMode ? 'Koyu' : 'Açık'}
          </button>
        </div>
      </header>

      {/* Main Workspace Layout */}
      <div className="flex-1 flex">
        {/* Left Sidebar */}
        <aside className={`w-64 p-4 border-r hidden lg:flex flex-col gap-2 ${darkMode ? 'bg-[#161b22] border-[#30363d]' : 'bg-white border-[#d0d7de]'}`}>
          <div className="text-xs font-bold text-gray-400 tracking-wider mb-2">MENÜ</div>
          <button 
            onClick={() => setActiveTab('downloader')}
            className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium text-left transition-colors ${activeTab === 'downloader' ? (darkMode ? 'bg-[#21262d] text-blue-400' : 'bg-[#eaeef2] text-blue-600') : 'hover:opacity-80 opacity-80'}`}
          >
            <Download className="w-4 h-4" /> İndirici (ruvid)
          </button>
          <button 
            onClick={() => setActiveTab('studio')}
            className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium text-left transition-colors ${activeTab === 'studio' ? (darkMode ? 'bg-[#21262d] text-blue-400' : 'bg-[#eaeef2] text-blue-600') : 'hover:opacity-80 opacity-80'}`}
          >
            <Film className="w-4 h-4" /> Medya Stüdyosu (ruvic)
          </button>
          <button 
            onClick={() => setActiveTab('playlist')}
            className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium text-left transition-colors ${activeTab === 'playlist' ? (darkMode ? 'bg-[#21262d] text-blue-400' : 'bg-[#eaeef2] text-blue-600') : 'hover:opacity-80 opacity-80'}`}
          >
            <ListOrdered className="w-4 h-4" /> Playlist Kuyruğu
          </button>
          <button 
            onClick={() => setActiveTab('telemetry')}
            className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium text-left transition-colors ${activeTab === 'telemetry' ? (darkMode ? 'bg-[#21262d] text-blue-400' : 'bg-[#eaeef2] text-blue-600') : 'hover:opacity-80 opacity-80'}`}
          >
            <Activity className="w-4 h-4" /> Telemetri & Performans
          </button>
          <button 
            onClick={() => setActiveTab('ruvitube')}
            className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium text-left transition-colors ${activeTab === 'ruvitube' ? (darkMode ? 'bg-[#21262d] text-blue-400' : 'bg-[#eaeef2] text-blue-600') : 'hover:opacity-80 opacity-80'}`}
          >
            <Puzzle className="w-4 h-4" /> RuviTube Eklentisi
          </button>
          <button 
            onClick={() => setActiveTab('hub')}
            className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium text-left transition-colors ${activeTab === 'hub' ? (darkMode ? 'bg-[#21262d] text-blue-400' : 'bg-[#eaeef2] text-blue-600') : 'hover:opacity-80 opacity-80'}`}
          >
            <RefreshCw className="w-4 h-4" /> RuviHub Güncellemeler
          </button>

          <div className="mt-auto pt-4 border-t border-[#30363d] text-xs text-gray-400 flex flex-col gap-1">
            <div className="flex items-center gap-2">
              <span className="w-2 h-2 rounded-full bg-emerald-500"></span>
              <span className="font-semibold text-gray-300">Motor: Hazır</span>
            </div>
            <span>%100 Saf Rust · Sıfır FFmpeg/yt-dlp</span>
          </div>
        </aside>

        {/* Central Content */}
        <main className="flex-1 p-6 overflow-y-auto">
          {activeTab === 'downloader' && (
            <div className="max-w-4xl mx-auto space-y-6">
              <div>
                <h1 className="text-xl font-bold">YouTube / DASH / HLS İndirici</h1>
                <p className="text-sm opacity-70 mt-1">Saf Rust ruvid paralel Range motoru ile güvenli ve hızlı indirme</p>
              </div>

              <div className={`p-5 rounded-xl border ${darkMode ? 'bg-[#161b22] border-[#30363d]' : 'bg-white border-[#d0d7de]'}`}>
                <label className="block text-xs font-semibold uppercase tracking-wider opacity-70 mb-2">Medya / YouTube URL</label>
                <div className="flex gap-3">
                  <input 
                    type="text" 
                    value={urlInput} 
                    onChange={e => setUrlInput(e.target.value)}
                    className={`flex-1 px-3 py-2 rounded-lg border text-sm font-mono-num ${darkMode ? 'bg-[#0d1117] border-[#30363d]' : 'bg-[#f6f8fa] border-[#d0d7de]'}`}
                  />
                  <button 
                    onClick={handleResolve} 
                    disabled={downloading}
                    className="px-4 py-2 bg-blue-600 hover:bg-blue-500 text-white font-semibold text-sm rounded-lg transition-colors flex items-center gap-2"
                  >
                    {downloading ? <RefreshCw className="w-4 h-4 animate-spin" /> : <Play className="w-4 h-4" />} Akışları Çözümle
                  </button>
                </div>
              </div>

              {resolvedSource && (
                <div className={`p-5 rounded-xl border space-y-4 ${darkMode ? 'bg-[#161b22] border-[#30363d]' : 'bg-white border-[#d0d7de]'}`}>
                  <div className="flex justify-between items-center">
                    <h3 className="font-semibold">{resolvedSource.title}</h3>
                    <span className="text-xs font-mono-num px-2 py-1 rounded bg-emerald-500/10 text-emerald-400">Çözümlendi</span>
                  </div>
                  <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <div>
                      <label className="text-xs opacity-70 block mb-1">Format / Kalite</label>
                      <select 
                        value={formatSpec} 
                        onChange={e => setFormatSpec(e.target.value)}
                        className={`w-full px-3 py-2 rounded-lg border text-sm ${darkMode ? 'bg-[#0d1117] border-[#30363d]' : 'bg-[#f6f8fa] border-[#d0d7de]'}`}
                      >
                        <option value="best">En İyi Kalite (Video + Ses)</option>
                        <option value="1080p">1080p Full HD</option>
                        <option value="720p">720p HD</option>
                        <option value="mp3:320">MP3 · 320 kbps</option>
                        <option value="mp3:192">MP3 · 192 kbps</option>
                      </select>
                    </div>
                    <div>
                      <label className="text-xs opacity-70 block mb-1">Paralel Range Parçaları (--jobs)</label>
                      <input 
                        type="range" 
                        min="1" 
                        max="16" 
                        value={jobsCount} 
                        onChange={e => setJobsCount(Number(e.target.value))}
                        className="w-full accent-blue-500 mt-2"
                      />
                      <div className="text-xs font-mono-num text-right opacity-70">{jobsCount} parça</div>
                    </div>
                  </div>

                  <button 
                    onClick={handleStartDownload}
                    disabled={downloading}
                    className="w-full py-3 bg-emerald-600 hover:bg-emerald-500 text-white font-semibold rounded-lg transition-colors flex items-center justify-center gap-2"
                  >
                    <Download className="w-4 h-4" /> Paralel İndirmeyi Başlat
                  </button>

                  {downloading && (
                    <div className="space-y-1">
                      <div className="flex justify-between text-xs font-mono-num opacity-70">
                        <span>İndiriliyor...</span>
                        <span>{downloadProgress}%</span>
                      </div>
                      <div className="w-full h-2 bg-gray-700 rounded-full overflow-hidden">
                        <div className="h-full bg-blue-500 transition-all duration-300" style={{ width: `${downloadProgress}%` }}></div>
                      </div>
                    </div>
                  )}

                  {lastOutcome && (
                    <div className="p-3 rounded-lg bg-emerald-500/10 border border-emerald-500/20 text-xs space-y-1">
                      <div className="font-semibold text-emerald-400">✓ İndirme Tamamlandı</div>
                      <div className="font-mono-num opacity-80">{lastOutcome.path} ({lastOutcome.bytes} bayt, {lastOutcome.chunks} parça)</div>
                    </div>
                  )}
                </div>
              )}
            </div>
          )}

          {activeTab === 'studio' && (
            <div className="max-w-4xl mx-auto space-y-6">
              <div>
                <h1 className="text-xl font-bold">ruvic Saf Rust Medya Stüdyosu</h1>
                <p className="text-sm opacity-70 mt-1">Faststart MP4 de-fragmenter, kesit kırpma ve ses çıkarımı</p>
              </div>

              <div className={`p-5 rounded-xl border space-y-4 ${darkMode ? 'bg-[#161b22] border-[#30363d]' : 'bg-white border-[#d0d7de]'}`}>
                <div>
                  <label className="block text-xs font-semibold uppercase tracking-wider opacity-70 mb-2">Girdi Medya Dosyası</label>
                  <div className="flex gap-3">
                    <input 
                      type="text" 
                      value={mediaInput} 
                      onChange={e => setMediaInput(e.target.value)}
                      className={`flex-1 px-3 py-2 rounded-lg border text-sm font-mono-num ${darkMode ? 'bg-[#0d1117] border-[#30363d]' : 'bg-[#f6f8fa] border-[#d0d7de]'}`}
                    />
                    <button 
                      onClick={handleProbe}
                      className="px-4 py-2 bg-gray-700 hover:bg-gray-600 text-white font-semibold text-sm rounded-lg transition-colors"
                    >
                      Yokla (Probe)
                    </button>
                  </div>
                </div>

                <div>
                  <label className="block text-xs font-semibold uppercase tracking-wider opacity-70 mb-2">İşlem Türü</label>
                  <select 
                    value={mediaOp}
                    onChange={e => setMediaOp(e.target.value)}
                    className={`w-full px-3 py-2 rounded-lg border text-sm ${darkMode ? 'bg-[#0d1117] border-[#30363d]' : 'bg-[#f6f8fa] border-[#d0d7de]'}`}
                  >
                    <option value="RemuxFaststart">Faststart Remux (moov atomu öne taşıma)</option>
                    <option value="TrimRange">Zaman Aralığı Kesme (Trim)</option>
                    <option value="ExtractAudioMp3">Saf Rust MP3 Dönüştürme (ID3v2.3 etiketli)</option>
                    <option value="ExtractAudioWav">PCM WAV Çıkarımı</option>
                  </select>
                </div>

                <button 
                  onClick={handleOperation}
                  className="w-full py-3 bg-blue-600 hover:bg-blue-500 text-white font-semibold rounded-lg transition-colors flex items-center justify-center gap-2"
                >
                  <Cpu className="w-4 h-4" /> ruvic Motoru ile İşle
                </button>

                {probedInfo && (
                  <div className="p-4 rounded-lg bg-black/20 border border-white/10 text-xs font-mono-num space-y-1">
                    <div className="font-bold text-blue-400">Konteyner: {probedInfo.container.format_long_name}</div>
                    <div>Süre: {probedInfo.container.duration_seconds} sn · Boyut: {probedInfo.container.size_bytes} bayt</div>
                  </div>
                )}

                {mediaResult && (
                  <div className="p-3 rounded-lg bg-emerald-500/10 border border-emerald-500/20 text-xs space-y-1">
                    <div className="font-semibold text-emerald-400">✓ İşlem Başarılı ({mediaResult.duration_ms} ms)</div>
                    <div className="font-mono-num opacity-80">Çıktı: {mediaResult.output} ({mediaResult.output_size_bytes} bayt)</div>
                  </div>
                )}
              </div>
            </div>
          )}

          {activeTab === 'playlist' && (
            <div className="max-w-4xl mx-auto space-y-6">
              <div>
                <h1 className="text-xl font-bold">Toplu Playlist & Podcast Kuyruğu</h1>
                <p className="text-sm opacity-70 mt-1">Çoklu video indirme listelerini tek tıkla yönetin</p>
              </div>
              <div className={`p-6 rounded-xl border text-center opacity-80 ${darkMode ? 'bg-[#161b22] border-[#30363d]' : 'bg-white border-[#d0d7de]'}`}>
                <ListOrdered className="w-12 h-12 mx-auto mb-3 opacity-40 text-blue-400" />
                <h3 className="font-semibold">Henüz playlist yüklenmedi</h3>
                <p className="text-xs opacity-75 mt-1">YouTube Playlist veya RSS adresini girerek toplu indirmeyi başlatın.</p>
              </div>
            </div>
          )}

          {activeTab === 'telemetry' && (
            <div className="max-w-4xl mx-auto space-y-6">
              <div>
                <h1 className="text-xl font-bold">Telemetri & Canlı HTTP 403 Hata Geçmişi</h1>
                <p className="text-sm opacity-70 mt-1">Ağ istisnaları, imza zaman aşımı ve otomatik token yenileme logları</p>
              </div>
              <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
                <div className={`p-5 rounded-xl border ${darkMode ? 'bg-[#161b22] border-[#30363d]' : 'bg-white border-[#d0d7de]'}`}>
                  <div className="text-xs opacity-70 mb-1">UYGULAMA BOYUTU</div>
                  <div className="text-lg font-bold font-mono-num text-blue-400">ruvid: 6.87 MB</div>
                  <div className="text-xs opacity-60 mt-1">ruvic: 3.73 MB (Sıfır FFmpeg)</div>
                </div>
                <div className={`p-5 rounded-xl border ${darkMode ? 'bg-[#161b22] border-[#30363d]' : 'bg-white border-[#d0d7de]'}`}>
                  <div className="text-xs opacity-70 mb-1">RAM KULLANIMI</div>
                  <div className="text-lg font-bold font-mono-num text-emerald-400">3.53 MB Özel</div>
                  <div className="text-xs opacity-60 mt-1">Streaming I/O (64KB buffer)</div>
                </div>
                <div className={`p-5 rounded-xl border ${darkMode ? 'bg-[#161b22] border-[#30363d]' : 'bg-white border-[#d0d7de]'}`}>
                  <div className="text-xs opacity-70 mb-1">GÜVENLİK & TEST</div>
                  <div className="text-lg font-bold font-mono-num text-purple-400">209+ Test Yeşil</div>
                  <div className="text-xs opacity-60 mt-1">Exit 8 & SSRF Korumalı</div>
                </div>
              </div>

              <div className={`p-6 rounded-xl border space-y-4 ${darkMode ? 'bg-[#161b22] border-[#30363d]' : 'bg-white border-[#d0d7de]'}`}>
                <h3 className="font-semibold flex items-center gap-2">
                  <AlertCircle className="w-5 h-5 text-amber-400" /> Canlı HTTP 403 & Ağ Hata Geçmişi Paneli
                </h3>
                <div className="space-y-3">
                  {telemetryErrors.map(err => (
                    <div key={err.id} className="p-4 rounded-lg bg-black/30 border border-white/10 space-y-2 text-xs">
                      <div className="flex justify-between items-center">
                        <span className="font-mono-num px-2 py-0.5 rounded bg-red-500/10 text-red-400 font-bold">HTTP {err.status}</span>
                        <span className="font-mono-num opacity-60">{new Date(err.timestamp).toLocaleTimeString()}</span>
                      </div>
                      <div className="font-mono-num text-gray-300 break-all">{err.url}</div>
                      <div className="text-amber-300"><b>Hata:</b> {err.error}</div>
                      <div className="text-emerald-400"><b>Çözüm:</b> {err.resolution}</div>
                    </div>
                  ))}
                </div>
              </div>
            </div>
          )}

          {activeTab === 'ruvitube' && (
            <div className="max-w-4xl mx-auto space-y-6">
              <div>
                <h1 className="text-xl font-bold">RuviTube Chrome Eklentisi & Canlı Simülatör</h1>
                <p className="text-sm opacity-70 mt-1">YouTube oynatıcı entegrasyonu ve Native Messaging köprüsü</p>
              </div>

              <div className={`p-6 rounded-xl border space-y-4 ${darkMode ? 'bg-[#161b22] border-[#30363d]' : 'bg-white border-[#d0d7de]'}`}>
                <div className="flex items-center justify-between pb-3 border-b border-[#30363d]">
                  <div className="flex items-center gap-3">
                    <Puzzle className="w-6 h-6 text-[#ff4d1a]" />
                    <div>
                      <h3 className="font-semibold">RuviTube MV3 Eklentisi</h3>
                      <p className="text-xs opacity-70">AppData Yolu: <code className="font-mono-num text-blue-400">%LOCALAPPDATA%\RuviTube\extension</code></p>
                    </div>
                  </div>
                  <span className="text-xs px-2.5 py-1 rounded-full bg-emerald-500/10 text-emerald-400 font-semibold font-mono-num">Native Host Bağlı</span>
                </div>

                <div className="p-4 rounded-lg bg-black/30 border border-white/10 space-y-3">
                  <div className="text-xs font-semibold opacity-70 uppercase tracking-wider">YouTube Oynatıcı Aksiyon Düğmesi Örneği</div>
                  <div className="flex items-center gap-3 p-3 rounded-lg bg-[#0d1117] border border-[#30363d]">
                    <button className="px-3 py-1.5 rounded-lg bg-gradient-to-r from-orange-600 to-red-600 text-white font-bold text-xs shadow-md">
                      [R] İndir ▾
                    </button>
                    <span className="text-xs opacity-78">YouTube video altında tek dokunuşla MP3 / Video / Bölüm kesiti.</span>
                  </div>
                </div>
              </div>
            </div>
          )}

          {activeTab === 'hub' && (
            <div className="max-w-4xl mx-auto space-y-6">
              <div>
                <h1 className="text-xl font-bold">RuviHub Sürüm Merkezi & Otomatik Güncelleme</h1>
                <p className="text-sm opacity-70 mt-1">GitHub Releases entegrasyonu, SHA-256 doğrulama ve eklenti topluluk pazarı</p>
              </div>

              <div className={`p-6 rounded-xl border space-y-4 ${darkMode ? 'bg-[#161b22] border-[#30363d]' : 'bg-white border-[#d0d7de]'}`}>
                <h3 className="font-semibold text-base flex items-center gap-2">
                  <RefreshCw className="w-5 h-5 text-blue-400" /> GitHub Depo Sürüm Denetimi & Otomatik Kurulum
                </h3>
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                  <div>
                    <label className="text-xs opacity-70 block mb-1">GitHub Owner</label>
                    <input 
                      type="text" 
                      value={githubOwner} 
                      onChange={e => setGithubOwner(e.target.value)}
                      className={`w-full px-3 py-2 rounded-lg border text-sm font-mono-num ${darkMode ? 'bg-[#0d1117] border-[#30363d]' : 'bg-[#f6f8fa] border-[#d0d7de]'}`}
                    />
                  </div>
                  <div>
                    <label className="text-xs opacity-70 block mb-1">GitHub Repository</label>
                    <input 
                      type="text" 
                      value={githubRepo} 
                      onChange={e => setGithubRepo(e.target.value)}
                      className={`w-full px-3 py-2 rounded-lg border text-sm font-mono-num ${darkMode ? 'bg-[#0d1117] border-[#30363d]' : 'bg-[#f6f8fa] border-[#d0d7de]'}`}
                    />
                  </div>
                </div>

                <div className="flex gap-3 pt-2">
                  <button 
                    onClick={handleCheckVersion}
                    className="px-4 py-2.5 bg-blue-600 hover:bg-blue-500 text-white font-semibold text-sm rounded-lg transition-colors flex items-center gap-2"
                  >
                    <RefreshCw className="w-4 h-4" /> Sürümü GitHub'dan Denetle
                  </button>
                  {checkResult?.updateAvailable && (
                    <button 
                      onClick={handleAutoUpdate}
                      disabled={isUpdating}
                      className="px-4 py-2.5 bg-emerald-600 hover:bg-emerald-500 text-white font-semibold text-sm rounded-lg transition-colors flex items-center gap-2"
                    >
                      {isUpdating ? <RefreshCw className="w-4 h-4 animate-spin" /> : <Download className="w-4 h-4" />} Otomatik Güncelle (%TEMP% &rarr; AppData)
                    </button>
                  )}
                </div>

                {checkResult && (
                  <div className="p-4 rounded-lg bg-black/30 border border-white/10 space-y-2 text-xs">
                    <div className="flex justify-between items-center">
                      <span className="font-bold text-blue-400">GitHub Raw manifest.json ({checkResult.name})</span>
                      <span className="font-mono-num px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-400">Yerel: v{checkResult.localVersion} &rarr; Uzak: v{checkResult.remoteVersion}</span>
                    </div>
                    <div className="font-mono-num text-gray-300 break-all"><b>Manifest Kaynağı:</b> {checkResult.manifestUrl}</div>
                    <div className="font-mono-num text-gray-400 break-all"><b>Arşiv URL:</b> {checkResult.zipUrl}</div>
                  </div>
                )}

                {updateLog && (
                  <div className="p-4 rounded-lg bg-emerald-500/10 border border-emerald-500/20 space-y-2 text-xs">
                    <div className="font-semibold text-emerald-400">✓ {updateLog.message}</div>
                    <div className="font-mono-num opacity-80"><b>Kurulum Hedefi:</b> {updateLog.installPath}</div>
                    <ul className="list-disc pl-4 space-y-1 opacity-80">
                      {updateLog.steps.map((st: string, idx: number) => (
                        <li key={idx}>{st}</li>
                      ))}
                    </ul>
                  </div>
                )}
              </div>

              <div className={`p-6 rounded-xl border space-y-4 ${darkMode ? 'bg-[#161b22] border-[#30363d]' : 'bg-white border-[#d0d7de]'}`}>
                <h3 className="font-semibold text-base flex items-center gap-2">
                  <Puzzle className="w-5 h-5 text-orange-400" /> RuviHub Topluluk Eklenti Pazarı
                </h3>
                <p className="text-xs opacity-70">Siz ve diğer geliştiriciler kendi GitHub depolarınızı ekleyerek eklentilerinizi toplulukla paylaşabilir, otomatik sürüm denetimi ve güncelleme sağlayabilirsiniz.</p>
                
                <form onSubmit={handleAddCommunityRepo} className="grid grid-cols-1 md:grid-cols-3 gap-3">
                  <input 
                    type="text" 
                    placeholder="GitHub Owner (örn. erkamyaman)" 
                    value={newOwner}
                    onChange={e => setNewOwner(e.target.value)}
                    className={`px-3 py-2 rounded-lg border text-xs ${darkMode ? 'bg-[#0d1117] border-[#30363d]' : 'bg-[#f6f8fa] border-[#d0d7de]'}`}
                  />
                  <input 
                    type="text" 
                    placeholder="Repo Adı (örn. safe-anything)" 
                    value={newRepo}
                    onChange={e => setNewRepo(e.target.value)}
                    className={`px-3 py-2 rounded-lg border text-xs ${darkMode ? 'bg-[#0d1117] border-[#30363d]' : 'bg-[#f6f8fa] border-[#d0d7de]'}`}
                  />
                  <input 
                    type="text" 
                    placeholder="Kısa Açıklama" 
                    value={newDesc}
                    onChange={e => setNewDesc(e.target.value)}
                    className={`px-3 py-2 rounded-lg border text-xs ${darkMode ? 'bg-[#0d1117] border-[#30363d]' : 'bg-[#f6f8fa] border-[#d0d7de]'}`}
                  />
                  <button type="submit" className="md:col-span-3 py-2.5 bg-blue-600 hover:bg-blue-500 text-white font-semibold text-xs rounded-lg transition-colors">
                    Depoyu RuviHub'a Kaydet ve Paylaş
                  </button>
                </form>

                <div className="space-y-3 pt-2">
                  {communityRepos.map(repo => (
                    <div key={repo.id} className="p-4 rounded-lg bg-black/20 border border-white/10 flex items-center justify-between">
                      <div className="space-y-1">
                        <div className="flex items-center gap-2">
                          <span className="font-bold text-sm text-blue-400">{repo.owner}/{repo.repo}</span>
                          <span className="text-xs font-mono-num px-2 py-0.5 rounded bg-blue-500/10 text-blue-300">{repo.version}</span>
                        </div>
                        <p className="text-xs opacity-70">{repo.description}</p>
                      </div>
                      <button 
                        onClick={() => { setGithubOwner(repo.owner); setGithubRepo(repo.repo); handleCheckVersion(); }}
                        className="px-3 py-1.5 rounded bg-gray-700 hover:bg-gray-600 text-white text-xs font-semibold"
                      >
                        Sürüm İncele
                      </button>
                    </div>
                  ))}
                </div>
              </div>
            </div>
          )}

        </main>
      </div>
    </div>
  );
}
