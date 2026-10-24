// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright 2026 Edgecast Cloud LLC.

//! The 1.0 promise: within `v1/`, the format changes only compatibly.
//! `fixtures/v1.0/` holds 1.0's OpenAPI document and the examples as
//! they were at 1.0, and is never regenerated (the API crate's
//! `v1_0_schema` tests pin it). A client generated from that document
//! must read today's examples, and a client generated from today's
//! document must read the 1.0 examples.
//!
//! The examples are written by hand; the API crate's tests check that
//! they parse with today's types and pass the tree checks. Parsing
//! cannot see a renamed or retyped optional field, which a client
//! reads as absent; the `v1_0_schema` tests compare the schemas for
//! that.

use std::path::Path;

use cloud_image_index_typify_client::{generated, v1_0};
use serde::de::DeserializeOwned;

fn read<T: DeserializeOwned>(relative: &str) -> Result<T, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("read {path:?}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("parse {path:?}: {e}"))
}

#[test]
fn both_clients_read_mixed_architectures_and_arch_specific_builds() -> Result<(), String> {
    let mut value: serde_json::Value = read("../../examples/v1/images.json")?;
    let images = value["images"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("missing fixture array"));
    let mut arm_only = images[0].clone();
    arm_only["build"]["build"] = "arm-newer".into();
    for a in arm_only["build"]["artifacts"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("missing fixture array"))
    {
        a["arch"] = "aarch64".into();
    }
    let mut arm = arm_only["build"]["artifacts"][0].clone();
    arm["variant"] = "kvm-cloud".into();
    images[0]["build"]["artifacts"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("missing fixture array"))
        .insert(0, arm);
    images.insert(0, arm_only);
    let current: generated::ImageList =
        serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    let old: v1_0::ImageList = serde_json::from_value(value).map_err(|e| e.to_string())?;
    for parsed in [serde_json::to_value(current), serde_json::to_value(old)] {
        let parsed = parsed.map_err(|e| e.to_string())?;
        assert_eq!(parsed["images"][0]["build"]["build"], "arm-newer");
        assert_eq!(
            parsed["images"][0]["build"]["artifacts"][0]["arch"],
            "aarch64"
        );
        assert_eq!(
            parsed["images"][1]["build"]["artifacts"][0]["arch"],
            "aarch64"
        );
        assert_eq!(
            parsed["images"][1]["build"]["artifacts"][0]["variant"],
            "kvm-cloud"
        );
        assert_eq!(
            parsed["images"][1]["build"]["artifacts"][1]["arch"],
            "x86_64"
        );
    }
    Ok(())
}

#[test]
fn a_1_0_client_reads_todays_image_list() -> Result<(), String> {
    let list: v1_0::ImageList = read("../../examples/v1/images.json")?;
    assert!(!list.images.is_empty());
    Ok(())
}

#[test]
fn a_1_0_client_reads_todays_distro_list() -> Result<(), String> {
    let list: v1_0::DistroList = read("../../examples/v1/index.json")?;
    assert!(!list.distros.is_empty());
    Ok(())
}

#[test]
fn todays_client_reads_the_1_0_image_list() -> Result<(), String> {
    let list: generated::ImageList = read("fixtures/v1.0/images.json")?;
    assert!(!list.images.is_empty());
    Ok(())
}

#[test]
fn todays_client_reads_the_1_0_distro_list() -> Result<(), String> {
    let list: generated::DistroList = read("fixtures/v1.0/index.json")?;
    assert!(!list.distros.is_empty());
    Ok(())
}
