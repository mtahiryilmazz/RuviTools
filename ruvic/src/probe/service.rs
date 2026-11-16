use std::path::Path;

use crate::backend::MediaBackend;
use crate::domain::media_info::MediaInfo;
use crate::error::{RuviError, RuviResult};

pub struct ProbeService<'a> {
    backend: &'a dyn MediaBackend,
}

impl<'a> ProbeService<'a> {
    pub fn new(backend: &'a dyn MediaBackend) -> Self {
        Self { backend }
    }

    pub async fn probe_file(&self, path: &Path) -> RuviResult<MediaInfo> {
        if !path.exists() {
            return Err(RuviError::Filesystem(format!(
                "input file does not exist: {}",
                path.display()
            )));
        }
        if !path.is_file() {
            return Err(RuviError::InvalidArgument(format!(
                "input path is not a file: {}",
                path.display()
            )));
        }
        self.backend.probe(path).await
    }
}
