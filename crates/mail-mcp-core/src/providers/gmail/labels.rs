use crate::error::Result;
use crate::providers::types::{Folder, Label};
use crate::types::{FolderId, LabelId};
use serde::Deserialize;
use super::client::AuthClient;

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn auth_client(server: &MockServer) -> AuthClient {
        AuthClient::new(
            reqwest::Client::new(),
            crate::oauth::ProviderConfig {
                auth_url: "https://x/auth".into(),
                token_url: format!("{}/token", server.uri()),
                client_id: "ci".into(),
                default_scopes: vec![],
            },
            crate::oauth::OAuthTokens {
                access_token: "AT".into(),
                refresh_token: Some("RT".into()),
                expires_at: chrono::Utc::now() + chrono::Duration::seconds(600),
                scope: None,
            },
        )
    }

    #[tokio::test]
    async fn list_labels_returns_user_labels_only() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/labels"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "labels": [
                    {"id":"INBOX","name":"INBOX","type":"system"},
                    {"id":"SENT","name":"SENT","type":"system"},
                    {"id":"Label_1","name":"Followups","type":"user"},
                ]
            })))
            .mount(&server).await;
        let c = auth_client(&server);
        let labels = list_labels_impl(&c, &format!("{}/gmail/v1", server.uri())).await.unwrap();
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].name, "Followups");
    }

    #[tokio::test]
    async fn list_folders_returns_system_labels() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/labels"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "labels": [
                    {"id":"INBOX","name":"INBOX","type":"system"},
                    {"id":"TRASH","name":"TRASH","type":"system"},
                    {"id":"Label_1","name":"Other","type":"user"},
                ]
            })))
            .mount(&server).await;
        let c = auth_client(&server);
        let folders = list_folders_impl(&c, &format!("{}/gmail/v1", server.uri())).await.unwrap();
        let names: Vec<_> = folders.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"INBOX"));
        assert!(names.contains(&"TRASH"));
        assert!(!names.contains(&"Other"));
    }
}

#[derive(Deserialize)]
struct LabelsResponse {
    labels: Vec<RawLabel>,
}

#[derive(Deserialize)]
struct RawLabel {
    id: String,
    name: String,
    #[serde(rename = "type")]
    kind: String,
}

pub async fn list_labels_impl(client: &AuthClient, base: &str) -> Result<Vec<Label>> {
    let url = format!("{base}/users/me/labels");
    let resp = client.get(&url).await?.error_for_status()?.json::<LabelsResponse>().await?;
    Ok(resp.labels.into_iter().filter(|l| l.kind == "user").map(|l| Label {
        id: LabelId::from(l.id),
        name: l.name,
        system: false,
    }).collect())
}

pub async fn list_folders_impl(client: &AuthClient, base: &str) -> Result<Vec<Folder>> {
    let url = format!("{base}/users/me/labels");
    let resp = client.get(&url).await?.error_for_status()?.json::<LabelsResponse>().await?;
    Ok(resp.labels.into_iter().filter(|l| l.kind == "system").map(|l| Folder {
        id: FolderId::from(l.id),
        name: l.name,
        system: true,
    }).collect())
}
