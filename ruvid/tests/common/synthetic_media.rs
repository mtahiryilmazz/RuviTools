#![allow(dead_code, clippy::too_many_arguments)]

/// Pure-Rust synthetic media byte generators and fixture builders.
///
/// Provides valid ISO-BMFF MP4 (faststart, non-faststart, co64, colr HDR, sv3d 360),
/// EBML WebM clusters, ADTS AAC frames, Subtitles (SRT, VTT, SBV, timedtext XML),
/// and Playlists (HLS, DASH MPD, YouTube watch HTML, RSS podcast).
/// Zero external tool dependencies (no ffmpeg, ffprobe, deno, or yt-dlp).

// =========================================================================
// 1. ISO-BMFF MP4 Fixture Builder
// =========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mp4VideoCodec {
    Avc1,
    Vp09,
    Av01,
    Hvc1,
}

impl Mp4VideoCodec {
    pub fn fourcc(&self) -> [u8; 4] {
        match self {
            Self::Avc1 => *b"avc1",
            Self::Vp09 => *b"vp09",
            Self::Av01 => *b"av01",
            Self::Hvc1 => *b"hvc1",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mp4AudioCodec {
    Aac,
    Opus,
    Flac,
}

impl Mp4AudioCodec {
    pub fn fourcc(&self) -> [u8; 4] {
        match self {
            Self::Aac => *b"mp4a",
            Self::Opus => *b"Opus",
            Self::Flac => *b"fLaC",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Mp4FixtureBuilder {
    pub duration_seconds: f64,
    pub video: Option<(Mp4VideoCodec, u32, u32)>,
    pub audio: Option<(Mp4AudioCodec, u32, u16)>,
    pub hdr: bool,
    pub spatial_360: bool,
    pub use_co64: bool,
    pub is_m4a: bool,
    pub mdat_payload: Vec<u8>,
}

impl Default for Mp4FixtureBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Mp4FixtureBuilder {
    pub fn new() -> Self {
        Self {
            duration_seconds: 10.0,
            video: Some((Mp4VideoCodec::Avc1, 1920, 1080)),
            audio: Some((Mp4AudioCodec::Aac, 48000, 2)),
            hdr: false,
            spatial_360: false,
            use_co64: false,
            is_m4a: false,
            mdat_payload: b"RUVID_SYNTHETIC_MEDIA_PAYLOAD_DATA_SAMPLE_CHUNK".to_vec(),
        }
    }

    pub fn video(mut self, width: u32, height: u32) -> Self {
        self.video = Some((Mp4VideoCodec::Avc1, width, height));
        self
    }

    pub fn video_codec(mut self, codec: Mp4VideoCodec, width: u32, height: u32) -> Self {
        self.video = Some((codec, width, height));
        self
    }

    pub fn audio(mut self, sample_rate: u32, channels: u16) -> Self {
        self.audio = Some((Mp4AudioCodec::Aac, sample_rate, channels));
        self
    }

    pub fn audio_codec(mut self, codec: Mp4AudioCodec, sample_rate: u32, channels: u16) -> Self {
        self.audio = Some((codec, sample_rate, channels));
        self
    }

    pub fn video_only(mut self) -> Self {
        self.audio = None;
        self
    }

    pub fn audio_only(mut self) -> Self {
        self.video = None;
        self.is_m4a = true;
        self
    }

    pub fn duration(mut self, duration_seconds: f64) -> Self {
        self.duration_seconds = duration_seconds;
        self
    }

    pub fn hdr_bt2020_pq(mut self) -> Self {
        self.hdr = true;
        self
    }

    pub fn spatial_360(mut self) -> Self {
        self.spatial_360 = true;
        self
    }

    pub fn with_co64(mut self, enable: bool) -> Self {
        self.use_co64 = enable;
        self
    }

    pub fn with_payload(mut self, payload: Vec<u8>) -> Self {
        self.mdat_payload = payload;
        self
    }

    /// Builds a valid Faststart MP4 (`ftyp` + `moov` + `mdat`) with correctly calculated chunk offsets.
    pub fn build_faststart(&self) -> Vec<u8> {
        self.build_internal(true)
    }

    /// Builds a valid Non-Faststart MP4 (`ftyp` + `mdat` + `moov`) where `mdat` precedes `moov`.
    pub fn build_non_faststart(&self) -> Vec<u8> {
        self.build_internal(false)
    }

    pub fn build(&self) -> Vec<u8> {
        self.build_faststart()
    }

    fn build_internal(&self, faststart: bool) -> Vec<u8> {
        let major_brand = if self.is_m4a { b"M4A " } else { b"isom" };
        let mut ftyp_body = Vec::new();
        ftyp_body.extend_from_slice(major_brand);
        ftyp_body.extend_from_slice(&512u32.to_be_bytes());
        ftyp_body.extend_from_slice(b"isomiso2avc1mp41");
        let ftyp = wrap_box(b"ftyp", &ftyp_body);

        let timescale = 1000u32;
        let duration_units = (self.duration_seconds.max(0.1) * f64::from(timescale)).round() as u32;

        let payload = if self.mdat_payload.is_empty() {
            b"RUVID_EMPTY_PAYLOAD"
        } else {
            self.mdat_payload.as_slice()
        };
        let _mdat_box_len = 8 + payload.len();

        // Pass 1: Build moov with dummy offset to calculate exact moov length
        let dummy_offset = 0u64;
        let dummy_moov = self.build_moov(timescale, duration_units, dummy_offset, self.use_co64);
        let moov_len = dummy_moov.len();

        // Calculate true chunk offset
        let true_offset: u64 = if faststart {
            (ftyp.len() + moov_len + 8) as u64
        } else {
            (ftyp.len() + 8) as u64
        };

        // Pass 2: Build final moov with exact offset
        let final_moov = self.build_moov(timescale, duration_units, true_offset, self.use_co64);

        let mdat = wrap_box(b"mdat", payload);

        let mut out = Vec::with_capacity(ftyp.len() + final_moov.len() + mdat.len());
        if faststart {
            out.extend_from_slice(&ftyp);
            out.extend_from_slice(&final_moov);
            out.extend_from_slice(&mdat);
        } else {
            out.extend_from_slice(&ftyp);
            out.extend_from_slice(&mdat);
            out.extend_from_slice(&final_moov);
        }
        out
    }

    fn build_moov(
        &self,
        timescale: u32,
        duration_units: u32,
        chunk_offset: u64,
        use_co64: bool,
    ) -> Vec<u8> {
        let mut next_track_id = 1u32;
        let mut traks = Vec::new();

        if let Some((vcodec, w, h)) = self.video {
            let v_track = build_video_trak(
                next_track_id,
                timescale,
                duration_units,
                w,
                h,
                vcodec.fourcc(),
                self.mdat_payload.len().max(16) as u32,
                chunk_offset,
                use_co64,
                self.hdr,
                self.spatial_360,
            );
            traks.push(v_track);
            next_track_id += 1;
        }

        if let Some((acodec, sr, ch)) = self.audio {
            let a_track = build_audio_trak(
                next_track_id,
                timescale,
                duration_units,
                sr,
                ch,
                acodec.fourcc(),
                self.mdat_payload.len().max(16) as u32,
                chunk_offset,
                use_co64,
            );
            traks.push(a_track);
            next_track_id += 1;
        }

        let mvhd = build_mvhd(timescale, duration_units, next_track_id);
        let mut moov_body = mvhd;
        for t in traks {
            moov_body.extend_from_slice(&t);
        }
        wrap_box(b"moov", &moov_body)
    }
}

pub fn wrap_box(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let total = (8 + body.len()) as u32;
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&total.to_be_bytes());
    out.extend_from_slice(tag);
    out.extend_from_slice(body);
    out
}

fn build_mvhd(timescale: u32, duration: u32, next_track_id: u32) -> Vec<u8> {
    let mut b = vec![0u8; 100];
    b[12..16].copy_from_slice(&timescale.to_be_bytes());
    b[16..20].copy_from_slice(&duration.to_be_bytes());
    b[20..24].copy_from_slice(&0x0001_0000u32.to_be_bytes()); // rate 1.0
    b[24..26].copy_from_slice(&0x0100u16.to_be_bytes()); // volume 1.0
    b[36..40].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    b[52..56].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    b[68..72].copy_from_slice(&0x4000_0000u32.to_be_bytes());
    b[96..100].copy_from_slice(&next_track_id.to_be_bytes());
    wrap_box(b"mvhd", &b)
}

fn build_tkhd(track_id: u32, duration: u32, width: u32, height: u32, is_audio: bool) -> Vec<u8> {
    let mut b = vec![0u8; 84];
    b[3] = 0x07;
    b[12..16].copy_from_slice(&track_id.to_be_bytes());
    b[20..24].copy_from_slice(&duration.to_be_bytes());
    if is_audio {
        b[36..38].copy_from_slice(&0x0100u16.to_be_bytes());
    }
    b[40..44].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    b[56..60].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    b[72..76].copy_from_slice(&0x4000_0000u32.to_be_bytes());
    b[76..80].copy_from_slice(&(width << 16).to_be_bytes());
    b[80..84].copy_from_slice(&(height << 16).to_be_bytes());
    wrap_box(b"tkhd", &b)
}

fn build_mdhd(timescale: u32, duration: u32) -> Vec<u8> {
    let mut b = vec![0u8; 24];
    b[12..16].copy_from_slice(&timescale.to_be_bytes());
    b[16..20].copy_from_slice(&duration.to_be_bytes());
    b[20..22].copy_from_slice(&0x55C4u16.to_be_bytes()); // "und"
    wrap_box(b"mdhd", &b)
}

fn build_hdlr(handler_type: &[u8; 4], name: &[u8]) -> Vec<u8> {
    let mut b = vec![0u8; 24];
    b[8..12].copy_from_slice(handler_type);
    b.extend_from_slice(name);
    b.push(0);
    wrap_box(b"hdlr", &b)
}

fn build_dinf() -> Vec<u8> {
    let url_box = wrap_box(b"url ", &[0, 0, 0, 1]);
    let mut dref_body = vec![0u8; 8];
    dref_body[4..8].copy_from_slice(&1u32.to_be_bytes());
    dref_body.extend_from_slice(&url_box);
    let dref = wrap_box(b"dref", &dref_body);
    wrap_box(b"dinf", &dref)
}

fn build_colr_box() -> Vec<u8> {
    // nclx: colour_primaries 9 (BT.2020), transfer 16 (SMPTE ST 2084 / PQ), matrix 9 (BT.2020 non-const), full_range 1 (80)
    let mut colr = Vec::with_capacity(12);
    colr.extend_from_slice(b"nclx");
    colr.extend_from_slice(&9u16.to_be_bytes());
    colr.extend_from_slice(&16u16.to_be_bytes());
    colr.extend_from_slice(&9u16.to_be_bytes());
    colr.push(0x80);
    wrap_box(b"colr", &colr)
}

fn build_sv3d_box() -> Vec<u8> {
    // sv3d spatial video -> proj -> prhd (equirectangular)
    let prhd = wrap_box(b"prhd", &[0; 24]);
    let equi = wrap_box(b"equi", &[0; 16]);
    let mut proj_body = prhd;
    proj_body.extend_from_slice(&equi);
    let proj = wrap_box(b"proj", &proj_body);
    let svhd = wrap_box(b"svhd", &[0; 12]);
    let mut sv3d_body = svhd;
    sv3d_body.extend_from_slice(&proj);
    wrap_box(b"sv3d", &sv3d_body)
}

fn build_stbl(
    stsd: Vec<u8>,
    duration: u32,
    sample_size: u32,
    chunk_offset: u64,
    use_co64: bool,
) -> Vec<u8> {
    let mut stts = vec![0u8; 16];
    stts[4..8].copy_from_slice(&1u32.to_be_bytes());
    stts[8..12].copy_from_slice(&1u32.to_be_bytes());
    stts[12..16].copy_from_slice(&duration.to_be_bytes());

    let mut stsc = vec![0u8; 20];
    stsc[4..8].copy_from_slice(&1u32.to_be_bytes());
    stsc[8..12].copy_from_slice(&1u32.to_be_bytes());
    stsc[12..16].copy_from_slice(&1u32.to_be_bytes());
    stsc[16..20].copy_from_slice(&1u32.to_be_bytes());

    let mut stsz = vec![0u8; 12];
    stsz[4..8].copy_from_slice(&sample_size.to_be_bytes());
    stsz[8..12].copy_from_slice(&1u32.to_be_bytes());

    let mut stss = vec![0u8; 12];
    stss[4..8].copy_from_slice(&1u32.to_be_bytes()); // 1 entry
    stss[8..12].copy_from_slice(&1u32.to_be_bytes()); // sample 1 is keyframe

    let mut body = stsd;
    body.extend_from_slice(&wrap_box(b"stts", &stts));
    body.extend_from_slice(&wrap_box(b"stsc", &stsc));
    body.extend_from_slice(&wrap_box(b"stsz", &stsz));
    body.extend_from_slice(&wrap_box(b"stss", &stss));

    if use_co64 || chunk_offset > u32::MAX as u64 {
        let mut co64 = vec![0u8; 16];
        co64[4..8].copy_from_slice(&1u32.to_be_bytes());
        co64[8..16].copy_from_slice(&chunk_offset.to_be_bytes());
        body.extend_from_slice(&wrap_box(b"co64", &co64));
    } else {
        let mut stco = vec![0u8; 12];
        stco[4..8].copy_from_slice(&1u32.to_be_bytes());
        stco[8..12].copy_from_slice(&(chunk_offset as u32).to_be_bytes());
        body.extend_from_slice(&wrap_box(b"stco", &stco));
    }

    wrap_box(b"stbl", &body)
}

fn build_video_trak(
    track_id: u32,
    timescale: u32,
    duration: u32,
    width: u32,
    height: u32,
    fourcc: [u8; 4],
    sample_size: u32,
    chunk_offset: u64,
    use_co64: bool,
    hdr: bool,
    spatial_360: bool,
) -> Vec<u8> {
    let tkhd = build_tkhd(track_id, duration, width, height, false);
    let mdhd = build_mdhd(timescale, duration);
    let hdlr = build_hdlr(b"vide", b"VideoHandler");

    let mut entry = vec![0u8; 78];
    entry[6..8].copy_from_slice(&1u16.to_be_bytes());
    entry[24..26].copy_from_slice(&(width as u16).to_be_bytes());
    entry[26..28].copy_from_slice(&(height as u16).to_be_bytes());
    entry[28..32].copy_from_slice(&0x0048_0000u32.to_be_bytes());
    entry[32..36].copy_from_slice(&0x0048_0000u32.to_be_bytes());
    entry[40..42].copy_from_slice(&1u16.to_be_bytes());
    entry[74..76].copy_from_slice(&24u16.to_be_bytes());
    entry[76..78].copy_from_slice(&0xFFFFu16.to_be_bytes());

    if hdr {
        entry.extend_from_slice(&build_colr_box());
    }
    if spatial_360 {
        entry.extend_from_slice(&build_sv3d_box());
    }

    let sample_entry = wrap_box(&fourcc, &entry);
    let mut stsd_body = vec![0u8; 8];
    stsd_body[4..8].copy_from_slice(&1u32.to_be_bytes());
    stsd_body.extend_from_slice(&sample_entry);
    let stsd = wrap_box(b"stsd", &stsd_body);

    let stbl = build_stbl(stsd, duration, sample_size, chunk_offset, use_co64);
    let vmhd = wrap_box(b"vmhd", &[0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
    let dinf = build_dinf();

    let mut minf_body = vmhd;
    minf_body.extend_from_slice(&dinf);
    minf_body.extend_from_slice(&stbl);
    let minf = wrap_box(b"minf", &minf_body);

    let mut mdia_body = mdhd;
    mdia_body.extend_from_slice(&hdlr);
    mdia_body.extend_from_slice(&minf);
    let mdia = wrap_box(b"mdia", &mdia_body);

    let mut trak_body = tkhd;
    trak_body.extend_from_slice(&mdia);
    wrap_box(b"trak", &trak_body)
}

fn build_audio_trak(
    track_id: u32,
    timescale: u32,
    duration: u32,
    sample_rate: u32,
    channels: u16,
    fourcc: [u8; 4],
    sample_size: u32,
    chunk_offset: u64,
    use_co64: bool,
) -> Vec<u8> {
    let tkhd = build_tkhd(track_id, duration, 0, 0, true);
    let mdhd = build_mdhd(timescale, duration);
    let hdlr = build_hdlr(b"soun", b"SoundHandler");

    let mut entry = vec![0u8; 28];
    entry[6..8].copy_from_slice(&1u16.to_be_bytes());
    entry[16..18].copy_from_slice(&channels.to_be_bytes());
    entry[18..20].copy_from_slice(&16u16.to_be_bytes());
    entry[24..28].copy_from_slice(&(sample_rate << 16).to_be_bytes());
    let sample_entry = wrap_box(&fourcc, &entry);

    let mut stsd_body = vec![0u8; 8];
    stsd_body[4..8].copy_from_slice(&1u32.to_be_bytes());
    stsd_body.extend_from_slice(&sample_entry);
    let stsd = wrap_box(b"stsd", &stsd_body);

    let stbl = build_stbl(stsd, duration, sample_size, chunk_offset, use_co64);
    let smhd = wrap_box(b"smhd", &[0u8; 8]);
    let dinf = build_dinf();

    let mut minf_body = smhd;
    minf_body.extend_from_slice(&dinf);
    minf_body.extend_from_slice(&stbl);
    let minf = wrap_box(b"minf", &minf_body);

    let mut mdia_body = mdhd;
    mdia_body.extend_from_slice(&hdlr);
    mdia_body.extend_from_slice(&minf);
    let mdia = wrap_box(b"mdia", &mdia_body);

    let mut trak_body = tkhd;
    trak_body.extend_from_slice(&mdia);
    wrap_box(b"trak", &trak_body)
}

// =========================================================================
// 2. EBML WebM Fixture Builder
// =========================================================================

pub struct WebmFixtureBuilder {
    pub duration_seconds: f64,
    pub width: u32,
    pub height: u32,
    pub sample_rate: f64,
    pub channels: u64,
    pub video_codec: &'static str,
    pub audio_codec: &'static str,
    pub hdr: bool,
    pub spatial_360: bool,
    pub cluster_payload: Vec<u8>,
}

impl Default for WebmFixtureBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl WebmFixtureBuilder {
    pub fn new() -> Self {
        Self {
            duration_seconds: 10.0,
            width: 1920,
            height: 1080,
            sample_rate: 48000.0,
            channels: 2,
            video_codec: "V_VP9",
            audio_codec: "A_OPUS",
            hdr: false,
            spatial_360: false,
            cluster_payload: b"RUVID_WEBM_CLUSTER_PAYLOAD_BLOCK".to_vec(),
        }
    }

    pub fn video(mut self, width: u32, height: u32, codec: &'static str) -> Self {
        self.width = width;
        self.height = height;
        self.video_codec = codec;
        self
    }

    pub fn audio(mut self, sample_rate: f64, channels: u64, codec: &'static str) -> Self {
        self.sample_rate = sample_rate;
        self.channels = channels;
        self.audio_codec = codec;
        self
    }

    pub fn duration(mut self, seconds: f64) -> Self {
        self.duration_seconds = seconds;
        self
    }

    pub fn hdr(mut self) -> Self {
        self.hdr = true;
        self
    }

    pub fn spatial_360(mut self) -> Self {
        self.spatial_360 = true;
        self
    }

    pub fn with_payload(mut self, payload: Vec<u8>) -> Self {
        self.cluster_payload = payload;
        self
    }

    pub fn build(&self) -> Vec<u8> {
        let mut out = Vec::new();

        // 1. EBML Header
        let mut header_body = Vec::new();
        header_body.extend_from_slice(&encode_ebml_string(0x4282, "webm")); // DocType
        header_body.extend_from_slice(&encode_ebml_uint(0x4287, 4)); // DocTypeVersion
        header_body.extend_from_slice(&encode_ebml_uint(0x4285, 2)); // DocTypeReadVersion
        out.extend_from_slice(&encode_ebml_master(0x1A45DFA3, &header_body));

        // 2. Segment
        let mut segment_body = Vec::new();

        // Info
        let mut info_body = Vec::new();
        info_body.extend_from_slice(&encode_ebml_uint(0x2AD7B1, 1_000_000)); // TimestampScale: 1ms
        info_body.extend_from_slice(&encode_ebml_float(0x4489, self.duration_seconds * 1000.0));
        info_body.extend_from_slice(&encode_ebml_string(0x4D80, "ruvid-synthetic"));
        segment_body.extend_from_slice(&encode_ebml_master(0x1549A966, &info_body));

        // Tracks
        let mut tracks_body = Vec::new();

        // Track 1 (Video)
        let mut v_track = Vec::new();
        v_track.extend_from_slice(&encode_ebml_uint(0xD7, 1)); // TrackNumber
        v_track.extend_from_slice(&encode_ebml_uint(0x73C5, 1)); // TrackUID
        v_track.extend_from_slice(&encode_ebml_uint(0x83, 1)); // TrackType = 1 (Video)
        v_track.extend_from_slice(&encode_ebml_string(0x86, self.video_codec)); // CodecID

        let mut video_settings = Vec::new();
        video_settings.extend_from_slice(&encode_ebml_uint(0xB0, self.width as u64));
        video_settings.extend_from_slice(&encode_ebml_uint(0xBA, self.height as u64));

        if self.hdr {
            let mut colour = Vec::new();
            colour.extend_from_slice(&encode_ebml_uint(0x55B1, 9)); // Matrix: BT.2020
            colour.extend_from_slice(&encode_ebml_uint(0x55B2, 10)); // BitsPerChannel: 10
            colour.extend_from_slice(&encode_ebml_uint(0x55B9, 1)); // Range: Full
            colour.extend_from_slice(&encode_ebml_uint(0x55BA, 16)); // Transfer: SMPTE ST 2084
            colour.extend_from_slice(&encode_ebml_uint(0x55BB, 9)); // Primaries: BT.2020
            video_settings.extend_from_slice(&encode_ebml_master(0x55B0, &colour));
        }

        if self.spatial_360 {
            let mut proj = Vec::new();
            proj.extend_from_slice(&encode_ebml_uint(0x7671, 1)); // Equirectangular
            video_settings.extend_from_slice(&encode_ebml_master(0x7670, &proj));
        }

        v_track.extend_from_slice(&encode_ebml_master(0xE0, &video_settings));
        tracks_body.extend_from_slice(&encode_ebml_master(0xAE, &v_track));

        // Track 2 (Audio)
        let mut a_track = Vec::new();
        a_track.extend_from_slice(&encode_ebml_uint(0xD7, 2)); // TrackNumber
        a_track.extend_from_slice(&encode_ebml_uint(0x73C5, 2)); // TrackUID
        a_track.extend_from_slice(&encode_ebml_uint(0x83, 2)); // TrackType = 2 (Audio)
        a_track.extend_from_slice(&encode_ebml_string(0x86, self.audio_codec));

        let mut audio_settings = Vec::new();
        audio_settings.extend_from_slice(&encode_ebml_float(0xB5, self.sample_rate));
        audio_settings.extend_from_slice(&encode_ebml_uint(0x9F, self.channels));
        a_track.extend_from_slice(&encode_ebml_master(0xE1, &audio_settings));
        tracks_body.extend_from_slice(&encode_ebml_master(0xAE, &a_track));

        segment_body.extend_from_slice(&encode_ebml_master(0x1654AE6B, &tracks_body));

        // Cluster
        let mut cluster_body = Vec::new();
        cluster_body.extend_from_slice(&encode_ebml_uint(0xE7, 0)); // Timestamp = 0

        // SimpleBlock for Track 1
        let mut block1 = Vec::new();
        block1.push(0x81); // Track 1
        block1.extend_from_slice(&0i16.to_be_bytes()); // Relative timecode 0
        block1.push(0x80); // Flags: Keyframe
        block1.extend_from_slice(&self.cluster_payload);
        cluster_body.extend_from_slice(&encode_ebml_binary(0xA3, &block1));

        segment_body.extend_from_slice(&encode_ebml_master(0x1F43B675, &cluster_body));

        out.extend_from_slice(&encode_ebml_master(0x18538067, &segment_body));
        out
    }
}

fn encode_vint(val: u64) -> Vec<u8> {
    if val < 0x7F {
        vec![(0x80 | val) as u8]
    } else if val < 0x3FFF {
        let v = 0x4000 | val;
        vec![(v >> 8) as u8, (v & 0xFF) as u8]
    } else if val < 0x1FFFFF {
        let v = 0x200000 | val;
        vec![(v >> 16) as u8, ((v >> 8) & 0xFF) as u8, (v & 0xFF) as u8]
    } else {
        let v = 0x10000000 | (val & 0x0FFFFFFF);
        vec![
            (v >> 24) as u8,
            ((v >> 16) & 0xFF) as u8,
            ((v >> 8) & 0xFF) as u8,
            (v & 0xFF) as u8,
        ]
    }
}

fn encode_ebml_id(id: u32) -> Vec<u8> {
    if id > 0xFFFFFF {
        id.to_be_bytes().to_vec()
    } else if id > 0xFFFF {
        let b = id.to_be_bytes();
        b[1..].to_vec()
    } else if id > 0xFF {
        let b = id.to_be_bytes();
        b[2..].to_vec()
    } else {
        vec![id as u8]
    }
}

fn encode_ebml_master(id: u32, body: &[u8]) -> Vec<u8> {
    let mut out = encode_ebml_id(id);
    out.extend_from_slice(&encode_vint(body.len() as u64));
    out.extend_from_slice(body);
    out
}

fn encode_ebml_uint(id: u32, val: u64) -> Vec<u8> {
    let bytes = if val <= 0xFF {
        vec![val as u8]
    } else if val <= 0xFFFF {
        (val as u16).to_be_bytes().to_vec()
    } else if val <= 0xFFFF_FFFF {
        (val as u32).to_be_bytes().to_vec()
    } else {
        val.to_be_bytes().to_vec()
    };
    let mut out = encode_ebml_id(id);
    out.extend_from_slice(&encode_vint(bytes.len() as u64));
    out.extend_from_slice(&bytes);
    out
}

fn encode_ebml_float(id: u32, val: f64) -> Vec<u8> {
    let bytes = val.to_be_bytes();
    let mut out = encode_ebml_id(id);
    out.extend_from_slice(&encode_vint(bytes.len() as u64));
    out.extend_from_slice(&bytes);
    out
}

fn encode_ebml_string(id: u32, s: &str) -> Vec<u8> {
    let mut out = encode_ebml_id(id);
    out.extend_from_slice(&encode_vint(s.len() as u64));
    out.extend_from_slice(s.as_bytes());
    out
}

fn encode_ebml_binary(id: u32, bytes: &[u8]) -> Vec<u8> {
    let mut out = encode_ebml_id(id);
    out.extend_from_slice(&encode_vint(bytes.len() as u64));
    out.extend_from_slice(bytes);
    out
}

// =========================================================================
// 3. ADTS AAC Frame Fixture Builder
// =========================================================================

pub struct AdtsFixtureBuilder {
    pub sample_rate_index: u8,
    pub channels: u8,
}

impl AdtsFixtureBuilder {
    pub fn new(sample_rate: u32, channels: u8) -> Self {
        let sr_idx = match sample_rate {
            96000 => 0,
            88200 => 1,
            64000 => 2,
            48000 => 3,
            44100 => 4,
            32000 => 5,
            24000 => 6,
            22050 => 7,
            16000 => 8,
            12000 => 9,
            11025 => 10,
            8000 => 11,
            _ => 3,
        };
        Self {
            sample_rate_index: sr_idx,
            channels: channels.clamp(1, 7),
        }
    }

    /// Builds a single valid 7-byte ADTS header frame with raw AAC payload.
    pub fn build_frame(&self, payload_len: usize) -> Vec<u8> {
        let frame_len = 7 + payload_len;
        let mut frame = Vec::with_capacity(frame_len);

        // Byte 0: 0xFF
        frame.push(0xFF);
        // Byte 1: 0xF1 (syncword 0xFFF + ID 0 MPEG-4 + layer 00 + protection absent 1)
        frame.push(0xF1);
        // Byte 2: profile (01 AAC LC) + sample_rate_index + private (0) + channel config msb
        let b2 =
            (0x01 << 6) | ((self.sample_rate_index & 0x0F) << 2) | ((self.channels >> 2) & 0x01);
        frame.push(b2);
        // Byte 3: channels lsb (2 bits) + original (0) + home (0) + copyright (0) + frame_len msb (2 bits)
        let b3 = ((self.channels & 0x03) << 6) | (((frame_len >> 11) & 0x03) as u8);
        frame.push(b3);
        // Byte 4: frame_len middle 8 bits
        frame.push(((frame_len >> 3) & 0xFF) as u8);
        // Byte 5: frame_len low 3 bits + buffer fullness (0x1F)
        let b5 = (((frame_len & 0x07) as u8) << 5) | 0x1F;
        frame.push(b5);
        // Byte 6: buffer fullness low 6 bits (0x3F) + AAC frames in packet (00 = 1 frame)
        frame.push(0xFC);

        // Raw AAC payload bytes
        for i in 0..payload_len {
            frame.push(((i * 7 + 13) % 251) as u8);
        }

        frame
    }

    /// Builds a stream of concatenated ADTS frames.
    pub fn build_stream(&self, num_frames: usize, payload_len_per_frame: usize) -> Vec<u8> {
        let mut stream = Vec::with_capacity(num_frames * (7 + payload_len_per_frame));
        for _ in 0..num_frames {
            stream.extend_from_slice(&self.build_frame(payload_len_per_frame));
        }
        stream
    }
}

// =========================================================================
// 4. Subtitle Fixture Builder
// =========================================================================

#[derive(Debug, Clone)]
pub struct SubtitleCue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

pub struct SubtitleFixtureBuilder {
    pub cues: Vec<SubtitleCue>,
}

impl Default for SubtitleFixtureBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SubtitleFixtureBuilder {
    pub fn new() -> Self {
        Self { cues: Vec::new() }
    }

    pub fn cue(mut self, start_ms: u64, end_ms: u64, text: &str) -> Self {
        self.cues.push(SubtitleCue {
            start_ms,
            end_ms,
            text: text.to_string(),
        });
        self
    }

    pub fn build_srt(&self) -> String {
        let mut s = String::new();
        for (i, cue) in self.cues.iter().enumerate() {
            let start = format_srt_time(cue.start_ms);
            let end = format_srt_time(cue.end_ms);
            s.push_str(&format!(
                "{}\n{} --> {}\n{}\n\n",
                i + 1,
                start,
                end,
                cue.text
            ));
        }
        s
    }

    pub fn build_vtt(&self) -> String {
        let mut s = String::from("WEBVTT\n\n");
        for (i, cue) in self.cues.iter().enumerate() {
            let start = format_vtt_time(cue.start_ms);
            let end = format_vtt_time(cue.end_ms);
            s.push_str(&format!(
                "{}\n{} --> {}\n{}\n\n",
                i + 1,
                start,
                end,
                cue.text
            ));
        }
        s
    }

    pub fn build_sbv(&self) -> String {
        let mut s = String::new();
        for cue in &self.cues {
            let start = format_sbv_time(cue.start_ms);
            let end = format_sbv_time(cue.end_ms);
            s.push_str(&format!("{},{}\n{}\n\n", start, end, cue.text));
        }
        s
    }

    pub fn build_timedtext_xml(&self) -> String {
        let mut xml =
            String::from(r#"<?xml version="1.0" encoding="utf-8" ?><timedtext format="3"><body>"#);
        for cue in &self.cues {
            let dur = cue.end_ms.saturating_sub(cue.start_ms);
            let escaped = cue
                .text
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;");
            xml.push_str(&format!(
                r#"<p t="{}" d="{}">{}</p>"#,
                cue.start_ms, dur, escaped
            ));
        }
        xml.push_str("</body></timedtext>");
        xml
    }
}

fn format_srt_time(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let mins = (ms % 3_600_000) / 60_000;
    let secs = (ms % 60_000) / 1000;
    let millis = ms % 1000;
    format!("{:02}:{:02}:{:02},{:03}", hours, mins, secs, millis)
}

fn format_vtt_time(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let mins = (ms % 3_600_000) / 60_000;
    let secs = (ms % 60_000) / 1000;
    let millis = ms % 1000;
    format!("{:02}:{:02}:{:02}.{:03}", hours, mins, secs, millis)
}

fn format_sbv_time(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let mins = (ms % 3_600_000) / 60_000;
    let secs = (ms % 60_000) / 1000;
    let millis = ms % 1000;
    format!("{}:{:02}:{:02}.{:03}", hours, mins, secs, millis)
}

// =========================================================================
// 5. Playlist Fixture Builder
// =========================================================================

pub struct PlaylistFixtureBuilder;

impl PlaylistFixtureBuilder {
    pub fn build_hls_vod(segment_urls: &[&str], target_duration: u32) -> String {
        let mut m3u = String::from("#EXTM3U\n#EXT-X-VERSION:3\n");
        m3u.push_str(&format!("#EXT-X-TARGETDURATION:{}\n", target_duration));
        m3u.push_str("#EXT-X-MEDIA-SEQUENCE:0\n");
        for url in segment_urls {
            m3u.push_str(&format!("#EXTINF:{}.0,\n{}\n", target_duration, url));
        }
        m3u.push_str("#EXT-X-ENDLIST\n");
        m3u
    }

    pub fn build_dash_mpd(video_url: &str, audio_url: &str, duration_sec: u32) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" minBufferTime="PT1.5S" type="static" mediaPresentationDuration="PT{}S" profiles="urn:mpeg:dash:profile:isoff-on-demand:2011">
  <Period>
    <AdaptationSet id="0" mimeType="video/mp4" contentType="video" subsegmentAlignment="true">
      <Representation id="v4k" bandwidth="15000000" width="3840" height="2160" frameRate="60" codecs="avc1.640033">
        <BaseURL>{}</BaseURL>
      </Representation>
    </AdaptationSet>
    <AdaptationSet id="1" mimeType="audio/mp4" contentType="audio" subsegmentAlignment="true">
      <Representation id="a128" bandwidth="128000" codecs="mp4a.40.2" audioSamplingRate="48000">
        <BaseURL>{}</BaseURL>
      </Representation>
    </AdaptationSet>
  </Period>
</MPD>"#,
            duration_sec, video_url, audio_url
        )
    }

    pub fn build_youtube_watch_html(
        title: &str,
        video_id: &str,
        formats_json: &str,
        adaptive_formats_json: &str,
        caption_tracks_json: &str,
        chapters: &[(&str, f64)],
    ) -> String {
        let mut markers = String::new();
        if !chapters.is_empty() {
            markers.push_str(
                r#","markersMap": [{"key": "DESCRIPTION_CHAPTERS", "value": {"chapters": ["#,
            );
            for (idx, (c_title, c_start)) in chapters.iter().enumerate() {
                if idx > 0 {
                    markers.push(',');
                }
                markers.push_str(&format!(
                    r#"{{"chapterRenderer": {{"title": {{"simpleText": "{}"}}, "timeRangeStartMillis": {}}}}}"#,
                    c_title,
                    (*c_start * 1000.0) as u64
                ));
            }
            markers.push_str(r#"]}}]"#);
        }

        let desc_chapters = chapters
            .iter()
            .map(|(c_title, c_start)| {
                let mins = (*c_start / 60.0).floor() as u64;
                let secs = (*c_start % 60.0) as u64;
                format!("{:02}:{:02} {}", mins, secs, c_title)
            })
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"<!DOCTYPE html><html><head><title>{}</title></head><body>
<script>
var ytInitialPlayerResponse = {{
  "playabilityStatus": {{"status": "OK"}},
  "videoDetails": {{
    "videoId": "{}",
    "title": "{}",
    "lengthSeconds": "600",
    "shortDescription": "{}"
  }},
  "streamingData": {{
    "formats": {},
    "adaptiveFormats": {}
  }},
  "captions": {{
    "playerCaptionsTracklistRenderer": {{
      "captionTracks": {}
    }}
  }}
  {}
}};
</script></body></html>"#,
            title,
            video_id,
            title,
            desc_chapters.replace('\n', "\\n"),
            formats_json,
            adaptive_formats_json,
            caption_tracks_json,
            markers
        )
    }

    pub fn build_podcast_rss(items: &[(&str, &str, usize)]) -> String {
        let mut rss = String::from(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
  <channel>
    <title>Ruvi Synthetic Podcast</title>
    <description>Pure Rust Test Podcast</description>
"#,
        );
        for (idx, (item_title, enclosure_url, length)) in items.iter().enumerate() {
            rss.push_str(&format!(
                r#"    <item>
      <title>{}</title>
      <guid>ep_{}</guid>
      <enclosure url="{}" length="{}" type="audio/mpeg"/>
    </item>
"#,
                item_title,
                idx + 1,
                enclosure_url,
                length
            ));
        }
        rss.push_str("  </channel>\n</rss>");
        rss
    }
}
