use std::{convert::Infallible, net::SocketAddr};

use chrono::Utc;
use hyper::body;
use hyper::service::{make_service_fn, service_fn};
use hyper::{self, Body, Error, Request, Response, Server, StatusCode};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use structopt::StructOpt;

mod webex;

use crate::webex::WebexClient;

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

fn process_new_assignee(new_assignee: &User, webhook: &MergeRequestWebhook) -> Option<webex::Message> {
    let merge_request = &webhook.merge_request;
    let project = &webhook.project;
    let user = &webhook.user;

    let to_person_email = new_assignee.email.to_owned();
    let markdown = format!(
        "MR [!{mr_iid} {mr_title}]({mr_url}) \
        ([{project_name}]({project_url})) \
        by @{user} \
        🤩 Added as assignee",
        mr_iid=merge_request.iid, mr_title=merge_request.title, mr_url=merge_request.url,
        project_name=project.name, project_url=project.web_url, user=user.username);

    Some(webex::Message::new(
        to_person_email,
        markdown
    ))
}

fn process_pipeline_status(webhook: &PipelineWebhook) -> Option<webex::Message> {
    let pipeline = &webhook.pipeline;
    let project = &webhook.project;
    let user = &webhook.user;

    let to_person_email = user.email.to_owned();
    let status_text = match pipeline.status {
        PipelineStatus::Success => Some("🌞 Success"),
        PipelineStatus::Failed => Some("⛈️ Failed"),
        PipelineStatus::Running => Some("⏳ Running"),
        _ => None,
    }?;
    let markdown = format!(
        "Pipeline [#{pipeline_id} {pipeline_ref}]({project_url}/-/pipelines/{pipeline_id}) \
        {pipeline_status}",
        pipeline_id=pipeline.id, pipeline_ref=pipeline.ref_, project_url=project.web_url,
        pipeline_status=status_text);

    Some(webex::Message::new(
        to_person_email,
        markdown,
    ))
}

async fn handle(request: Request<Body>, webex_client: WebexClient) -> Result<Response<Body>, Infallible> {
    let mut response = Response::new(Body::empty());

    // Takes all data chunks, not just the first one:
    let res = body::to_bytes(request.into_body()).await;
    match res {
        Ok(my_bytest) => {
            let my_string = String::from_utf8(my_bytest.to_vec()).unwrap();
            let v: Value = serde_json::from_str(&my_string).unwrap();
            debug!("{} Request: {}",
                Utc::now().format("%Y-%m-%d %H:%M:%S"),
                serde_json::to_string_pretty(&v).unwrap()
            );
            *response.body_mut() = Body::from(my_string.to_uppercase());
            *response.status_mut() = StatusCode::OK;

            let webhook_msg: Result<Webhook, serde_json::Error> =
                serde_json::from_str(&my_string);
            match webhook_msg {
                Ok(Webhook::MergeRequest(webhook)) => {
                    debug!("Merge Request Webhook: {:?}", webhook);
                    if let Some(assignee_changes) = webhook.get_assignee_changes() {
                        for new_assignee in get_new_assignees(assignee_changes) {
                            if let Some(msg) = process_new_assignee(&new_assignee, &webhook) {

                                match webex_client.clone().send_message(msg).await {
                                    Ok(_) => info!("Sent assignee message to: {}", &new_assignee.email),
                                    Err(err) => warn!("Error sending assignee message to {}: {:?}", &new_assignee.email, err),
                                }
                            }
                        }
                    }
                }
                Ok(Webhook::Pipeline(webhook)) => {
                    debug!("Pipeline Webhook: {:?}", webhook);
                    if let Some(msg) = process_pipeline_status(&webhook) {
                        match webex_client.clone().send_message(msg).await {
                            Ok(_) => info!("Sent pipeline message to: {}", &webhook.user.email),
                            Err(err) => warn!("Error sending pipeline message to {}: {:?}", &webhook.user.email, err),
                        }
                    }
                }
                Err(err) => warn!("Error decoding web hook: {:?}", err),
            }
        }
        Err(error) => warn!("Error: {}", error),
    }

    Ok(response)
}

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(short, long, default_value = "config/default")]
    config: String,

    #[structopt(long, default_value = "127.0.0.1")]
    address: String,

    #[structopt(short, long, default_value = "4001")]
    port: u32,
}

#[derive(Deserialize, Debug)]
struct GitlabConfig {
    webhook_path: Option<String>,
    webhook_token: Option<String>,
}

#[derive(Deserialize, Debug)]
struct WebexConfig {
    access_token: String,
    webhook_path: Option<String>,
    webhook_token: Option<String>,
    whoami_link: Option<String>,
}

#[derive(Deserialize, Debug)]
struct Config {
    gitlab: GitlabConfig,
    webex: WebexConfig,
}

impl Config {
    fn new(filename: &str) -> Result<Self, config::ConfigError> {
        let mut config = config::Config::default();
        config.merge(config::File::with_name(filename))?;
        config.merge(config::Environment::with_prefix("REVBOT").separator("__"))?;

        config.try_into()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::from_args();
    info!("We would start on: {}:{}", opt.address, opt.port);

    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let config = Config::new("conf/default")?;

    debug!("Config (now what?): {:?}", config);

    let webex_client = WebexClient::new(config.webex.access_token, config.webex.whoami_link);

    let addr_str = format!("{}:{}", opt.address, opt.port);
    let addr: SocketAddr = addr_str.parse().expect("Bad address");

    let make_service = make_service_fn(move |_| {
        let webex_client = webex_client.clone();

        async move {
            Ok::<_, Error>(service_fn(move |request: Request<Body>| {
                let webex_client = webex_client.clone();
                handle(request, webex_client)
            }))
        }
    });

    let server = Server::bind(&addr).serve(make_service);

    if let Err(e) = server.await {
        error!("server error: {}", e);
    }

    Ok(())
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
