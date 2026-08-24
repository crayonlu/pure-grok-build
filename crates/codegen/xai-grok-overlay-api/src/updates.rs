use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    Stable,
    Nightly,
    Enterprise,
}

impl Default for UpdateChannel {
    fn default() -> Self {
        Self::Stable
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateSourceRef {
    pub kind: String,
    pub location: String,
    pub channel: UpdateChannel,
}

impl UpdateSourceRef {
    pub fn github_release(repo: impl Into<String>, channel: UpdateChannel) -> Self {
        Self {
            kind: "github_release".to_owned(),
            location: repo.into(),
            channel,
        }
    }

    pub fn base_url(base_url: impl Into<String>, channel: UpdateChannel) -> Self {
        Self {
            kind: "base_url".to_owned(),
            location: base_url.into(),
            channel,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_source_is_provider_neutral() {
        let source = UpdateSourceRef::base_url(
            "https://downloads.example.test/grok",
            UpdateChannel::Nightly,
        );
        let json = serde_json::to_value(source).expect("serialize update source");

        assert_eq!(json["kind"], "base_url");
        assert_eq!(json["channel"], "nightly");
    }
}
