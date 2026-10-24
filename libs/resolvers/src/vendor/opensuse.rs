// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright 2026 Edgecast Cloud LLC.

//! openSUSE Leap cloud-image vendor profile.
//!
//! Leap publishes per-version Minimal-VM Cloud qcow2 images at
//! `https://download.opensuse.org/distribution/leap/<X>.<Y>/appliances/`.
//! Versions and their lifecycle state come from openSUSE's
//! `get.opensuse.org/api/v0/distributions.json`; the image in a
//! version's directory is found via MirrorCache's JSON listing
//! (`?json=1`). The sidecar is a Linux-style `.sha256` sibling so
//! we pin the hash at metadata time. Tumbleweed is intentionally skipped — its
//! current `appliances/` directory only ships MicroOS-flavored
//! immutable images (Combustion/Ignition, not cloud-init nocloud).

mod releases;

use anyhow::{Context, Result};
use async_trait::async_trait;
use url::Url;

use super::{PinnedQcow2, ResolvedImage, VendorProfile, VersionEntry, checksum_document};
use crate::verify::SumsStyle;

pub struct OpenSuse;

#[async_trait]
impl VendorProfile for OpenSuse {
    fn name(&self) -> &str {
        "opensuse"
    }

    async fn list_versions(&self, http: &reqwest::Client) -> Result<Vec<VersionEntry>> {
        Ok(releases::catalog(
            &releases::fetch_distributions(http).await?,
        ))
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
    let url: Url = resolved.url.parse().context("opensuse image url")?;
    PinnedQcow2 {
        url,
        series: format!("leap{}", resolved.leap_version),
        version: resolved.build,
        description: format!(
            "openSUSE Leap {} CloudInit NoCloud compatible image. \
                 Built to run on bhyve virtual machines.",
            resolved.leap_version
        ),
        homepage: "https://www.opensuse.org/",
        document: checksum_document(
            &resolved.checksum_url,
            &resolved.checksum_filename,
            SumsStyle::Gnu,
        )?,
        point_release: None,
        signatures: Vec::new(),
        sha256: resolved.sha256,
    }
    .into_resolved("opensuse")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_image_keeps_dated_build_and_checksum_filename() {
        let filename = "Leap-16.0-Minimal-VM.aarch64-Cloud-Build16.143.qcow2";
        let url =
            format!("https://download.opensuse.org/distribution/leap/16.0/appliances/{filename}");
        let checksum_url = format!("{url}.sha256");
        let image = resolved_image(releases::Resolved {
            leap_version: "16.0".to_string(),
            build: "16.0-Build16.143".to_string(),
            url,
            sha256: "a".repeat(64),
            checksum_url: checksum_url.clone(),
            checksum_filename: filename.to_string(),
        })
        .unwrap_or_else(|e| panic!("{e:#}"));
        assert_eq!(image.version, "16.0-Build16.143");
        assert_eq!(
            image.verifier.checksum_source(),
            crate::verify::ChecksumSource::Pinned {
                algorithm: crate::verify::HashAlgorithm::Sha256,
                hash: "a".repeat(64),
                document: Some(
                    checksum_document(&checksum_url, filename, SumsStyle::Gnu)
                        .unwrap_or_else(|e| panic!("{e:#}"))
                ),
            }
        );
    }
}
