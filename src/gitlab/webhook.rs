use std::fmt;

use bytes::Bytes;
use serde::Deserialize;
use serde_json::Value;
use tracing::debug;

use crate::message::Message;
use super::client::GitlabClient;
use super::common::{MergeRequestAttributes, PipelineAttributes, Project, StatusState, User};

#[derive(Clone, Debug)]
struct NotFound;

impl fmt::Display for NotFound {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Details Not Found")
    }
}

impl std::error::Error for NotFound {}

#[derive(Clone, Debug)]
struct UnsupportedWebhook;

impl fmt::Display for UnsupportedWebhook {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Unsupported Webhook")
    }
}

impl std::error::Error for UnsupportedWebhook {}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct AssigneeChanges {
    current: Vec<User>,
    previous: Vec<User>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Changes {
    assignees: Option<AssigneeChanges>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct MergeRequestWebhook {
    assignees: Option<Vec<User>>,
    changes: Option<Changes>,
    #[serde(rename = "object_attributes")]
    merge_request: MergeRequestAttributes,
    project: Project,
    user: User,
}

impl MergeRequestWebhook {
    fn get_assignee_changes(&self) -> Option<&AssigneeChanges> {
        match &self.changes {
            Some(changes) => match &changes.assignees {
                Some(assignee_changes) => return Some(assignee_changes),
                None => return None,
            },
            None => return None,
        }
    }
}


#[derive(Debug, Deserialize, PartialEq)]
struct PipelineWebhook {
    merge_request: Option<MergeRequestAttributes>,
    #[serde(rename = "object_attributes")]
    pipeline: PipelineAttributes,
    project: Project,
    user: User,
}


#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "object_kind", rename_all = "snake_case")]
enum Webhook {
    MergeRequest(MergeRequestWebhook),
    Pipeline(PipelineWebhook),
}

fn get_new_assignees(assignee_changes: &AssigneeChanges) -> Vec<User> {
    let current_assignees = &assignee_changes.current;
    current_assignees
        .into_iter()
        .filter(|&assignee| !assignee_changes.previous.contains(assignee))
        .map(|a| (a).clone())
        .collect()
}

fn process_new_assignee(new_assignee: &User, webhook: &MergeRequestWebhook) -> Option<Message> {
    let merge_request = &webhook.merge_request;
    let project = &webhook.project;
    let user = &webhook.user;

    let recipient_email = new_assignee.email.to_owned();
    let message = format!(
        "[!{mr_iid} {mr_title}]({mr_url}) \
        ([{project_name}]({project_url})) \
        by @{user} \
        🤩 Added as assignee",
        mr_iid=merge_request.iid, mr_title=merge_request.title, mr_url=merge_request.url,
        project_name=project.name, project_url=project.web_url, user=user.username);

    Some(Message {
        recipient_email,
        message,
    })
}

async fn process_pipeline_status(webhook: &PipelineWebhook, gitlab_client: &GitlabClient) -> Option<Message> {
    let pipeline = &webhook.pipeline;
    let project = &webhook.project;
    let user = &webhook.user;

    let recipient_email = user.email.to_owned();
    let status_text = match pipeline.status {
        StatusState::Success => Some("🌞 Success"),
        StatusState::Failed => Some("⛈️ Failed"),
        StatusState::Running => Some("⏳ Running"),
        _ => None,
    }?;


    let gitlab_client = gitlab_client.clone();
    let pipeline_details = gitlab_client.get_pipeline_details(webhook.project.id, webhook.pipeline.id).await?;
    // We intentionally skip pipelines that don't have a merge request attached.
    let merge_request_iid = webhook.merge_request.as_ref()?.iid;
    let merge_request = gitlab_client.get_merge_request_details(webhook.project.id, merge_request_iid).await?;

    let message = format!(
        "[!{mr_iid} {mr_title}]({mr_url}) \
        ([{project_name}]({project_url})) \
        [#{pipeline_id}]({pipeline_url}) \
        {pipeline_status}",
        mr_iid=merge_request.iid, mr_title=merge_request.title, mr_url=merge_request.web_url,
        project_name=project.name, project_url=project.web_url,
        pipeline_id=pipeline.id, pipeline_url=pipeline_details.web_url, pipeline_status=status_text);

    Some(Message {
        recipient_email,
        message,
    })
}

fn process_merge_request(webhook: &MergeRequestWebhook) -> Result<Vec<Message>, Box<dyn std::error::Error>> {
    let mut messages = Vec::<Message>::new();
    if let Some(assignee_changes) = webhook.get_assignee_changes() {
        for new_assignee in get_new_assignees(assignee_changes) {
            if let Some(msg) = process_new_assignee(&new_assignee, &webhook) {
                messages.push(msg);
            }
        }
    }

    Ok(messages)
}

async fn process_pipeline(webhook: &PipelineWebhook, gitlab_client: &GitlabClient) -> Result<Vec<Message>, Box<dyn std::error::Error>> {

    match process_pipeline_status(webhook, gitlab_client).await {
        Some(message) => Ok(vec![message]),
        None => Ok(Vec::new()),
    }
}

pub async fn process_webhook(bytes: Bytes, gitlab_client: GitlabClient) -> Result<Vec<Message>, Box<dyn std::error::Error>> {
    let string = String::from_utf8(bytes.to_vec())?;
    let webhook: Webhook = serde_json::from_str(&string).map_err(|_| UnsupportedWebhook)?;
    let v: Value = serde_json::from_str(&string).unwrap();
    debug!("Received Webhook: {}", serde_json::to_string_pretty(&v).unwrap());

    let response = match webhook {
        Webhook::MergeRequest(webhook) => process_merge_request(&webhook),
        Webhook::Pipeline(webhook) => process_pipeline(&webhook, &gitlab_client).await,
    };

    response
}

#[cfg(test)]
mod test {
    use super::*;
    use gitlab::types::MergeStatus;

    #[test]
    fn test_deserialize_merge_request() {
        let json = r#"
        {
          "object_attributes": {
            "created_at": "2021-09-06 10:54:57 -0500",
            "description": "",
            "id": 289144,
            "iid": 3,
            "merge_error": null,
            "merge_status": "unchecked",
            "merge_when_pipeline_succeeds": false,
            "state": "opened",
            "state_id": 1,
            "url": "https://gitlab.com/hds-/mr-test/-/merge_requests/3",
            "title": "Fail pipeline"
          },
          "object_kind": "merge_request",
          "project": {
            "id": 17898,
            "name": "mr-test",
            "path_with_namespace": "hds-/mr-test",
            "web_url": "https://gitlab.com/hds-/mr-test"
          },
          "user": {
            "email": "hds@example.com",
            "id": 1069,
            "name": "Hayden Stainsby",
            "username": "hds-"
          }
        }
        "#;

      let expected = Webhook::MergeRequest(MergeRequestWebhook {
          assignees: None,
          changes: None,
          merge_request: MergeRequestAttributes {
              action: None,
              iid: 3,
              merge_status: MergeStatus::Unchecked,
              title: "Fail pipeline".to_owned(),
              url: "https://gitlab.com/hds-/mr-test/-/merge_requests/3".to_owned(),
          },
          project: Project {
              id: 17898,
              name: "mr-test".to_owned(),
              path_with_namespace: "hds-/mr-test".to_owned(),
              web_url: "https://gitlab.com/hds-/mr-test".to_owned(),
          },
          user: User {
              email: "hds@example.com".to_owned(),
              id: 1069,
              name: "Hayden Stainsby".to_owned(),
              username: "hds-".to_owned(),
          },
      });

      println!("{}", json);
      let webhook: Webhook = serde_json::from_str(&json).unwrap();
      assert_eq!(expected, webhook);
    }

    #[test]
    fn test_deserialize_pipeline() {
        let json = r#"
        {
          "object_attributes": {
            "finished_at": null,
            "id": 4038106,
            "ref": "fail-pipeline",
            "status": "running"
          },
          "object_kind": "pipeline",
          "project": {
            "id": 17898,
            "name": "mr-test",
            "path_with_namespace": "hds-/mr-test",
            "web_url": "https://gitlab.com/hds-/mr-test"
          },
          "user": {
            "email": "hds@example.com",
            "id": 1069,
            "name": "Hayden Stainsby",
            "username": "hds-"
          }
        }
      "#;

      let expected = Webhook::Pipeline(PipelineWebhook {
          merge_request: None,
          pipeline: PipelineAttributes {
              finished_at: None,
              id: 4038106,
              ref_: "fail-pipeline".to_owned(),
              status: StatusState::Running,
          },
          project: Project {
              id: 17898,
              name: "mr-test".to_owned(),
              path_with_namespace: "hds-/mr-test".to_owned(),
              web_url: "https://gitlab.com/hds-/mr-test".to_owned(),
          },
          user: User {
              email: "hds@example.com".to_owned(),
              id: 1069,
              name: "Hayden Stainsby".to_owned(),
              username: "hds-".to_owned(),
          },
      });

      println!("{}", json);
      let webhook: Webhook = serde_json::from_str(&json).unwrap();
      assert_eq!(expected, webhook);
    }

}
