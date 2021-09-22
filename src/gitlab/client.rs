use gitlab::Gitlab;
use gitlab::api::{projects, Query};
use tracing::debug;

use super::common::{Pipeline, MergeRequest};

#[derive(Clone, Debug)]
pub struct GitlabClient {
    hostname: String,
    access_token: String,
}

impl GitlabClient {
    pub fn new(hostname: String, access_token: String) -> Self {
        Self {
            hostname,
            access_token,
        }
    }


    fn create_client(&self) -> Gitlab {
        let hostname = &self.hostname;
        let access_token = &self.access_token;
        let client = Gitlab::new(hostname, access_token).unwrap();

        client
    }

    pub async fn get_pipeline_details(&self, project_id: u64, pipeline_id: u64) -> Option<Pipeline> {
        let client = self.create_client();
        let endpoint = projects::pipelines::Pipeline::builder()
            .project(project_id)
            .pipeline(pipeline_id)
            .build()
            .unwrap();
        let pipeline: Pipeline = endpoint.query(&client).unwrap();
        debug!("Pipeline: {:?}", pipeline);
        Some(pipeline)
    }

    pub async fn get_merge_request_details(&self, project_id: u64, merge_request_iid: u64) -> Option<MergeRequest> {
        let client = self.create_client();
        let endpoint  = projects::merge_requests::MergeRequest::builder()
            .project(project_id)
            .merge_request(merge_request_iid)
            .build()
            .unwrap();
        let merge_request: MergeRequest = endpoint.query(&client).unwrap();
        debug!("Merge Request: {:?}", merge_request);

        Some(merge_request)
    }
}
