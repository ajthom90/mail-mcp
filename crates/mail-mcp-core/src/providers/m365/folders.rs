// Wired up by Task 8 (provider.rs MailProvider impl) — until then,
// list_*_impl have no non-test callers and the module-level
// `pub use folders::*;` would also be unused. See parse.rs for the same
// pattern.
#![allow(dead_code)]

//! Mail folder + label listing for Microsoft Graph.
//!
//! Graph splits these concepts cleanly: `mailFolders` is the Outlook folder
//! tree (Inbox, Sent Items, Deleted Items, Drafts, Junk, Archive, …) and
//! `outlook/masterCategories` is the user-defined "category" list — the
//! closest Graph analogue to Gmail labels. We surface only the well-known
//! folders (the ones with a `wellKnownName`) so the tray's folder picker
//! stays predictable; user-created sub-folders get exposed in a later task
//! when the search-by-folder feature lands.

use crate::error::Result;
use crate::providers::gmail::AuthClient;
use crate::providers::types::{Folder, Label};
use crate::types::{FolderId, LabelId};
use serde::Deserialize;

const WELL_KNOWN_FOLDERS: &[&str] = &[
    "inbox",
    "sentitems",
    "deleteditems",
    "drafts",
    "junkemail",
    "archive",
];

#[derive(Deserialize)]
struct ListResponse<T> {
    value: Vec<T>,
}

#[derive(Deserialize)]
struct RawFolder {
    #[serde(rename = "displayName", default)]
    display_name: String,
    #[serde(rename = "wellKnownName", default)]
    well_known_name: Option<String>,
}

#[derive(Deserialize)]
struct RawCategory {
    #[serde(default)]
    id: String,
    #[serde(rename = "displayName", default)]
    display_name: String,
}

pub async fn list_folders_impl(client: &AuthClient, base: &str) -> Result<Vec<Folder>> {
    let url = format!("{base}/me/mailFolders?$top=100");
    let resp = client
        .get(&url)
        .await?
        .error_for_status()?
        .json::<ListResponse<RawFolder>>()
        .await?;
    Ok(resp
        .value
        .into_iter()
        .filter_map(|f| {
            let wkn = f.well_known_name?;
            if !WELL_KNOWN_FOLDERS.contains(&wkn.as_str()) {
                return None;
            }
            // Use the well-known name as the stable folder id so
            // mark_read("inbox") + similar callers don't depend on the
            // tenant-specific Graph object id.
            Some(Folder {
                id: FolderId::from(wkn),
                name: f.display_name,
                system: true,
            })
        })
        .collect())
}

pub async fn list_labels_impl(client: &AuthClient, base: &str) -> Result<Vec<Label>> {
    let url = format!("{base}/me/outlook/masterCategories");
    let resp = client
        .get(&url)
        .await?
        .error_for_status()?
        .json::<ListResponse<RawCategory>>()
        .await?;
    Ok(resp
        .value
        .into_iter()
        .map(|c| Label {
            id: LabelId::from(c.id),
            name: c.display_name,
            system: false,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
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
    async fn list_folders_filters_to_well_known_only() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1.0/me/mailFolders"))
            .and(query_param("$top", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": [
                    {"id":"AA","displayName":"Inbox","wellKnownName":"inbox"},
                    {"id":"BB","displayName":"Sent Items","wellKnownName":"sentitems"},
                    {"id":"CC","displayName":"My Custom Folder"},
                    {"id":"DD","displayName":"recoverableitemsdeletions","wellKnownName":"recoverableitemsdeletions"}
                ]
            })))
            .mount(&server)
            .await;
        let c = auth_client(&server);
        let folders = list_folders_impl(&c, &format!("{}/v1.0", server.uri()))
            .await
            .unwrap();
        let names: Vec<_> = folders.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"Inbox"));
        assert!(names.contains(&"Sent Items"));
        assert!(!names.contains(&"My Custom Folder"));
        assert!(!names.contains(&"recoverableitemsdeletions"));
        // Folder id is the well-known name, not the tenant id.
        let inbox = folders.iter().find(|f| f.name == "Inbox").unwrap();
        assert_eq!(inbox.id.as_str(), "inbox");
    }

    #[tokio::test]
    async fn list_labels_returns_user_categories() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1.0/me/outlook/masterCategories"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": [
                    {"id":"cat-1","displayName":"VIP","color":"preset0"},
                    {"id":"cat-2","displayName":"Followups","color":"preset3"}
                ]
            })))
            .mount(&server)
            .await;
        let c = auth_client(&server);
        let labels = list_labels_impl(&c, &format!("{}/v1.0", server.uri()))
            .await
            .unwrap();
        let names: Vec<_> = labels.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains(&"VIP"));
        assert!(names.contains(&"Followups"));
        assert!(labels.iter().all(|l| !l.system));
    }
}
