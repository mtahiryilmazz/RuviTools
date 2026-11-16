#[derive(Debug, Clone, Default)]
pub struct MpdDocument {
    pub mpd_type: String,
    pub media_duration_seconds: Option<f64>,
    pub base_url: Option<String>,
    pub periods: Vec<Period>,
}

#[derive(Debug, Clone, Default)]
pub struct Period {
    pub duration_seconds: Option<f64>,
    pub base_url: Option<String>,
    pub adaptation_sets: Vec<AdaptationSet>,
}

#[derive(Debug, Clone, Default)]
pub struct AdaptationSet {
    pub content_type: Option<String>,
    pub mime_type: Option<String>,
    pub lang: Option<String>,
    pub base_url: Option<String>,
    pub segment_template: Option<SegmentTemplateSpec>,
    pub representations: Vec<Representation>,
}

#[derive(Debug, Clone, Default)]
pub struct Representation {
    pub id: String,
    pub bandwidth: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub codecs: Option<String>,
    pub frame_rate: Option<String>,
    pub mime_type: Option<String>,
    pub base_url: Option<String>,
    pub segment_template: Option<SegmentTemplateSpec>,
    pub segment_list: Option<SegmentListSpec>,
}

#[derive(Debug, Clone, Default)]
pub struct SegmentTemplateSpec {
    pub initialization: Option<String>,
    pub media: Option<String>,
    pub start_number: Option<u64>,
    pub timescale: Option<u64>,
    pub duration: Option<u64>,
    pub timeline: Vec<SegmentTimelineEntry>,
}

#[derive(Debug, Clone, Copy)]
pub struct SegmentTimelineEntry {
    pub start_time: Option<u64>,
    pub duration: u64,
    pub repeat: i64,
}

#[derive(Debug, Clone, Default)]
pub struct SegmentListSpec {
    pub initialization: Option<String>,
    pub segments: Vec<String>,
}
