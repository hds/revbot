use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, warn};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message {
    #[serde(rename = "toPersonEmail")]
    to_person_email: String,
    markdown: String,
}

impl Message {
    pub fn new(to_person_email: String, markdown: String) -> Self {
        Message {
            to_person_email,
            markdown,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WebexClient {
    access_token: String,
    whoami_link: Option<String>,
}

impl WebexClient {
    pub fn new(access_token: String, whoami_link: Option<String>) -> Self {
        Self {
            access_token,
            whoami_link,
        }
    }

    pub async fn send_message(self, msg: Message) -> Result<(), Box<dyn std::error::Error>> {
        let client = reqwest::Client::new();

        let mut msg = msg.clone();
        if let Some(whoami_link) = self.whoami_link {
            msg.markdown.push_str(&format!(" ([who am I?]({}))", whoami_link));
        }

        debug!("Sending message: {:?}", &msg);
        let res = client.post("https://api.ciscospark.com/v1/messages")
            .json(&msg)
            .bearer_auth(&self.access_token)
            .send()
            .await?;

        match res.json::<Value>().await {
            Ok(json) => debug!("Response body: {}", json),
            Err(err) => warn!("Couldn't parse body to JSON: {}", err),
        }

        Ok(())
    }
}
