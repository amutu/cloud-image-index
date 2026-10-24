// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright 2026 Edgecast Cloud LLC.

//! FreeBSD release discovery via the VM-IMAGES directory listing.
//!
//! There is no JSON or RFC822 metadata file for FreeBSD releases.
//! The canonical listing is the auto-generated HTML directory index
//! at `https://download.freebsd.org/releases/VM-IMAGES/`. We grep
//! for `<major>.<minor>-RELEASE/` entries and pick the numerically
//! highest one. This is the same algorithm `releng/check-old.sh`
//! and several other FreeBSD release tools use.

use anyhow::{Context, Result};
use regex::Regex;

const VM_IMAGES_URL: &str = "https://download.freebsd.org/releases/VM-IMAGES/";

pub async fn find_latest(http: &reqwest::Client) -> Result<String> {
    eprintln!("Fetching FreeBSD VM-IMAGES directory listing ...");
    let body = http
        .get(VM_IMAGES_URL)
        .send()
        .await
        .with_context(|| format!("GET {VM_IMAGES_URL}"))?
        .error_for_status()
        .with_context(|| format!("status from {VM_IMAGES_URL}"))?
        .text()
        .await
        .with_context(|| format!("read body of {VM_IMAGES_URL}"))?;
    parse_latest_from_html(&body)
        .ok_or_else(|| anyhow::anyhow!("no `X.Y-RELEASE/` entries found in {VM_IMAGES_URL}"))
}

fn parse_latest_from_html(body: &str) -> Option<String> {
    let re = Regex::new(r"\b(\d+)\.(\d+)-RELEASE/").ok()?;
    let mut best: Option<(u32, u32)> = None;
    for cap in re.captures_iter(body) {
        let major: u32 = cap.get(1)?.as_str().parse().ok()?;
        let minor: u32 = cap.get(2)?.as_str().parse().ok()?;
        match best {
            None => best = Some((major, minor)),
            Some(b) if (major, minor) > b => best = Some((major, minor)),
            _ => {}
        }
    }
    best.map(|(maj, min)| format!("{maj}.{min}"))
}

/// List every `X.Y-RELEASE` version in the VM-IMAGES index,
/// newest-first. Same directory the resolver reads for `latest`.
pub async fn list(http: &reqwest::Client) -> Result<Vec<String>> {
    eprintln!("Fetching FreeBSD VM-IMAGES directory listing ...");
    let body = http
        .get(VM_IMAGES_URL)
        .send()
        .await
        .with_context(|| format!("GET {VM_IMAGES_URL}"))?
        .error_for_status()
        .with_context(|| format!("status from {VM_IMAGES_URL}"))?
        .text()
        .await
        .with_context(|| format!("read body of {VM_IMAGES_URL}"))?;
    Ok(parse_all_from_html(&body))
}

fn parse_all_from_html(body: &str) -> Vec<String> {
    let re = match Regex::new(r"\b(\d+)\.(\d+)-RELEASE/") {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let mut versions: Vec<(u32, u32)> = re
        .captures_iter(body)
        .filter_map(|cap| {
            let major = cap.get(1)?.as_str().parse().ok()?;
            let minor = cap.get(2)?.as_str().parse().ok()?;
            Some((major, minor))
        })
        .collect();
    versions.sort_unstable();
    versions.dedup();
    versions.reverse(); // newest-first
    versions
        .into_iter()
        .map(|(maj, min)| format!("{maj}.{min}"))
        .collect()
}

/// Validate and normalize a user-supplied release token. Accepts
/// `X.Y` and `X.Y-RELEASE` and returns just `X.Y`. Other forms
/// (codenames, "current", patch suffixes like `-p1`) are rejected;
/// FreeBSD doesn't publish VM images for those at the same URL.
pub fn parse_version(input: &str) -> Result<String> {
    let s = input.trim();
    let stripped = s.strip_suffix("-RELEASE").unwrap_or(s);
    let parts: Vec<&str> = stripped.split('.').collect();
    if parts.len() != 2 || parts.iter().any(|p| p.parse::<u32>().is_err()) {
        anyhow::bail!("freebsd: expected version like '15.0' or '15.0-RELEASE', got {input:?}");
    }
    Ok(stripped.to_string())
}

pub fn pick_arch_directory(
    body: &str,
    arch: crate::vendor::Architecture,
) -> Result<Option<String>> {
    let re = Regex::new(r#"href="([^"]+)""#)?;
    let candidates: &[&str] = match arch {
        crate::vendor::Architecture::X86_64 => &["amd64/"],
        crate::vendor::Architecture::Aarch64 => &["aarch64/", "arm64/"],
    };
    Ok(candidates
        .iter()
        .find(|name| re.captures_iter(body).any(|c| &c[1] == **name))
        .map(|s| s.to_string()))
}

pub fn pick_cloud_image(
    body: &str,
    version: &str,
    arch: crate::vendor::Architecture,
) -> Result<Option<String>> {
    let machine = match arch {
        crate::vendor::Architecture::X86_64 => "amd64",
        crate::vendor::Architecture::Aarch64 => "arm64-aarch64",
    };
    let filename = format!("FreeBSD-{version}-RELEASE-{machine}-BASIC-CLOUDINIT-zfs.raw.xz");
    let re = Regex::new(r#"href="([^"]+)""#)?;
    Ok(re
        .captures_iter(body)
        .any(|c| c[1] == filename)
        .then_some(filename))
}

pub async fn discover_image(
    http: &reqwest::Client,
    version: &str,
    arch: crate::vendor::Architecture,
) -> Result<Option<(String, String)>> {
    let root = format!("{VM_IMAGES_URL}{version}-RELEASE/");
    let body = fetch_listing(http, &root).await?;
    let Some(directory) = pick_arch_directory(&body, arch)? else {
        return Ok(None);
    };
    let mut base = format!("{root}{directory}");
    let body = fetch_listing(http, &base).await?;
    if directory == "arm64/" {
        let Some(subdir) = pick_arch_directory(&body, arch)? else {
            return Ok(None);
        };
        base.push_str(&subdir);
        let body = fetch_listing(http, &base).await?;
        if !body.contains("href=\"Latest/\"") {
            return Ok(None);
        }
    } else if !body.contains("href=\"Latest/\"") {
        return Ok(None);
    }
    base.push_str("Latest/");
    let listing = fetch_listing(http, &base).await?;
    Ok(pick_cloud_image(&listing, version, arch)?.map(|filename| (base, filename)))
}

async fn fetch_listing(http: &reqwest::Client, url: &str) -> Result<String> {
    http.get(url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("status {url}"))?
        .text()
        .await
        .with_context(|| format!("body {url}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn arm_fixture_discovers_directory_and_exact_basic_cloudinit_zfs_filename() {
        use crate::vendor::Architecture;
        let directories = r#"<a href="amd64/">amd64</a><a href="aarch64/">aarch64</a>"#;
        assert_eq!(
            pick_arch_directory(directories, Architecture::Aarch64)
                .unwrap()
                .as_deref(),
            Some("aarch64/")
        );
        assert_eq!(
            pick_arch_directory(r#"<a href="arm64/">arm64</a>"#, Architecture::Aarch64)
                .unwrap()
                .as_deref(),
            Some("arm64/")
        );
        assert!(
            pick_arch_directory(r#"<a href="amd64/">x</a>"#, Architecture::Aarch64)
                .unwrap()
                .is_none()
        );
        let listing = r#"
<a href="CHECKSUM.SHA256">sums</a>
<a href="FreeBSD-15.0-RELEASE-arm64-aarch64-BASIC-CLOUDINIT-ufs.raw.xz">x</a>
<a href="FreeBSD-15.0-RELEASE-arm64-aarch64-BASIC-CLOUDINIT-zfs.qcow2.xz">x</a>
<a href="FreeBSD-15.0-RELEASE-arm64-aarch64-zfs.raw.xz">x</a>
<a href="FreeBSD-15.0-RELEASE-arm64-aarch64-BASIC-CLOUDINIT-zfs.raw.xz">x</a>
<a href="FreeBSD-15.0-RELEASE-amd64-BASIC-CLOUDINIT-zfs.raw.xz">x</a>
"#;
        assert_eq!(
            pick_cloud_image(listing, "15.0", Architecture::Aarch64)
                .unwrap()
                .as_deref(),
            Some("FreeBSD-15.0-RELEASE-arm64-aarch64-BASIC-CLOUDINIT-zfs.raw.xz")
        );
        assert_eq!(
            pick_cloud_image(listing, "15.0", Architecture::X86_64)
                .unwrap()
                .as_deref(),
            Some("FreeBSD-15.0-RELEASE-amd64-BASIC-CLOUDINIT-zfs.raw.xz")
        );
        assert!(
            pick_cloud_image(listing, "14.4", Architecture::Aarch64)
                .unwrap()
                .is_none()
        );
        assert!(
            pick_cloud_image("<html></html>", "15.0", Architecture::Aarch64)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn parse_latest_picks_highest_version() {
        let body = r#"
            <a href="13.5-RELEASE/">13.5-RELEASE/</a>
            <a href="14.3-RELEASE/">14.3-RELEASE/</a>
            <a href="14.4-RELEASE/">14.4-RELEASE/</a>
            <a href="15.0-RELEASE/">15.0-RELEASE/</a>
        "#;
        assert_eq!(parse_latest_from_html(body).unwrap(), "15.0");
    }

    #[test]
    fn parse_latest_handles_double_digits() {
        let body = "9.3-RELEASE/ 10.4-RELEASE/ 11.4-RELEASE/";
        assert_eq!(parse_latest_from_html(body).unwrap(), "11.4");
    }

    #[test]
    fn parse_latest_returns_none_when_empty() {
        assert!(parse_latest_from_html("").is_none());
        assert!(parse_latest_from_html("nothing here").is_none());
    }

    #[test]
    fn parse_version_accepts_short_form() {
        assert_eq!(parse_version("15.0").unwrap(), "15.0");
    }

    #[test]
    fn parse_version_strips_suffix() {
        assert_eq!(parse_version("15.0-RELEASE").unwrap(), "15.0");
    }

    #[test]
    fn parse_version_rejects_invalid() {
        assert!(parse_version("15").is_err());
        assert!(parse_version("CURRENT").is_err());
        assert!(parse_version("15.0-p1").is_err());
        assert!(parse_version("15.0.1").is_err());
    }

    #[test]
    fn listing_is_newest_first() {
        let body = r#"<a href="14.4-RELEASE/">x</a><a href="15.1-RELEASE/">x</a>
<a href="9.3-RELEASE/">x</a><a href="15.0-RELEASE/">x</a>"#;
        assert_eq!(parse_all_from_html(body), ["15.1", "15.0", "14.4", "9.3"]);
    }
}
