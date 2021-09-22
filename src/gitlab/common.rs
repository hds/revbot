use chrono::{DateTime, Utc};
pub use gitlab::types::{MergeStatus, StatusState};
use serde::Deserialize;


#[derive(Deserialize, Clone, Debug)]
pub struct User {
    pub email: String,
    pub id: u64,
    pub name: String,
    pub username: String,
}

impl PartialEq for User {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct UserBasic {
    pub id: u64,
    pub username: String,
    pub web_url: String,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct MergeRequestAttributes {
    pub action: Option<String>,
    pub iid: u64,
    pub merge_status: MergeStatus,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct PipelineAttributes {
    pub finished_at: Option<String>,
    pub id: u64,
    #[serde(rename = "ref")]
    pub ref_: String,
    pub status: StatusState,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Project {
    pub id: u64,
    pub name: String,
    pub path_with_namespace: String,
    pub web_url: String,
}

#[derive(Debug, Deserialize)]
pub struct Pipeline {
    #[serde(rename = "ref")]
    pub ref_: String,
    pub status: StatusState,
    pub web_url: String,
}

#[derive(Debug, Deserialize)]
pub struct MergeRequest {
    pub title: String,
    //    description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub author: UserBasic,
    pub assignees: Option<Vec<UserBasic>>,
    pub reviewers: Option<Vec<UserBasic>>,
    pub id: u64,
    pub iid: u64,
    pub merge_status: String,
    pub work_in_progress: bool,
    pub web_url: String,
    pub pipeline: Option<Pipeline>,
}

