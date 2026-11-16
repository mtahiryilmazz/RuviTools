use crate::domain::conversion_job::OperationKind;
use crate::domain::output_spec::SubtitleMode;
use crate::domain::stream::{AudioCodec, VideoCodec};
use crate::planner::ConversionPlan;

/// Builds safe, shell-free argument vectors for `ffmpeg` and `ffprobe`.
pub fn build_ffprobe_args(input: &str) -> Vec<String> {
    vec![
        "-v".to_string(),
        "quiet".to_string(),
        "-print_format".to_string(),
        "json".to_string(),
        "-show_format".to_string(),
        "-show_streams".to_string(),
        "-show_chapters".to_string(),
        input.to_string(),
    ]
}

pub fn build_ffmpeg_args(plan: &ConversionPlan) -> Vec<String> {
    let mut args = vec![
        "-hide_banner".to_string(),
        "-y".to_string(),
        "-nostdin".to_string(),
    ];

    if let Some(ref hwaccel) = plan.hwaccel_input_flag {
        args.push("-hwaccel".to_string());
        args.push(hwaccel.clone());
    }

    if let Some(threads) = plan.threads {
        args.push("-threads".to_string());
        args.push(threads.to_string());
    }

    match plan.operation {
        OperationKind::Trim => {
            if let Some(ref start) = plan.start_time {
                args.push("-ss".to_string());
                args.push(start.clone());
            }
            args.push("-i".to_string());
            args.push(plan.input.to_string_lossy().into_owned());
            if let Some(ref end) = plan.end_time {
                args.push("-to".to_string());
                args.push(end.clone());
            } else if let Some(dur) = plan.duration_seconds {
                args.push("-t".to_string());
                args.push(format!("{dur:.3}"));
            }
            if plan.reencode {
                append_video_args(&mut args, plan);
                append_audio_args(&mut args, plan);
            } else {
                args.push("-c".to_string());
                args.push("copy".to_string());
            }
        }
        OperationKind::Thumbnail => {
            if let Some(ref at) = plan.thumbnail_at {
                args.push("-ss".to_string());
                args.push(at.clone());
            }
            args.push("-i".to_string());
            args.push(plan.input.to_string_lossy().into_owned());
            args.push("-vframes".to_string());
            args.push("1".to_string());
            args.push("-q:v".to_string());
            args.push("2".to_string());
        }
        OperationKind::ExtractAudio => {
            args.push("-i".to_string());
            args.push(plan.input.to_string_lossy().into_owned());
            args.push("-vn".to_string());
            if let Some(idx) = plan.audio_stream {
                args.push("-map".to_string());
                args.push(format!("0:{idx}"));
            }
            append_audio_args(&mut args, plan);
        }
        OperationKind::Remux => {
            args.push("-i".to_string());
            args.push(plan.input.to_string_lossy().into_owned());
            if let Some(idx) = plan.video_stream {
                args.push("-map".to_string());
                args.push(format!("0:{idx}"));
            }
            if let Some(idx) = plan.audio_stream {
                args.push("-map".to_string());
                args.push(format!("0:{idx}"));
            }
            args.push("-c".to_string());
            args.push("copy".to_string());
            if plan.faststart {
                args.push("-movflags".to_string());
                args.push("+faststart".to_string());
            }
        }
        OperationKind::Convert => {
            args.push("-i".to_string());
            args.push(plan.input.to_string_lossy().into_owned());
            if let Some(idx) = plan.video_stream {
                args.push("-map".to_string());
                args.push(format!("0:{idx}"));
            }
            if let Some(idx) = plan.audio_stream {
                args.push("-map".to_string());
                args.push(format!("0:{idx}"));
            }
            append_video_args(&mut args, plan);
            append_audio_args(&mut args, plan);
            match plan.subtitle_mode {
                SubtitleMode::None => {
                    args.push("-sn".to_string());
                }
                SubtitleMode::Copy => {
                    args.push("-c:s".to_string());
                    args.push("copy".to_string());
                }
                SubtitleMode::MovText => {
                    args.push("-c:s".to_string());
                    args.push("mov_text".to_string());
                }
            }
            if plan.faststart {
                args.push("-movflags".to_string());
                args.push("+faststart".to_string());
            }
        }
    }

    args.push("-f".to_string());
    args.push(plan.container.ffmpeg_format().to_string());
    args.push("-progress".to_string());
    args.push("pipe:1".to_string());
    args.push("-nostats".to_string());
    args.push(plan.temp_output.to_string_lossy().into_owned());

    args
}

fn append_video_args(args: &mut Vec<String>, plan: &ConversionPlan) {
    let Some(ref encoder) = plan.video_encoder else {
        return;
    };
    args.push("-c:v".to_string());
    args.push(encoder.clone());
    if encoder == "copy" {
        return;
    }

    if let Some(q) = plan.quality {
        let flag = match plan.video_codec {
            Some(VideoCodec::H264 | VideoCodec::H265)
                if encoder.contains("nvenc") || encoder.contains("qsv") =>
            {
                "-cq"
            }
            _ => "-crf",
        };
        args.push(flag.to_string());
        args.push(q.to_string());
    }

    if let Some(ref speed) = plan.speed_preset
        && (encoder == "libx264" || encoder == "libx265")
    {
        args.push("-preset".to_string());
        args.push(speed.clone());
    }

    if let Some(ref pix_fmt) = plan.pixel_format {
        args.push("-pix_fmt".to_string());
        args.push(pix_fmt.clone());
    }

    if let Some(max_h) = plan.max_height {
        args.push("-vf".to_string());
        args.push(format!("scale=-2:'min({max_h},ih)'"));
    }
}

fn append_audio_args(args: &mut Vec<String>, plan: &ConversionPlan) {
    let Some(ref encoder) = plan.audio_encoder else {
        return;
    };
    args.push("-c:a".to_string());
    args.push(encoder.clone());
    if encoder == "copy"
        || matches!(
            plan.audio_codec,
            Some(AudioCodec::Flac | AudioCodec::PcmS16Le)
        )
    {
        return;
    }
    if let Some(ref bitrate) = plan.audio_bitrate {
        args.push("-b:a".to_string());
        args.push(bitrate.clone());
    }
}
