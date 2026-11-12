//! The optional Walkr address and credential-free probe path.
use crate::{browser::WebUrl, walkr::Error};
use serde::{Deserialize, Deserializer, de::Error as _};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct WalkrConfig {
    #[serde(deserialize_with = "address")]
    pub(crate) url: Option<WebUrl>,
    #[serde(deserialize_with = "probe_path")]
    pub(crate) probe_path: String,
}

impl Default for WalkrConfig {
    fn default() -> Self {
        Self {
            url: None,
            probe_path: "/sdk.js".into(),
        }
    }
}

fn address<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<WebUrl>, D::Error> {
    let value = String::deserialize(deserializer)?;
    let parsed = url::Url::parse(&value).map_err(|_| D::Error::custom(Error::InvalidUrl))?;
    // URL parsing removes empty user info, so inspect the authority too.
    let user_info = value.split_once(':').is_some_and(|(_, rest)| {
        rest.trim_start_matches(['/', '\\'])
            .split(['/', '\\', '?', '#'])
            .next()
            .is_some_and(|authority| authority.contains('@'))
    });
    if user_info
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(D::Error::custom(Error::InvalidUrl));
    }
    WebUrl::try_from(value)
        .map(Some)
        .map_err(|_| D::Error::custom(Error::InvalidUrl))
}

fn probe_path<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let path = String::deserialize(deserializer)?;
    if !path.starts_with('/')
        || path.len() > 256
        || path.contains(['?', '#'])
        || path.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(D::Error::custom(Error::InvalidProbePath));
    }
    Ok(path)
}
