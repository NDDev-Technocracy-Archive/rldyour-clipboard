//! Desktop presentation commands. Opaque decimal keys retain exact archive IDs
//! across the native/webview boundary, including values above JavaScript's range.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum View {
    Recent,
    History,
    Favorites,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum DesktopAction {
    Browse {
        view: View,
        query: String,
        before: Option<String>,
    },
    Favorite {
        key: String,
        pinned: bool,
    },
    Restore {
        key: String,
    },
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DesktopEntry {
    pub key: String,
    #[serde(flatten)]
    pub summary: crate::wire::Summary,
}
impl From<crate::wire::Summary> for DesktopEntry {
    fn from(summary: crate::wire::Summary) -> Self {
        Self {
            key: summary.id.to_string(),
            summary,
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum DesktopResult {
    Page {
        items: Vec<DesktopEntry>,
        more: bool,
    },
    Changed {},
    Restored {},
}

#[cfg(test)]
mod tests {
    #[test]
    fn desktop_keys_preserve_the_entire_archive_identifier() {
        let summary = crate::wire::Summary {
            id: i64::MAX,
            kind: "text",
            mimes: vec!["text/plain".into()],
            bytes: 1,
            preview: None,
            width: None,
            height: None,
            thumb: false,
            pinned: true,
            source: None,
            at: 1,
        };
        let value = serde_json::to_value(super::DesktopEntry::from(summary)).unwrap();
        assert_eq!(value["key"], "9223372036854775807");
    }
}
