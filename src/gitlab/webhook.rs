use std::fmt;

use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::message::Message;

#[derive(Serialize, Deserialize, Clone, Debug)]
struct User {
    email: String,
    id: u64,
    name: String,
    username: String,
}

impl PartialEq for User {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct AssigneeChanges {
    current: Vec<User>,
    previous: Vec<User>,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
struct Changes {
    assignees: Option<AssigneeChanges>,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum MergeStatus {
    Unchecked,
    Checking,
    CanBeMerged,
    CannotBeMerged,
    CannotBeMergedRecheck,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
struct MergeRequestAttributes {
    iid: u64,
    merge_status: MergeStatus,
    title: String,
    url: String,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PipelineStatus {
    Created,
    WaitingForResource,
    Preparing,
    Pending,
    Running,
    Success,
    Failed,
    Canceled,
    Skipped,
    Manual,
    Scheduled,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
struct PipelineAttributes {
    finished_at: Option<String>,
    id: u64,
    #[serde(rename = "ref")]
    ref_: String,
    status: PipelineStatus,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
struct Project {
    name: String,
    web_url: String,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
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


#[derive(Debug, Deserialize, PartialEq, Serialize)]
struct PipelineWebhook {
    #[serde(rename = "object_attributes")]
    pipeline: PipelineAttributes,
    project: Project,
    user: User,
}


#[derive(Debug, Deserialize, PartialEq, Serialize)]
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
        "MR [!{mr_iid} {mr_title}]({mr_url}) \
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

fn process_pipeline_status(webhook: &PipelineWebhook) -> Option<Message> {
    let pipeline = &webhook.pipeline;
    let project = &webhook.project;
    let user = &webhook.user;

    let recipient_email = user.email.to_owned();
    let status_text = match pipeline.status {
        PipelineStatus::Success => Some("🌞 Success"),
        PipelineStatus::Failed => Some("⛈️ Failed"),
        PipelineStatus::Running => Some("⏳ Running"),
        _ => None,
    }?;
    let message = format!(
        "Pipeline [#{pipeline_id} {pipeline_ref}]({project_url}/-/pipelines/{pipeline_id}) \
        {pipeline_status}",
        pipeline_id=pipeline.id, pipeline_ref=pipeline.ref_, project_url=project.web_url,
        pipeline_status=status_text);

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

fn process_pipeline(webhook: &PipelineWebhook) -> Result<Vec<Message>, Box<dyn std::error::Error>> {
    match process_pipeline_status(webhook) {
        Some(message) => Ok(vec![message]),
        None => Ok(Vec::new()),
    }
}

#[derive(Clone, Debug)]
struct UnsupportedWebhook;

impl fmt::Display for UnsupportedWebhook {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Unsupported Webhook")
    }
}

impl std::error::Error for UnsupportedWebhook {}

pub fn process_webhook(bytes: Bytes) -> Result<Vec<Message>, Box<dyn std::error::Error>> {
    let string = String::from_utf8(bytes.to_vec())?;
    let webhook: Webhook = serde_json::from_str(&string).map_err(|_| UnsupportedWebhook)?;

    let response = match webhook {
        Webhook::MergeRequest(webhook) => process_merge_request(&webhook),
        Webhook::Pipeline(webhook) => process_pipeline(&webhook),
    };

    response
}

#[cfg(test)]
mod test {
    use super::*;

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
        "url": "https://main.gitlab.in.here.com/stainsby/mr-test/-/merge_requests/3",
        "title": "Fail pipeline"
      },
      "object_kind": "merge_request",
      "project": {
        "name": "mr-test",
        "web_url": "https://main.gitlab.in.here.com/stainsby/mr-test"
      },
      "user": {
        "email": "hayden.stainsby@here.com",
        "id": 1069,
        "name": "Stainsby, Hayden",
        "username": "stainsby"
      }
      }
      "#;

      let expected = Webhook::MergeRequest(MergeRequestWebhook {
          assignees: None,
          changes: None,
          merge_request: MergeRequestAttributes {
              iid: 3,
              merge_status: MergeStatus::Unchecked,
              title: "Fail pipeline".to_owned(),
              url: "https://main.gitlab.in.here.com/stainsby/mr-test/-/merge_requests/3".to_owned(),
          },
          project: Project {
              name: "mr-test".to_owned(),
              web_url: "https://main.gitlab.in.here.com/stainsby/mr-test".to_owned(),
          },
          user: User {
              email: "hayden.stainsby@here.com".to_owned(),
              id: 1069,
              name: "Stainsby, Hayden".to_owned(),
              username: "stainsby".to_owned(),
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
        "name": "mr-test",
        "web_url": "https://main.gitlab.in.here.com/stainsby/mr-test"
      },
      "user": {
        "email": "hayden.stainsby@here.com",
        "id": 1069,
        "name": "Stainsby, Hayden",
        "username": "stainsby"
      }
      }
      "#;

      let expected = Webhook::Pipeline(PipelineWebhook {
          pipeline: PipelineAttributes {
              finished_at: None,
              id: 4038106,
              ref_: "fail-pipeline".to_owned(),
              status: PipelineStatus::Running,
          },
          project: Project {
              name: "mr-test".to_owned(),
              web_url: "https://main.gitlab.in.here.com/stainsby/mr-test".to_owned(),
          },
          user: User {
              email: "hayden.stainsby@here.com".to_owned(),
              id: 1069,
              name: "Stainsby, Hayden".to_owned(),
              username: "stainsby".to_owned(),
          },
      });

      println!("{}", json);
      let webhook: Webhook = serde_json::from_str(&json).unwrap();
      assert_eq!(expected, webhook);
    }

}
