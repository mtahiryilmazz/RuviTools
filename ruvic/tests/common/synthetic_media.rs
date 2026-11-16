//! Pure-Rust Synthetic Media Byte Generators and Test Fixtures.
//!
//! Provides deterministic byte-level ISO-BMFF MP4, EBML WebM, and ADTS AAC fixtures
//! without external dependencies or wrappers (100% pure Rust).

#![allow(dead_code)]

use std::path::Path;

// ============================================================================
// ISO-BMFF (MP4) Low-Level Box Builders
// ============================================================================

/// Wraps a 4-byte box tag and payload into a standard 32-bit ISO-BMFF box:
/// `[4-byte size (BE)][4-byte tag][payload]`
pub fn wrap_box(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let size = (payload.len() + 8) as u32;
    let mut out = Vec::with_capacity(size as usize);
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(tag);
    out.extend_from_slice(payload);
    out
}

/// Wraps a 4-byte box tag and payload into a 64-bit large ISO-BMFF box:
/// `[size=1 (4B)][tag (4B)][64-bit size (8B)][payload]`
pub fn wrap_box_64(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let size = (payload.len() + 16) as u64;
    let mut out = Vec::with_capacity(payload.len() + 16);
    out.extend_from_slice(&1u32.to_be_bytes());
    out.extend_from_slice(tag);
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// Builds an ISO-BMFF `ftyp` (File Type) box.
pub fn build_ftyp(major_brand: &[u8; 4], minor_version: u32, compatible: &[&[u8; 4]]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(major_brand);
    payload.extend_from_slice(&minor_version.to_be_bytes());
    for b in compatible {
        payload.extend_from_slice(*b);
    }
    wrap_box(b"ftyp", &payload)
}

/// Builds an ISO-BMFF `mvhd` (Movie Header) box (version 0).
pub fn build_mvhd(timescale: u32, duration: u32, next_track_id: u32) -> Vec<u8> {
    let mut payload = vec![0u8; 100]; // version(1) + flags(3) + ctime(4) + mtime(4) + timescale(4) + duration(4) + rate(4) + vol(2) + res(10) + matrix(36) + pre(24) + next_track_id(4)
    payload[12..16].copy_from_slice(&timescale.to_be_bytes());
    payload[16..20].copy_from_slice(&duration.to_be_bytes());
    payload[20..24].copy_from_slice(&0x0001_0000u32.to_be_bytes()); // rate 1.0
    payload[24..26].copy_from_slice(&0x0100u16.to_be_bytes()); // volume 1.0
    // Unity matrix at index 36..72 (36 bytes): 0x00010000, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000
    payload[36..40].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    payload[52..56].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    payload[68..72].copy_from_slice(&0x4000_0000u32.to_be_bytes());
    payload[96..100].copy_from_slice(&next_track_id.to_be_bytes());
    wrap_box(b"mvhd", &payload)
}

/// Builds an ISO-BMFF `tkhd` (Track Header) box (version 0).
pub fn build_tkhd(
    track_id: u32,
    duration: u32,
    width: u32,
    height: u32,
    is_audio: bool,
) -> Vec<u8> {
    let mut payload = vec![0u8; 84];
    payload[3] = 0x07; // flags: enabled, in_movie, in_preview
    payload[12..16].copy_from_slice(&track_id.to_be_bytes());
    payload[20..24].copy_from_slice(&duration.to_be_bytes());
    if is_audio {
        payload[36..38].copy_from_slice(&0x0100u16.to_be_bytes()); // volume 1.0
    }
    // Unity matrix at 40..76
    payload[40..44].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    payload[56..60].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    payload[72..76].copy_from_slice(&0x4000_0000u32.to_be_bytes());
    // width & height as 16.16 fixed point
    payload[76..80].copy_from_slice(&(width << 16).to_be_bytes());
    payload[80..84].copy_from_slice(&(height << 16).to_be_bytes());
    wrap_box(b"tkhd", &payload)
}

/// Builds an ISO-BMFF `mdhd` (Media Header) box.
pub fn build_mdhd(timescale: u32, duration: u32) -> Vec<u8> {
    let mut payload = vec![0u8; 24];
    payload[12..16].copy_from_slice(&timescale.to_be_bytes());
    payload[16..20].copy_from_slice(&duration.to_be_bytes());
    payload[20..22].copy_from_slice(&0x55C4u16.to_be_bytes()); // 'und' language
    wrap_box(b"mdhd", &payload)
}

/// Builds an ISO-BMFF `hdlr` (Handler Reference) box.
pub fn build_hdlr(handler_type: &[u8; 4], name: &str) -> Vec<u8> {
    let mut payload = vec![0u8; 24]; // version(1)+flags(3)+pre_defined(4)+handler(4)+reserved(12)
    payload[8..12].copy_from_slice(handler_type);
    payload.extend_from_slice(name.as_bytes());
    payload.push(0); // null terminator
    wrap_box(b"hdlr", &payload)
}

/// Builds `vmhd` (Video Media Header).
pub fn build_vmhd() -> Vec<u8> {
    let mut payload = vec![0u8; 12];
    payload[3] = 0x01; // flags
    wrap_box(b"vmhd", &payload)
}

/// Builds `smhd` (Sound Media Header).
pub fn build_smhd() -> Vec<u8> {
    let payload = vec![0u8; 8];
    wrap_box(b"smhd", &payload)
}

/// Builds `dinf` containing a single `dref` with a local `url ` entry.
pub fn build_dinf() -> Vec<u8> {
    let url_box = wrap_box(b"url ", &[0, 0, 0, 1]); // self-contained flag
    let mut dref_payload = vec![0u8; 8];
    dref_payload[4..8].copy_from_slice(&1u32.to_be_bytes()); // 1 entry
    dref_payload.extend_from_slice(&url_box);
    let dref_box = wrap_box(b"dref", &dref_payload);
    wrap_box(b"dinf", &dref_box)
}

/// Builds `stsd` containing a visual sample entry (`avc1`, `hvc1`, `vp09`, `av01`)
/// with optional child boxes like `colr`, `sv3d`, `mdcv`, `clli`.
pub fn build_stsd_video(
    codec_fourcc: &[u8; 4],
    width: u16,
    height: u16,
    extra_boxes: &[u8],
) -> Vec<u8> {
    let mut entry = vec![0u8; 78];
    entry[6..8].copy_from_slice(&1u16.to_be_bytes()); // data_reference_index
    entry[24..26].copy_from_slice(&width.to_be_bytes());
    entry[26..28].copy_from_slice(&height.to_be_bytes());
    entry[28..32].copy_from_slice(&0x0048_0000u32.to_be_bytes()); // horiz res 72 dpi
    entry[32..36].copy_from_slice(&0x0048_0000u32.to_be_bytes()); // vert res 72 dpi
    entry[40..42].copy_from_slice(&1u16.to_be_bytes()); // frame_count = 1
    entry[74..76].copy_from_slice(&0x0018u16.to_be_bytes()); // depth 24
    entry[76..78].copy_from_slice(&0xFFFFu16.to_be_bytes()); // pre_defined = -1
    entry.extend_from_slice(extra_boxes);
    let sample_entry = wrap_box(codec_fourcc, &entry);

    let mut stsd_payload = vec![0u8; 8];
    stsd_payload[4..8].copy_from_slice(&1u32.to_be_bytes());
    stsd_payload.extend_from_slice(&sample_entry);
    wrap_box(b"stsd", &stsd_payload)
}

/// Builds `stsd` containing an audio sample entry (`mp4a`, `Opus`, `fLaC`).
pub fn build_stsd_audio(codec_fourcc: &[u8; 4], sample_rate: u32, channels: u16) -> Vec<u8> {
    let mut entry = vec![0u8; 28];
    entry[6..8].copy_from_slice(&1u16.to_be_bytes()); // data_reference_index
    entry[16..18].copy_from_slice(&channels.to_be_bytes());
    entry[18..20].copy_from_slice(&16u16.to_be_bytes()); // sample_size 16 bits
    entry[24..28].copy_from_slice(&(sample_rate << 16).to_be_bytes()); // sample_rate 16.16
    let sample_entry = wrap_box(codec_fourcc, &entry);

    let mut stsd_payload = vec![0u8; 8];
    stsd_payload[4..8].copy_from_slice(&1u32.to_be_bytes());
    stsd_payload.extend_from_slice(&sample_entry);
    wrap_box(b"stsd", &stsd_payload)
}

/// Builds `stts` (Time to Sample) box.
pub fn build_stts(sample_count: u32, sample_delta: u32) -> Vec<u8> {
    let mut payload = vec![0u8; 16];
    payload[4..8].copy_from_slice(&1u32.to_be_bytes()); // 1 entry
    payload[8..12].copy_from_slice(&sample_count.to_be_bytes());
    payload[12..16].copy_from_slice(&sample_delta.to_be_bytes());
    wrap_box(b"stts", &payload)
}

/// Builds `stsc` (Sample to Chunk) box.
pub fn build_stsc(entries: &[(u32, u32, u32)]) -> Vec<u8> {
    let mut payload = vec![0u8; 8];
    payload[4..8].copy_from_slice(&(entries.len() as u32).to_be_bytes());
    for (first_chunk, samples_per_chunk, desc_index) in entries {
        payload.extend_from_slice(&first_chunk.to_be_bytes());
        payload.extend_from_slice(&samples_per_chunk.to_be_bytes());
        payload.extend_from_slice(&desc_index.to_be_bytes());
    }
    wrap_box(b"stsc", &payload)
}

/// Builds `stsz` (Sample Size) box.
pub fn build_stsz(uniform_size: u32, sample_count: u32, individual_sizes: &[u32]) -> Vec<u8> {
    let mut payload = vec![0u8; 12];
    payload[4..8].copy_from_slice(&uniform_size.to_be_bytes());
    payload[8..12].copy_from_slice(&sample_count.to_be_bytes());
    if uniform_size == 0 {
        for s in individual_sizes {
            payload.extend_from_slice(&s.to_be_bytes());
        }
    }
    wrap_box(b"stsz", &payload)
}

/// Builds 32-bit `stco` (Chunk Offset) box.
pub fn build_stco(offsets: &[u32]) -> Vec<u8> {
    let mut payload = vec![0u8; 8];
    payload[4..8].copy_from_slice(&(offsets.len() as u32).to_be_bytes());
    for &off in offsets {
        payload.extend_from_slice(&off.to_be_bytes());
    }
    wrap_box(b"stco", &payload)
}

/// Builds 64-bit `co64` (Chunk Large Offset) box.
pub fn build_co64(offsets: &[u64]) -> Vec<u8> {
    let mut payload = vec![0u8; 8];
    payload[4..8].copy_from_slice(&(offsets.len() as u32).to_be_bytes());
    for &off in offsets {
        payload.extend_from_slice(&off.to_be_bytes());
    }
    wrap_box(b"co64", &payload)
}

/// Builds `stss` (Sync Sample / Keyframe) box.
pub fn build_stss(sync_sample_numbers: &[u32]) -> Vec<u8> {
    let mut payload = vec![0u8; 8];
    payload[4..8].copy_from_slice(&(sync_sample_numbers.len() as u32).to_be_bytes());
    for &num in sync_sample_numbers {
        payload.extend_from_slice(&num.to_be_bytes());
    }
    wrap_box(b"stss", &payload)
}

/// Builds an ISO-BMFF `colr` NCLX box.
/// E.g. BT.2020 primaries=9, PQ transfer=16, matrix=9, full_range=true.
pub fn build_colr_nclx(primaries: u16, transfer: u16, matrix: u16, full_range: bool) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(b"nclx");
    payload.extend_from_slice(&primaries.to_be_bytes());
    payload.extend_from_slice(&transfer.to_be_bytes());
    payload.extend_from_slice(&matrix.to_be_bytes());
    payload.push(if full_range { 0x80 } else { 0x00 });
    wrap_box(b"colr", &payload)
}

/// Builds an ISO-BMFF `mdcv` (Mastering Display Colour Volume) box.
pub fn build_mdcv(
    primaries_green: (u16, u16),
    primaries_blue: (u16, u16),
    primaries_red: (u16, u16),
    white_point: (u16, u16),
    max_luminance: u32,
    min_luminance: u32,
) -> Vec<u8> {
    let mut payload = Vec::with_capacity(24);
    payload.extend_from_slice(&primaries_green.0.to_be_bytes());
    payload.extend_from_slice(&primaries_green.1.to_be_bytes());
    payload.extend_from_slice(&primaries_blue.0.to_be_bytes());
    payload.extend_from_slice(&primaries_blue.1.to_be_bytes());
    payload.extend_from_slice(&primaries_red.0.to_be_bytes());
    payload.extend_from_slice(&primaries_red.1.to_be_bytes());
    payload.extend_from_slice(&white_point.0.to_be_bytes());
    payload.extend_from_slice(&white_point.1.to_be_bytes());
    payload.extend_from_slice(&max_luminance.to_be_bytes());
    payload.extend_from_slice(&min_luminance.to_be_bytes());
    wrap_box(b"mdcv", &payload)
}

/// Builds an ISO-BMFF `clli` (Content Light Level Information) box.
pub fn build_clli(max_content_light_level: u16, max_pic_average_light_level: u16) -> Vec<u8> {
    let mut payload = Vec::with_capacity(4);
    payload.extend_from_slice(&max_content_light_level.to_be_bytes());
    payload.extend_from_slice(&max_pic_average_light_level.to_be_bytes());
    wrap_box(b"clli", &payload)
}

/// Builds an ISO-BMFF `sv3d` (Spatial 360 Video) box enclosing a projection box (`proj`).
pub fn build_sv3d_equirectangular() -> Vec<u8> {
    let prhd = wrap_box(b"prhd", &[0, 0, 0, 0]);
    let equi = wrap_box(b"equi", &[0, 0, 0, 0]);
    let mut proj_body = Vec::new();
    proj_body.extend_from_slice(&prhd);
    proj_body.extend_from_slice(&equi);
    let proj = wrap_box(b"proj", &proj_body);

    let mut sv3d_body = Vec::new();
    sv3d_body.extend_from_slice(&proj);
    wrap_box(b"sv3d", &sv3d_body)
}

// ============================================================================
// ISO-BMFF MP4 Fixture Builder (High-Level)
// ============================================================================

#[derive(Debug, Clone)]
pub struct Mp4FixtureBuilder {
    pub major_brand: [u8; 4],
    pub duration_seconds: f64,
    pub video: Option<VideoTrackSpec>,
    pub audio: Option<AudioTrackSpec>,
    pub mdat_payload: Vec<u8>,
    pub force_co64: bool,
    pub faststart: bool,
}

#[derive(Debug, Clone)]
pub struct VideoTrackSpec {
    pub codec: [u8; 4],
    pub width: u32,
    pub height: u32,
    pub colr: Option<Vec<u8>>,
    pub mdcv: Option<Vec<u8>>,
    pub clli: Option<Vec<u8>>,
    pub sv3d: Option<Vec<u8>>,
    pub extra_boxes: Vec<u8>,
    pub keyframes: Option<Vec<u32>>,
}

#[derive(Debug, Clone)]
pub struct AudioTrackSpec {
    pub codec: [u8; 4],
    pub sample_rate: u32,
    pub channels: u16,
}

impl Default for Mp4FixtureBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Mp4FixtureBuilder {
    pub fn new() -> Self {
        Self {
            major_brand: *b"isom",
            duration_seconds: 10.0,
            video: Some(VideoTrackSpec {
                codec: *b"avc1",
                width: 1920,
                height: 1080,
                colr: None,
                mdcv: None,
                clli: None,
                sv3d: None,
                extra_boxes: Vec::new(),
                keyframes: Some(vec![1]),
            }),
            audio: Some(AudioTrackSpec {
                codec: *b"mp4a",
                sample_rate: 48_000,
                channels: 2,
            }),
            mdat_payload: b"RUVIC_SYNTHETIC_MEDIA_PAYLOAD_DATA".to_vec(),
            force_co64: false,
            faststart: true,
        }
    }

    pub fn video_only(mut self, width: u32, height: u32) -> Self {
        self.video = Some(VideoTrackSpec {
            codec: *b"avc1",
            width,
            height,
            colr: None,
            mdcv: None,
            clli: None,
            sv3d: None,
            extra_boxes: Vec::new(),
            keyframes: Some(vec![1]),
        });
        self.audio = None;
        self
    }

    pub fn audio_only(mut self, sample_rate: u32, channels: u16) -> Self {
        self.major_brand = *b"M4A ";
        self.video = None;
        self.audio = Some(AudioTrackSpec {
            codec: *b"mp4a",
            sample_rate,
            channels,
        });
        self
    }

    pub fn duration(mut self, dur: f64) -> Self {
        self.duration_seconds = dur;
        self
    }

    pub fn faststart(mut self, fs: bool) -> Self {
        self.faststart = fs;
        self
    }

    pub fn force_co64(mut self, co64: bool) -> Self {
        self.force_co64 = co64;
        self
    }

    pub fn mdat_payload(mut self, payload: &[u8]) -> Self {
        self.mdat_payload = payload.to_vec();
        self
    }

    pub fn colr_bt2020_pq(mut self) -> Self {
        if let Some(ref mut v) = self.video {
            v.colr = Some(build_colr_nclx(9, 16, 9, true));
        }
        self
    }

    pub fn hdr_mdcv_clli(mut self) -> Self {
        if let Some(ref mut v) = self.video {
            v.mdcv = Some(build_mdcv(
                (8500, 39850),
                (6550, 2300),
                (35400, 14600),
                (15635, 16450),
                10_000_000,
                50,
            ));
            v.clli = Some(build_clli(1000, 400));
        }
        self
    }

    pub fn spatial_360(mut self) -> Self {
        if let Some(ref mut v) = self.video {
            v.sv3d = Some(build_sv3d_equirectangular());
        }
        self
    }

    pub fn add_video_extra_box(mut self, extra_box: &[u8]) -> Self {
        if let Some(ref mut v) = self.video {
            v.extra_boxes.extend_from_slice(extra_box);
        }
        self
    }

    pub fn build(&self) -> Vec<u8> {
        if self.faststart {
            self.build_faststart()
        } else {
            self.build_non_faststart()
        }
    }

    pub fn build_faststart(&self) -> Vec<u8> {
        self.build_with_order(true)
    }

    pub fn build_non_faststart(&self) -> Vec<u8> {
        self.build_with_order(false)
    }

    fn build_with_order(&self, faststart: bool) -> Vec<u8> {
        let ftyp = build_ftyp(
            &self.major_brand,
            512,
            &[b"isom", b"iso2", b"avc1", b"mp41"],
        );
        let timescale = 1000u32;
        let duration_units = (self.duration_seconds.max(0.1) * timescale as f64).round() as u32;

        let payload_len = self.mdat_payload.len() as u32;
        let mdat_hdr_len = 8u64;

        // In faststart: ftyp -> moov -> mdat
        // In non-faststart: ftyp -> mdat -> moov
        let (first_chunk_offset, moov) = if faststart {
            // First pass: dummy offset 0 to calculate moov length
            let dummy_moov = self.assemble_moov(timescale, duration_units, 0, 0);
            let offset = ftyp.len() as u64 + dummy_moov.len() as u64 + mdat_hdr_len;
            let video_off = offset;
            let audio_off = if self.video.is_some() && self.audio.is_some() {
                offset + (payload_len as u64 / 2)
            } else {
                offset
            };
            let real_moov = self.assemble_moov(timescale, duration_units, video_off, audio_off);
            (offset, real_moov)
        } else {
            // In non-faststart, mdat payload starts immediately after ftyp + 8
            let offset = ftyp.len() as u64 + mdat_hdr_len;
            let video_off = offset;
            let audio_off = if self.video.is_some() && self.audio.is_some() {
                offset + (payload_len as u64 / 2)
            } else {
                offset
            };
            let real_moov = self.assemble_moov(timescale, duration_units, video_off, audio_off);
            (offset, real_moov)
        };

        let _ = first_chunk_offset;
        let mdat = wrap_box(b"mdat", &self.mdat_payload);

        let mut out = Vec::with_capacity(ftyp.len() + moov.len() + mdat.len());
        if faststart {
            out.extend_from_slice(&ftyp);
            out.extend_from_slice(&moov);
            out.extend_from_slice(&mdat);
        } else {
            out.extend_from_slice(&ftyp);
            out.extend_from_slice(&mdat);
            out.extend_from_slice(&moov);
        }
        out
    }

    fn assemble_moov(
        &self,
        timescale: u32,
        duration_units: u32,
        video_chunk_offset: u64,
        audio_chunk_offset: u64,
    ) -> Vec<u8> {
        let mut traks = Vec::new();
        let mut next_track_id = 1u32;

        if let Some(ref v) = self.video {
            let track_id = next_track_id;
            next_track_id += 1;

            let tkhd = build_tkhd(track_id, duration_units, v.width, v.height, false);
            let mdhd = build_mdhd(timescale, duration_units);
            let hdlr = build_hdlr(b"vide", "VideoHandler");
            let vmhd = build_vmhd();
            let dinf = build_dinf();

            let mut extra = Vec::new();
            if let Some(ref c) = v.colr {
                extra.extend_from_slice(c);
            }
            if let Some(ref m) = v.mdcv {
                extra.extend_from_slice(m);
            }
            if let Some(ref cl) = v.clli {
                extra.extend_from_slice(cl);
            }
            if let Some(ref s) = v.sv3d {
                extra.extend_from_slice(s);
            }
            extra.extend_from_slice(&v.extra_boxes);

            let stsd = build_stsd_video(&v.codec, v.width as u16, v.height as u16, &extra);
            let sample_count = (self.duration_seconds * 30.0).max(1.0) as u32;
            let stts = build_stts(sample_count, (timescale / 30).max(1));
            let stsc = build_stsc(&[(1, sample_count, 1)]);
            let sample_payload_len = if self.audio.is_some() {
                self.mdat_payload.len() / 2
            } else {
                self.mdat_payload.len()
            };
            let stsz = build_stsz(
                (sample_payload_len as u32 / sample_count).max(1),
                sample_count,
                &[],
            );

            let chunk_atom = if self.force_co64 || video_chunk_offset > u32::MAX as u64 {
                build_co64(&[video_chunk_offset])
            } else {
                build_stco(&[video_chunk_offset as u32])
            };

            let mut stbl_body = Vec::new();
            stbl_body.extend_from_slice(&stsd);
            stbl_body.extend_from_slice(&stts);
            if let Some(ref kf) = v.keyframes {
                stbl_body.extend_from_slice(&build_stss(kf));
            }
            stbl_body.extend_from_slice(&stsc);
            stbl_body.extend_from_slice(&stsz);
            stbl_body.extend_from_slice(&chunk_atom);
            let stbl = wrap_box(b"stbl", &stbl_body);

            let mut minf_body = Vec::new();
            minf_body.extend_from_slice(&vmhd);
            minf_body.extend_from_slice(&dinf);
            minf_body.extend_from_slice(&stbl);
            let minf = wrap_box(b"minf", &minf_body);

            let mut mdia_body = Vec::new();
            mdia_body.extend_from_slice(&mdhd);
            mdia_body.extend_from_slice(&hdlr);
            mdia_body.extend_from_slice(&minf);
            let mdia = wrap_box(b"mdia", &mdia_body);

            let mut trak_body = Vec::new();
            trak_body.extend_from_slice(&tkhd);
            trak_body.extend_from_slice(&mdia);
            traks.extend_from_slice(&wrap_box(b"trak", &trak_body));
        }

        if let Some(ref a) = self.audio {
            let track_id = next_track_id;
            next_track_id += 1;

            let tkhd = build_tkhd(track_id, duration_units, 0, 0, true);
            let mdhd = build_mdhd(
                a.sample_rate,
                (self.duration_seconds * a.sample_rate as f64) as u32,
            );
            let hdlr = build_hdlr(b"soun", "SoundHandler");
            let smhd = build_smhd();
            let dinf = build_dinf();

            let stsd = build_stsd_audio(&a.codec, a.sample_rate, a.channels);
            let sample_count =
                (self.duration_seconds * (a.sample_rate as f64 / 1024.0)).max(1.0) as u32;
            let stts = build_stts(sample_count, 1024);
            let stsc = build_stsc(&[(1, sample_count, 1)]);
            let audio_payload_len = if self.video.is_some() {
                self.mdat_payload.len() / 2
            } else {
                self.mdat_payload.len()
            };
            let stsz = build_stsz(
                (audio_payload_len as u32 / sample_count).max(1),
                sample_count,
                &[],
            );

            let chunk_atom = if self.force_co64 || audio_chunk_offset > u32::MAX as u64 {
                build_co64(&[audio_chunk_offset])
            } else {
                build_stco(&[audio_chunk_offset as u32])
            };

            let mut stbl_body = Vec::new();
            stbl_body.extend_from_slice(&stsd);
            stbl_body.extend_from_slice(&stts);
            stbl_body.extend_from_slice(&stsc);
            stbl_body.extend_from_slice(&stsz);
            stbl_body.extend_from_slice(&chunk_atom);
            let stbl = wrap_box(b"stbl", &stbl_body);

            let mut minf_body = Vec::new();
            minf_body.extend_from_slice(&smhd);
            minf_body.extend_from_slice(&dinf);
            minf_body.extend_from_slice(&stbl);
            let minf = wrap_box(b"minf", &minf_body);

            let mut mdia_body = Vec::new();
            mdia_body.extend_from_slice(&mdhd);
            mdia_body.extend_from_slice(&hdlr);
            mdia_body.extend_from_slice(&minf);
            let mdia = wrap_box(b"mdia", &mdia_body);

            let mut trak_body = Vec::new();
            trak_body.extend_from_slice(&tkhd);
            trak_body.extend_from_slice(&mdia);
            traks.extend_from_slice(&wrap_box(b"trak", &trak_body));
        }

        let mvhd = build_mvhd(timescale, duration_units, next_track_id);
        let mut moov_body = Vec::new();
        moov_body.extend_from_slice(&mvhd);
        moov_body.extend_from_slice(&traks);
        wrap_box(b"moov", &moov_body)
    }
}

// ============================================================================
// EBML (WebM) Builders
// ============================================================================

/// Builds a pure-Rust synthetic WebM container with EBML header, Info, Tracks,
/// optional HDR Colour (`0x55B0`) and Projection (`0x7670`), and Cluster.
#[derive(Debug, Clone)]
pub struct WebmFixtureBuilder {
    pub duration_seconds: f64,
    pub video: Option<(String, u32, u32)>, // (codec_id, width, height)
    pub audio: Option<(String, f64, u16)>, // (codec_id, sample_rate, channels)
    pub colour: Option<Vec<u8>>,
    pub projection: Option<Vec<u8>>,
    pub payload: Vec<u8>,
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
            video: Some(("V_VP9".to_string(), 1920, 1080)),
            audio: Some(("A_OPUS".to_string(), 48000.0, 2)),
            colour: None,
            projection: None,
            payload: b"RUVIC_SYNTHETIC_WEBM_PAYLOAD".to_vec(),
        }
    }

    pub fn video_only(mut self, width: u32, height: u32) -> Self {
        self.video = Some(("V_VP9".to_string(), width, height));
        self.audio = None;
        self
    }

    pub fn audio_only(mut self, sample_rate: f64, channels: u16) -> Self {
        self.video = None;
        self.audio = Some(("A_OPUS".to_string(), sample_rate, channels));
        self
    }

    pub fn duration(mut self, dur: f64) -> Self {
        self.duration_seconds = dur;
        self
    }

    pub fn colour_bt2020_pq(mut self) -> Self {
        let mut c_body = Vec::new();
        c_body.extend_from_slice(&ebml_uint(0x55B1, 9)); // matrix: BT.2020 non-constant luminance
        c_body.extend_from_slice(&ebml_uint(0x55B7, 9)); // primaries: BT.2020
        c_body.extend_from_slice(&ebml_uint(0x55B8, 16)); // transfer: SMPTE ST 2084 (PQ)
        c_body.extend_from_slice(&ebml_uint(0x55B9, 2)); // range: full
        self.colour = Some(ebml_element(0x55B0, &c_body));
        self
    }

    pub fn projection_equirectangular(mut self) -> Self {
        let mut p_body = Vec::new();
        p_body.extend_from_slice(&ebml_uint(0x7671, 1)); // projection type 1 = equirectangular
        self.projection = Some(ebml_element(0x7670, &p_body));
        self
    }

    pub fn build(&self) -> Vec<u8> {
        // EBML Header
        let mut hdr_body = Vec::new();
        hdr_body.extend_from_slice(&ebml_uint(0x4286, 1));
        hdr_body.extend_from_slice(&ebml_uint(0x42F7, 1));
        hdr_body.extend_from_slice(&ebml_uint(0x42F2, 4));
        hdr_body.extend_from_slice(&ebml_uint(0x42F3, 8));
        hdr_body.extend_from_slice(&ebml_element(0x4282, b"webm"));
        hdr_body.extend_from_slice(&ebml_uint(0x4287, 4));
        hdr_body.extend_from_slice(&ebml_uint(0x4285, 2));
        let ebml_hdr = ebml_element(0x1A45_DFA3, &hdr_body);

        // Segment Info
        let mut info_body = Vec::new();
        info_body.extend_from_slice(&ebml_uint(0x002A_D7B1, 1_000_000)); // TimestampScale 1ms
        let dur_ms = (self.duration_seconds.max(0.1) * 1000.0).to_be_bytes();
        info_body.extend_from_slice(&ebml_element(0x4489, &dur_ms));
        info_body.extend_from_slice(&ebml_element(0x4D80, b"ruvic-synth"));
        let info_elem = ebml_element(0x1549_A966, &info_body);

        // Tracks
        let mut tracks_body = Vec::new();
        let mut track_num = 1u64;

        if let Some((ref codec_id, w, h)) = self.video {
            let mut video_el = Vec::new();
            video_el.extend_from_slice(&ebml_uint(0xB0, u64::from(w.max(2))));
            video_el.extend_from_slice(&ebml_uint(0xBA, u64::from(h.max(2))));
            if let Some(ref c) = self.colour {
                video_el.extend_from_slice(c);
            }
            if let Some(ref p) = self.projection {
                video_el.extend_from_slice(p);
            }

            let mut te = Vec::new();
            te.extend_from_slice(&ebml_uint(0xD7, track_num));
            te.extend_from_slice(&ebml_uint(0x73C5, track_num));
            te.extend_from_slice(&ebml_uint(0x83, 1)); // track type 1 = video
            te.extend_from_slice(&ebml_element(0x86, codec_id.as_bytes()));
            te.extend_from_slice(&ebml_element(0xE0, &video_el));
            tracks_body.extend_from_slice(&ebml_element(0xAE, &te));
            track_num += 1;
        }

        if let Some((ref codec_id, sr, ch)) = self.audio {
            let mut audio_el = Vec::new();
            audio_el.extend_from_slice(&ebml_element(0xB5, &sr.to_be_bytes()));
            audio_el.extend_from_slice(&ebml_uint(0x9F, u64::from(ch.max(1))));

            let mut te = Vec::new();
            te.extend_from_slice(&ebml_uint(0xD7, track_num));
            te.extend_from_slice(&ebml_uint(0x73C5, track_num));
            te.extend_from_slice(&ebml_uint(0x83, 2)); // track type 2 = audio
            te.extend_from_slice(&ebml_element(0x86, codec_id.as_bytes()));
            te.extend_from_slice(&ebml_element(0xE1, &audio_el));
            tracks_body.extend_from_slice(&ebml_element(0xAE, &te));
        }

        let tracks_elem = ebml_element(0x1654_AE6B, &tracks_body);

        // Cluster with SimpleBlock
        let mut cluster_body = ebml_uint(0xE7, 0); // timecode 0
        let payload_slice = if self.payload.is_empty() {
            b"RUVIC_CLUSTER_DATA".as_slice()
        } else {
            &self.payload
        };
        cluster_body.extend_from_slice(&ebml_element(0xA3, payload_slice));
        let cluster_elem = ebml_element(0x1F43_B675, &cluster_body);

        let mut seg_body = info_elem;
        seg_body.extend_from_slice(&tracks_elem);
        seg_body.extend_from_slice(&cluster_elem);
        let segment = ebml_element(0x1853_8067, &seg_body);

        let mut out = Vec::with_capacity(ebml_hdr.len() + segment.len());
        out.extend_from_slice(&ebml_hdr);
        out.extend_from_slice(&segment);
        out
    }
}

pub fn ebml_element(id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let id_bytes = id.to_be_bytes();
    let start = id_bytes.iter().position(|&b| b != 0).unwrap_or(3);
    out.extend_from_slice(&id_bytes[start..]);
    out.extend_from_slice(&encode_ebml_vint(payload.len() as u64));
    out.extend_from_slice(payload);
    out
}

pub fn ebml_uint(id: u32, val: u64) -> Vec<u8> {
    if val == 0 {
        return ebml_element(id, &[0]);
    }
    let bytes = val.to_be_bytes();
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(7);
    ebml_element(id, &bytes[start..])
}

pub fn encode_ebml_vint(size: u64) -> Vec<u8> {
    if size < 0x7F {
        vec![0x80 | (size as u8)]
    } else if size < 0x3FFF {
        let v = (0x4000u16 | (size as u16)).to_be_bytes();
        v.to_vec()
    } else if size < 0x1F_FFFF {
        let v = (0x20_0000u32 | (size as u32)).to_be_bytes();
        v[1..4].to_vec()
    } else if size < 0x0FFF_FFFF {
        let v = (0x1000_0000u32 | (size as u32)).to_be_bytes();
        v.to_vec()
    } else {
        let mut out = vec![0x01];
        out.extend_from_slice(&size.to_be_bytes()[1..]);
        out
    }
}

// ============================================================================
// ADTS AAC Fixture Builders & Validator
// ============================================================================

/// ADTS AAC frame builder (ISO/IEC 13818-7).
#[derive(Debug, Clone)]
pub struct AdtsFixtureBuilder {
    pub sample_rate: u32,
    pub channels: u16,
}

impl AdtsFixtureBuilder {
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            sample_rate,
            channels,
        }
    }

    pub fn sample_rate_index(sr: u32) -> u8 {
        match sr {
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
            7350 => 12,
            _ => 3, // fallback 48000
        }
    }

    /// Builds a single valid ADTS frame with 7-byte header + payload.
    pub fn build_frame(&self, payload: &[u8]) -> Vec<u8> {
        let frame_len = 7 + payload.len();
        let sr_idx = Self::sample_rate_index(self.sample_rate);
        let ch = self.channels.clamp(1, 7) as u8;

        let mut header = [0u8; 7];
        header[0] = 0xFF;
        header[1] = 0xF1; // syncword 0xFFF, MPEG-4, layer 00, no CRC (protection_absent = 1)
        header[2] = 0x40 | (sr_idx << 2) | ((ch >> 2) & 0x01); // profile 01 (AAC-LC)
        header[3] = (ch << 6) | (((frame_len >> 11) & 0x03) as u8);
        header[4] = ((frame_len >> 3) & 0xFF) as u8;
        header[5] = (((frame_len & 0x07) << 5) | 0x1F) as u8; // buffer fullness top 5 bits
        header[6] = 0xFC; // buffer fullness bottom 6 bits (all 1s) + 0 AAC frames

        let mut out = Vec::with_capacity(frame_len);
        out.extend_from_slice(&header);
        out.extend_from_slice(payload);
        out
    }

    /// Builds a multi-frame continuous ADTS stream.
    pub fn build_stream(&self, frame_count: usize, payload_size: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for i in 0..frame_count {
            let payload = vec![(i as u8).wrapping_add(0x42); payload_size];
            out.extend_from_slice(&self.build_frame(&payload));
        }
        out
    }
}

/// Validates that `data` is a valid continuous ADTS AAC bitstream from start to EOF.
/// Returns Ok(frame_count) or Err(reason).
pub fn validate_adts_stream(data: &[u8]) -> Result<usize, String> {
    if data.is_empty() {
        return Err("data is empty".to_string());
    }
    let mut pos = 0usize;
    let mut count = 0usize;

    while pos < data.len() {
        if pos + 7 > data.len() {
            return Err(format!("truncated ADTS header at offset {pos}"));
        }
        if data[pos] != 0xFF || (data[pos + 1] & 0xF6) != 0xF0 {
            return Err(format!(
                "invalid syncword at offset {pos}: {:02X}{:02X}",
                data[pos],
                data[pos + 1]
            ));
        }
        let frame_len = (usize::from(data[pos + 3] & 0x03) << 11)
            | (usize::from(data[pos + 4]) << 3)
            | usize::from(data[pos + 5] >> 5);
        if frame_len < 7 {
            return Err(format!("invalid frame length {frame_len} at offset {pos}"));
        }
        if pos + frame_len > data.len() {
            return Err(format!(
                "frame length {frame_len} extends past EOF at offset {pos}"
            ));
        }
        pos += frame_len;
        count += 1;
    }

    Ok(count)
}

// ============================================================================
// ISO-BMFF Box Inspection & Verification Utilities
// ============================================================================

/// Parses all top-level boxes in an MP4 byte array.
/// Returns a list of `(box_tag, box_offset, box_total_size)`.
pub fn parse_top_level_boxes(data: &[u8]) -> Vec<([u8; 4], usize, usize)> {
    let mut boxes = Vec::new();
    let mut pos = 0usize;

    while pos + 8 <= data.len() {
        let size32 =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let tag = [data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]];
        let (box_len, _hlen) = if size32 == 1 {
            if pos + 16 > data.len() {
                break;
            }
            let size64 = u64::from_be_bytes([
                data[pos + 8],
                data[pos + 9],
                data[pos + 10],
                data[pos + 11],
                data[pos + 12],
                data[pos + 13],
                data[pos + 14],
                data[pos + 15],
            ]) as usize;
            (size64, 16)
        } else if size32 == 0 {
            (data.len() - pos, 8)
        } else {
            (size32, 8)
        };

        if box_len < 8 || pos + box_len > data.len() {
            boxes.push((tag, pos, box_len));
            break;
        }
        boxes.push((tag, pos, box_len));
        pos += box_len;
    }

    boxes
}

/// Checks whether `moov` precedes `mdat` in the top-level boxes.
pub fn is_mp4_faststart(data: &[u8]) -> bool {
    let boxes = parse_top_level_boxes(data);
    let moov_pos = boxes.iter().position(|(t, _, _)| t == b"moov");
    let mdat_pos = boxes.iter().position(|(t, _, _)| t == b"mdat");
    match (moov_pos, mdat_pos) {
        (Some(moov), Some(mdat)) => moov < mdat,
        _ => false,
    }
}

/// Recursively searches for the first box with matching `target` tag and returns its payload slice.
pub fn find_box_payload<'a>(data: &'a [u8], target: &[u8; 4]) -> Option<&'a [u8]> {
    let mut pos = 0usize;
    while pos + 8 <= data.len() {
        let size32 =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let tag = [data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]];
        let (box_len, hlen) = if size32 == 1 {
            if pos + 16 > data.len() {
                return None;
            }
            let s64 = u64::from_be_bytes([
                data[pos + 8],
                data[pos + 9],
                data[pos + 10],
                data[pos + 11],
                data[pos + 12],
                data[pos + 13],
                data[pos + 14],
                data[pos + 15],
            ]) as usize;
            (s64, 16)
        } else if size32 == 0 {
            (data.len() - pos, 8)
        } else {
            (size32, 8)
        };

        if box_len < hlen || pos + box_len > data.len() {
            return None;
        }

        if &tag == target {
            return Some(&data[pos + hlen..pos + box_len]);
        }

        // Search recursively inside container boxes
        if matches!(
            &tag,
            b"moov"
                | b"trak"
                | b"mdia"
                | b"minf"
                | b"stbl"
                | b"dinf"
                | b"stsd"
                | b"avc1"
                | b"hvc1"
                | b"vp09"
                | b"av01"
                | b"mp4a"
                | b"sv3d"
                | b"proj"
        ) {
            let inner_payload = &data[pos + hlen..pos + box_len];
            // stsd has version/flags (4B) and entry count (4B) before child boxes
            let search_start = if &tag == b"stsd" {
                8
            } else if matches!(&tag, b"avc1" | b"hvc1" | b"vp09" | b"av01") {
                78
            } else if &tag == b"mp4a" {
                28
            } else {
                0
            };
            if inner_payload.len() > search_start {
                if let Some(found) = find_box_payload(&inner_payload[search_start..], target) {
                    return Some(found);
                }
            }
        }

        pos += box_len;
    }
    None
}

/// Recursively checks whether a box with `target` tag exists in `data`.
pub fn has_box_recursive(data: &[u8], target: &[u8; 4]) -> bool {
    find_box_payload(data, target).is_some()
}

/// Extracts all chunk offsets from the first `stco` box found.
pub fn extract_stco_offsets(data: &[u8]) -> Vec<u64> {
    if let Some(payload) = find_box_payload(data, b"stco") {
        if payload.len() < 8 {
            return Vec::new();
        }
        let count = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]) as usize;
        let mut offsets = Vec::with_capacity(count);
        let mut p = 8;
        for _ in 0..count {
            if p + 4 > payload.len() {
                break;
            }
            let off =
                u32::from_be_bytes([payload[p], payload[p + 1], payload[p + 2], payload[p + 3]]);
            offsets.push(off as u64);
            p += 4;
        }
        offsets
    } else {
        Vec::new()
    }
}

/// Extracts all chunk offsets from the first `co64` box found.
pub fn extract_co64_offsets(data: &[u8]) -> Vec<u64> {
    if let Some(payload) = find_box_payload(data, b"co64") {
        if payload.len() < 8 {
            return Vec::new();
        }
        let count = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]) as usize;
        let mut offsets = Vec::with_capacity(count);
        let mut p = 8;
        for _ in 0..count {
            if p + 8 > payload.len() {
                break;
            }
            let off = u64::from_be_bytes([
                payload[p],
                payload[p + 1],
                payload[p + 2],
                payload[p + 3],
                payload[p + 4],
                payload[p + 5],
                payload[p + 6],
                payload[p + 7],
            ]);
            offsets.push(off);
            p += 8;
        }
        offsets
    } else {
        Vec::new()
    }
}

// ============================================================================
// Isolated Command Runner for Tests
// ============================================================================

/// Returns a pre-configured `assert_cmd::Command` for `ruvic` completely isolated
/// from external FFmpeg/ffprobe, with an empty PATH and isolated config/cache/state dirs.
pub fn native_cmd(isolated_dir: &Path) -> assert_cmd::Command {
    let empty_path_dir = isolated_dir.join("empty_path");
    let _ = std::fs::create_dir_all(&empty_path_dir);

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("ruvic");
    cmd.env("PATH", &empty_path_dir)
        .env_remove("RUVIC_FFMPEG_PATH")
        .env_remove("RUVIC_FFPROBE_PATH")
        .env("RUVI_CONFIG_DIR", isolated_dir.join("cfg"))
        .env("RUVI_CACHE_DIR", isolated_dir.join("cache"))
        .env("RUVI_STATE_DIR", isolated_dir.join("state"));
    cmd
}

/// Asserts that no leftover `.ruvic-tmp-*` files exist in the specified directory.
pub fn assert_no_temp_files(dir: &Path) {
    if !dir.is_dir() {
        return;
    }
    let leftovers: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".ruvic-tmp-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "expected zero .ruvic-tmp-* files in {}, found: {:?}",
        dir.display(),
        leftovers
    );
}

/// Probes a media file using `native_cmd` and returns parsed JSON.
pub fn probe_json(isolated_dir: &Path, media_path: &Path) -> serde_json::Value {
    let output = native_cmd(isolated_dir)
        .args(["probe", media_path.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).expect("valid probe json")
}
