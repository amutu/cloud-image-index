// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright 2026 Edgecast Cloud LLC.

//! Validate the hand-written example tree under `examples/` with the
//! same checks the generator applies to its output.

use std::path::{Path, PathBuf};

use cloud_image_index_api::tree;

fn examples_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

#[test]
fn examples_match_types_and_paths() -> Result<(), String> {
    tree::validate(&examples_root()).map_err(|problems| problems.join("\n"))?;
    Ok(())
}

#[test]
fn mixed_architectures_round_trip_without_schema_changes() -> Result<(), String> {
    use cloud_image_index_api::ImageList;
    let text = std::fs::read_to_string(examples_root().join("v1/images.json"))
        .map_err(|e| e.to_string())?;
    let mut value: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let artifacts = value["images"][0]["build"]["artifacts"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("missing artifacts array"));
    let mut arm = artifacts[0].clone();
    arm["arch"] = "aarch64".into();
    arm["variant"] = "kvm-cloud".into();
    artifacts.insert(0, arm);
    let list: ImageList = serde_json::from_value(value).map_err(|e| e.to_string())?;
    assert_eq!(list.images[0].build.artifacts[0].arch, "aarch64");
    assert_eq!(list.images[0].build.artifacts[1].arch, "x86_64");
    let back = serde_json::to_value(list).map_err(|e| e.to_string())?;
    assert_eq!(
        back["images"][0]["build"]["artifacts"][0]["variant"],
        "kvm-cloud"
    );
    Ok(())
}

#[test]
fn examples_cover_every_file_kind() -> Result<(), String> {
    let tree = tree::validate(&examples_root()).map_err(|problems| problems.join("\n"))?;
    assert!(tree.distro_list.is_some());
    assert!(tree.openapi.is_some());
    assert!(tree.images.is_some());
    assert!(tree.index_html.is_some());
    assert!(tree.docs_html.is_some());
    assert!(!tree.distros.is_empty());
    assert!(!tree.releases.is_empty());
    assert!(!tree.aliases.is_empty());
    Ok(())
}
