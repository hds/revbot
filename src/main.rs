use std::{convert::Infallible, net::SocketAddr};

use chrono::Utc;
use hyper::body;
use hyper::service::{make_service_fn, service_fn};
use hyper::{self, Body, Error, Request, Response, Server, StatusCode};
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

#[derive(Serialize, Deserialize, Clone, Debug)]
struct AssigneeChanges {
    current: Vec<User>,
    previous: Vec<User>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Changes {
    assignees: Option<AssigneeChanges>,
}

#[derive(Serialize, Deserialize, Debug)]
struct MergeRequestAttributes {
    iid: u64,
    merge_status: String,
    title: String,
    url: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Project {
    name: String,
    web_url: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct WebhookMessage {
    assignees: Option<Vec<User>>,
    changes: Option<Changes>,
    object_attributes: Option<MergeRequestAttributes>,
    object_kind: String,
    project: Project,
    user: User,
}

impl WebhookMessage {
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

fn get_new_assignees(assignee_changes: &AssigneeChanges) -> Vec<User> {
    let current_assignees = &assignee_changes.current;
    current_assignees
        .into_iter()
        .filter(|&assignee| !assignee_changes.previous.contains(assignee))
        .map(|a| (a).clone())
        .collect()
}

fn process_new_assignee(new_assignee: &User, webhook: &WebhookMessage) -> Option<webex::Message> {
    let mr_attr = match &webhook.object_attributes {
        Some(mr_attr) => mr_attr,
        None => return None,
    };
    let project = &webhook.project;
    let user = &webhook.user;

    let to_person_email = new_assignee.email.to_owned();
    let markdown = format!(
        "(Test) [!{mr_iid} {mr_title}]({mr_url}) \
        ([{project_name}]({project_url})) \
        by @{user} \
        🤩 Added as assignee",
        mr_iid=mr_attr.iid, mr_title=mr_attr.title, mr_url=mr_attr.url,
        project_name=project.name, project_url=project.web_url, user=user.username);

    Some(webex::Message::new(
        to_person_email,
        markdown
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
            println!(
                "\x1b[0;32m{}\x1b[0m Request: {}",
                Utc::now().format("%Y-%m-%d %H:%M:%S"),
                serde_json::to_string_pretty(&v).unwrap()
            );
            *response.body_mut() = Body::from(my_string.to_uppercase());
            *response.status_mut() = StatusCode::OK;

            let webhook_msg: Result<WebhookMessage, serde_json::Error> =
                serde_json::from_str(&my_string);
            match webhook_msg {
                Ok(webhook_msg) => {
                    println!("Web Hook: {:?}", webhook_msg);
                    if let Some(assignee_changes) = webhook_msg.get_assignee_changes() {
                        for new_assignee in get_new_assignees(assignee_changes) {
                            if let Some(msg) = process_new_assignee(&new_assignee, &webhook_msg) {

                                match webex_client.clone().send_message(&msg).await {
                                    Ok(_) => println!("Sent message to: {}", &new_assignee.email),
                                    Err(err) => println!("Error sending message to {}: {:?}", &new_assignee.email, err),
                                }
                            }
                        }
                    }
                }
                Err(err) => println!("Error decoding web hook: {:?}", err),
            }
        }
        Err(error) => println!("Error: {}", error),
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
    println!("We would start on: {}:{}", opt.address, opt.port);

    let config = Config::new("conf/default")?;

    println!("Config (now what?): {:?}", config);

    let webex_client = WebexClient::new(config.webex.access_token);

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
        eprintln!("server error: {}", e);
    }

    Ok(())
}
