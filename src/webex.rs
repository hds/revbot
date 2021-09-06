use log::{debug, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Debug)]
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
}

impl WebexClient {
    pub fn new(access_token: String) -> Self {
        Self {
            access_token,
        }
    }

    pub async fn send_message(self, msg: &Message) -> Result<(), Box<dyn std::error::Error>> {
        let client = reqwest::Client::new();

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
