use std::{convert::Infallible, net::SocketAddr};

use bytes::Bytes;
use hyper::body;
use hyper::service::{make_service_fn, service_fn};
use hyper::{self, Body, Error, Request, Response, Server};
use serde::Deserialize;
use structopt::StructOpt;
use tracing::{debug, error, info, warn};
use tracing_subscriber::{prelude::*, EnvFilter};

mod message;
mod gitlab;
mod webex;

use crate::gitlab::client::GitlabClient;
use crate::gitlab::webhook::process_webhook;
use crate::webex::WebexClient;

async fn send_messages(messages: Vec<message::Message>, webex_client: WebexClient) {

        for message in messages {

            let recipient_email = message.recipient_email.clone();
            let webex_msg = webex::Message::new(message.recipient_email, message.message);
            let webex_client = webex_client.clone();
            match webex_client.send_message(webex_msg).await {
                Ok(_) => info!("Sent assignee message to: {}", recipient_email),
                Err(err) => warn!("Error sending assignee message to {}: {:?}", recipient_email, err),
            }
        }
}

fn handle_webhook(bytes: Bytes, gitlab_client: GitlabClient, webex_client: WebexClient) {

    tokio::spawn(async move {
        let gitlab_client = gitlab_client.clone();
        let messages = match process_webhook(bytes, gitlab_client).await {
            Ok(messages) => messages,
            Err(error) => {
                warn!("Error creating messages from webhook: {}", error);
                return;
            }
        };
        let webex_client = webex_client.clone();
        send_messages(messages, webex_client.clone()).await;
    });
}

async fn handle(request: Request<Body>, gitlab_client: GitlabClient, webex_client: WebexClient) -> Result<Response<Body>, Infallible> {
    let response = Response::new(Body::empty());

    match body::to_bytes(request.into_body()).await {
        Ok(bytes) => handle_webhook(bytes, gitlab_client, webex_client),
        Err(error) => warn!("Error getting request body: {}", error),
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
    access_token: String,
    hostname: String,
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

fn init_tracing() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let opt = Opt::from_args();
    info!("We would start on: {}:{}", opt.address, opt.port);

    let config = Config::new("conf/default")?;

    debug!("Config (now what?): {:?}", config);

    let gitlab_client = GitlabClient::new(config.gitlab.hostname, config.gitlab.access_token);
    let webex_client = WebexClient::new(config.webex.access_token, config.webex.whoami_link);

    let addr_str = format!("{}:{}", opt.address, opt.port);
    let addr: SocketAddr = addr_str.parse().expect("Bad address");

    let make_service = make_service_fn(move |_| {
        let gitlab_client = gitlab_client.clone();
        let webex_client = webex_client.clone();

        async move {
            Ok::<_, Error>(service_fn(move |request: Request<Body>| {
                let gitlab_client = gitlab_client.clone();
                let webex_client = webex_client.clone();
                handle(request, gitlab_client, webex_client)
            }))
        }
    });

    let server = Server::bind(&addr).serve(make_service);

    if let Err(e) = server.await {
        error!("server error: {}", e);
    }

    Ok(())
}
