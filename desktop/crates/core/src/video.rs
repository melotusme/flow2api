//! Track frontend video operations without mixing results from unrelated records.
use crate::flow;
use anyhow::{bail, Result};
use serde_json::Value;

#[derive(Clone, Debug)]
pub(crate) struct VideoResult {
    pub url: String,
    pub media_id: String,
}

struct Operation {
    id: String,
    media_id: String,
    result: Option<VideoResult>,
}

pub(crate) struct VideoBatch {
    project: String,
    operations: Vec<Operation>,
    direct: Option<VideoResult>,
}

impl VideoBatch {
    pub fn submitted(payload: &Value, project: &str) -> Result<Self> {
        let mut batch = Self {
            project: project.into(),
            operations: vec![],
            direct: None,
        };
        for record in flow::video_records(payload, Some(project)) {
            let id = record[0].as_str().unwrap();
            if batch.operations.iter().any(|op| op.id == id) {
                continue;
            }
            batch.operations.push(Operation {
                id: id.into(),
                media_id: record[2].as_str().unwrap().into(),
                result: None,
            });
        }
        if batch.operations.is_empty() {
            // Only a response without operation records can use a global direct URL.
            if flow::video_records(payload, None).is_empty() {
                if let Some(code) = flow::error_code(payload) {
                    bail!("Flow 视频生成失败：{code}");
                }
                batch.direct = flow::media_url(payload, "video").map(|url| VideoResult {
                    url,
                    media_id: String::new(),
                });
            }
            if batch.direct.is_none() {
                bail!("视频提交未返回当前项目的任务 ID 或下载地址");
            }
        } else {
            batch.update(payload, None)?;
        }
        Ok(batch)
    }

    pub fn pending(&self) -> Vec<String> {
        self.operations
            .iter()
            .filter(|op| op.result.is_none())
            .map(|op| op.id.clone())
            .collect()
    }

    pub fn results(&self) -> Option<Vec<VideoResult>> {
        if let Some(result) = &self.direct {
            return Some(vec![result.clone()]);
        }
        self.operations.iter().map(|op| op.result.clone()).collect()
    }

    /// `queried` scopes a record-free as29s reply to the requested operation.
    pub fn update(&mut self, payload: &Value, queried: Option<&str>) -> Result<()> {
        let records = flow::video_records(payload, None);
        if records.is_empty() {
            if let Some(code) = flow::error_code(payload) {
                bail!("Flow 视频生成失败：{code}");
            }
            if let Some(op) = queried.and_then(|id| {
                self.operations
                    .iter_mut()
                    .find(|op| op.id == id && op.result.is_none())
            }) {
                if let Some(url) = flow::media_url(payload, "video") {
                    op.result = Some(VideoResult {
                        url,
                        media_id: op.media_id.clone(),
                    });
                }
            }
            return Ok(());
        }
        for record in records {
            let id = record[0].as_str().unwrap();
            if queried.is_some_and(|query| query != id)
                || !(record[1].is_null() || record[1] == self.project)
            {
                continue;
            }
            let Some(op) = self
                .operations
                .iter_mut()
                .find(|op| op.id == id && op.result.is_none())
            else {
                continue;
            };
            op.media_id = record[2].as_str().unwrap().into();
            if let Some(code) = flow::error_code(record) {
                bail!("Flow 视频生成失败：{code}");
            }
            if let Some(url) = flow::media_url(record, "video") {
                op.result = Some(VideoResult {
                    url,
                    media_id: op.media_id.clone(),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const P: &str = "10000000-0000-0000-0000-000000000000";
    const A: &str = "20000000-0000-0000-0000-000000000000";
    const B: &str = "30000000-0000-0000-0000-000000000000";
    const C: &str = "40000000-0000-0000-0000-000000000000";
    const M: &str = "50000000-0000-0000-0000-000000000000";
    const N: &str = "60000000-0000-0000-0000-000000000000";

    #[test]
    fn reordered_multiple_results_keep_operation_identity_and_updated_media() -> Result<()> {
        let mut batch = VideoBatch::submitted(&json!([[A, P, M], [B, P, M], [A, P, M]]), P)?;
        assert_eq!(batch.pending(), vec![A, B]);
        batch.update(
            &json!([
                [C, P, M, "PUBLIC_ERROR_UNSAFE_GENERATION"],
                [B, null, N, "https://example.test/video/b"]
            ]),
            None,
        )?;
        assert_eq!(batch.pending(), vec![A]);
        // A later active duplicate cannot overwrite a completed result.
        batch.update(
            &json!([[B, P, M], [A, P, N, "https://example.test/video/a"]]),
            None,
        )?;
        let results = batch.results().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].url, "https://example.test/video/a");
        assert_eq!(results[1].url, "https://example.test/video/b");
        assert!(results.iter().all(|r| r.media_id == N));
        Ok(())
    }

    #[test]
    fn fallback_is_scoped_and_does_not_borrow_unrelated_urls() -> Result<()> {
        let mut batch = VideoBatch::submitted(&json!([[A, P, M]]), P)?;
        batch.update(
            &json!([
                [A, C, N, "https://example.test/video/wrong-project"],
                [B, P, N, "https://example.test/video/wrong-operation"]
            ]),
            Some(A),
        )?;
        assert!(batch.results().is_none());
        batch.update(&json!([[A, null, N]]), None)?;
        batch.update(&json!(["https://example.test/video/ready"]), Some(A))?;
        assert_eq!(batch.results().unwrap()[0].media_id, N);
        assert!(
            VideoBatch::submitted(&json!([[B, C, M, "https://example.test/video/other"]]), P)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn direct_results_and_upstream_errors_are_explicit() -> Result<()> {
        let direct = VideoBatch::submitted(&json!(["https://example.test/video/direct"]), P)?;
        assert_eq!(direct.results().unwrap()[0].media_id, "");
        let mut batch = VideoBatch::submitted(&json!([[A, P, M]]), P)?;
        assert!(batch
            .update(&json!(["PUBLIC_ERROR_UNSAFE_GENERATION"]), None)
            .is_err());
        assert!(batch
            .update(&json!([[A, P, M, "ERROR_FAILED"]]), None)
            .is_err());
        Ok(())
    }
}
