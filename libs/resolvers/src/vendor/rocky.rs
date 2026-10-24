// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright 2026 Edgecast Cloud LLC.

//! Rocky Linux cloud-image vendor profile.
//!
//! Rocky publishes GenericCloud-Base qcow2 images per major at
//! `https://download.rockylinux.org/pub/rocky/<major>/images/x86_64/`,
//! with a per-file BSD-style `.CHECKSUM` sidecar
//! (`SHA256 (filename) = hex`). The release-resolution path fetches
//! both the directory listing and the chosen build's sidecar, so the
//! upstream sha256 is already known by the time the verifier runs —
//! we use a plain `Sha256Pinned` and callers know the hash without
//! downloading the qcow2.

mod releases;

use anyhow::{Context, Result};
use async_trait::async_trait;
use url::Url;

use super::{
    PinnedQcow2, ResolvedImage, VendorProfile, VersionEntry, checksum_document, detached_signature,
    point_release_of,
};
use crate::verify::SumsStyle;

pub struct Rocky;

/// One entry per published major, newest first.
fn catalog(mut majors: Vec<u32>) -> Vec<VersionEntry> {
    majors.sort_unstable_by(|a, b| b.cmp(a));
    majors
        .into_iter()
        .map(|m| VersionEntry {
            token: m.to_string(),
            series: format!("rocky{m}"),
            version: m.to_string(),
            title: format!("Rocky Linux {m}"),
            eol_date: None,
            supported: true,
            lts: false,
            dev: None,
            channel: false,
        })
        .collect()
}

#[async_trait]
impl VendorProfile for Rocky {
    fn name(&self) -> &str {
        "rocky"
    }

    async fn list_versions(&self, http: &reqwest::Client) -> Result<Vec<VersionEntry>> {
        Ok(catalog(releases::list(http).await?))
    }

    async fn resolve_release(
        &self,
        release: &str,
        http: &reqwest::Client,
    ) -> Result<ResolvedImage> {
        let resolved = releases::resolve(http, release).await?;
        resolved_image(resolved)
    }

    async fn resolve_release_arch(
        &self,
        release: &str,
        arch: super::Architecture,
        http: &reqwest::Client,
    ) -> Result<Option<ResolvedImage>> {
        releases::resolve_arch(http, release, arch)
            .await?
            .map(resolved_image)
            .transpose()
    }
}

fn resolved_image(resolved: releases::Resolved) -> Result<ResolvedImage> {
    let url: Url = resolved.url.parse().context("rocky image url")?;
    let point_release = point_release_of(&resolved.build);
    PinnedQcow2 {
        url,
        series: format!("rocky{}", resolved.major),
        version: resolved.build,
        description: format!(
            "Rocky Linux {} CloudInit NoCloud compatible image. \
                 Built to run on bhyve virtual machines.",
            resolved.major
        ),
        homepage: "https://rockylinux.org/",
        document: checksum_document(
            &resolved.checksum_url,
            &resolved.checksum_filename,
            SumsStyle::Bsd,
        )?,
        point_release,
        signatures: vec![detached_signature(&resolved.checksum_url, ".asc")?],
        sha256: resolved.sha256,
    }
    .into_resolved("rocky")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_image_keeps_dated_build_and_checksum_filename() {
        let filename = "Rocky-9-GenericCloud-Base-9.7-20251123.2.aarch64.qcow2";
        let url = format!("https://download.rockylinux.org/pub/rocky/9/images/aarch64/{filename}");
        let checksum_url = format!("{url}.CHECKSUM");
        let image = resolved_image(releases::Resolved {
            major: "9".to_string(),
            build: "9.7-20251123.2".to_string(),
            url,
            sha256: "a".repeat(64),
            checksum_url: checksum_url.clone(),
            checksum_filename: filename.to_string(),
        })
        .unwrap_or_else(|e| panic!("{e:#}"));
        assert_eq!(image.version, "9.7-20251123.2");
        assert_eq!(image.facts.point_release.as_deref(), Some("9.7"));
        assert_eq!(
            image.verifier.checksum_source(),
            crate::verify::ChecksumSource::Pinned {
                algorithm: crate::verify::HashAlgorithm::Sha256,
                hash: "a".repeat(64),
                document: Some(
                    checksum_document(&checksum_url, filename, SumsStyle::Bsd)
                        .unwrap_or_else(|e| panic!("{e:#}"))
                ),
            }
        );
        assert_eq!(image.facts.signatures[0].signs.as_str(), checksum_url);
    }

    #[test]
    fn catalog_is_newest_first() {
        let tokens: Vec<String> = catalog(vec![8, 10, 9])
            .into_iter()
            .map(|e| e.token)
            .collect();
        assert_eq!(tokens, ["10", "9", "8"]);
    }
}
