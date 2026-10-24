// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright 2026 Edgecast Cloud LLC.

//! AlmaLinux cloud-image vendor profile.
//!
//! AlmaLinux publishes GenericCloud qcow2 images per major at
//! `https://repo.almalinux.org/almalinux/<major>/cloud/x86_64/images/`.
//! The directory listing at `/almalinux/` enumerates supported
//! majors (8, 9, 10, …); the `-latest.x86_64.qcow2` filename is a
//! rolling pointer to whatever build is current. The sibling
//! `CHECKSUM` file is Linux-style (`<sha256>  <filename>`), and
//! since the latest pointer and its dated alias share a hash we
//! can resolve the dated form once at metadata time and verify
//! with a plain `Sha256Pinned`.

mod releases;

use anyhow::{Context, Result};
use async_trait::async_trait;
use url::Url;

use super::{
    PinnedQcow2, ResolvedImage, VendorProfile, VersionEntry, checksum_document, detached_signature,
    point_release_of,
};
use crate::verify::SumsStyle;

pub struct Alma;

/// One entry per published major, newest first.
fn catalog(mut majors: Vec<u32>) -> Vec<VersionEntry> {
    majors.sort_unstable_by(|a, b| b.cmp(a));
    majors
        .into_iter()
        .map(|m| VersionEntry {
            token: m.to_string(),
            series: format!("alma{m}"),
            version: m.to_string(),
            title: format!("AlmaLinux {m}"),
            eol_date: None,
            supported: true,
            lts: false,
            dev: None,
            channel: false,
        })
        .collect()
}

#[async_trait]
impl VendorProfile for Alma {
    fn name(&self) -> &str {
        "alma"
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
    let url: Url = resolved.url.parse().context("alma image url")?;
    let point_release = point_release_of(&resolved.build);
    PinnedQcow2 {
        url,
        series: format!("alma{}", resolved.major),
        // Build identifier (e.g. `9.7-20260501`) so distinct
        // rebuilds dedupe in the manifest.
        version: resolved.build,
        description: format!(
            "AlmaLinux {} CloudInit NoCloud compatible image. \
                 Built to run on bhyve virtual machines.",
            resolved.major
        ),
        homepage: "https://almalinux.org/",
        document: checksum_document(
            &resolved.checksum_url,
            &resolved.checksum_filename,
            SumsStyle::Gnu,
        )?,
        point_release,
        signatures: vec![detached_signature(&resolved.checksum_url, ".asc")?],
        sha256: resolved.sha256,
    }
    .into_resolved("alma")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_image_keeps_dated_build_and_checksum_filename() {
        let filename = "AlmaLinux-9-GenericCloud-9.7-20260501.aarch64.qcow2";
        let checksum_url = "https://repo.almalinux.org/almalinux/9/cloud/aarch64/images/CHECKSUM";
        let image = resolved_image(releases::Resolved {
            major: "9".to_string(),
            build: "9.7-20260501".to_string(),
            url: format!("https://repo.almalinux.org/almalinux/9/cloud/aarch64/images/{filename}"),
            sha256: "a".repeat(64),
            checksum_url: checksum_url.to_string(),
            checksum_filename: filename.to_string(),
        })
        .unwrap_or_else(|e| panic!("{e:#}"));
        assert_eq!(image.version, "9.7-20260501");
        assert_eq!(image.facts.point_release.as_deref(), Some("9.7"));
        assert_eq!(
            image.verifier.checksum_source(),
            crate::verify::ChecksumSource::Pinned {
                algorithm: crate::verify::HashAlgorithm::Sha256,
                hash: "a".repeat(64),
                document: Some(
                    checksum_document(checksum_url, filename, SumsStyle::Gnu)
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
