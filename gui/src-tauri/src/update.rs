//! Self-update.
//!
//! Luna ships through the platform installers, so there is no in-app updater to
//! drive. What this module does instead is check the release feed and tell the
//! user when a newer version exists, with a link to the download — which is
//! honest about how the app is distributed and still useful.

use serde::Deserialize;

use crate::dto::UpdateStateDto;

/// The repository releases are published to.
const REPO: &str = "tngklp/luna";

/// One release from the GitHub API.
#[derive(Debug, Clone, Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    prerelease: bool,
}

/// Compare two dotted versions, ignoring a leading `v`.
///
/// Returns true when `candidate` is newer than `current`. A prerelease suffix
/// (`-beta.1`) sorts *below* the release it precedes, so `0.1.0-beta.1` is not
/// newer than `0.1.0`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let split = |value: &str| -> (Vec<u32>, bool) {
        let trimmed = value.trim_start_matches('v');
        // A `-` or `+` starts the prerelease/build metadata, which is not part
        // of the numeric comparison.
        let (numbers, suffix) = match trimmed.split_once(['-', '+']) {
            Some((numbers, suffix)) => (numbers, Some(suffix)),
            None => (trimmed, None),
        };
        let parts = numbers.split('.').map(|part| part.parse::<u32>().unwrap_or(0)).collect();
        (parts, suffix.is_some())
    };

    let (a, a_pre) = split(candidate);
    let (b, b_pre) = split(current);
    for index in 0..a.len().max(b.len()) {
        let left = a.get(index).copied().unwrap_or(0);
        let right = b.get(index).copied().unwrap_or(0);
        if left != right {
            return left > right;
        }
    }
    // Same numbers: a release beats a prerelease of the same version.
    b_pre && !a_pre
}

/// Ask the release feed whether a newer version exists.
pub async fn check(current: &str) -> UpdateStateDto {
    let client = reqwest::Client::builder()
        .user_agent("luna-updater")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();

    let response = client
        .get(format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await;

    let release: Release = match response {
        Ok(response) if response.status().is_success() => match response.json().await {
            Ok(release) => release,
            Err(err) => {
                return UpdateStateDto::error(format!("could not read the release feed: {err}"))
            }
        },
        Ok(response) => {
            return UpdateStateDto::error(format!("the release feed returned {}", response.status()))
        }
        Err(err) => return UpdateStateDto::error(format!("could not reach the release feed: {err}")),
    };

    if release.prerelease || !is_newer(&release.tag_name, current) {
        return UpdateStateDto::up_to_date(current);
    }

    UpdateStateDto::available(&release.tag_name, release.body.unwrap_or_default(), current)
}

/// The page a user goes to in order to install the new version.
pub fn download_url(tag: &str) -> String {
    format!("https://github.com/{REPO}/releases/tag/{tag}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_higher_version_is_newer() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.1.10", "0.1.9"));
    }

    #[test]
    fn the_same_version_is_not_newer() {
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
    }

    #[test]
    fn an_older_version_is_not_newer() {
        assert!(!is_newer("0.1.0", "0.2.0"));
    }

    #[test]
    fn a_prerelease_suffix_does_not_confuse_the_comparison() {
        assert!(is_newer("0.2.0-beta.1", "0.1.0"));
        assert!(!is_newer("0.1.0-beta.1", "0.1.0"));
    }

    #[test]
    fn the_download_url_points_at_the_tag() {
        assert!(download_url("v0.2.0").ends_with("/releases/tag/v0.2.0"));
    }
}
