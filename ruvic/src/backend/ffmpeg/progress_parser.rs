use crate::backend::ProgressUpdate;

#[derive(Debug, Default)]
pub struct ProgressParser {
    current: ProgressUpdate,
}

impl ProgressParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_line(&mut self, line: &str) -> Option<ProgressUpdate> {
        let trimmed = line.trim();
        let (key, value) = trimmed.split_once('=')?;
        let key = key.trim();
        let value = value.trim();

        match key {
            "out_time_us" | "out_time_ms" => {
                if let Ok(us) = value.parse::<u64>() {
                    self.current.out_time_us = Some(us);
                }
                None
            }
            "fps" => {
                if let Ok(fps) = value.parse::<f64>() {
                    self.current.fps = Some(fps);
                }
                None
            }
            "speed" => {
                if value != "N/A" {
                    self.current.speed = Some(value.to_string());
                }
                None
            }
            "progress" => {
                self.current.finished = value == "end";
                let emitted = std::mem::take(&mut self.current);
                Some(emitted)
            }
            _ => None,
        }
    }
}
