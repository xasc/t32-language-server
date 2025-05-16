// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use crate::{ls::FileIndex, protocol::Uri, t32::SUFFIXES};

pub fn locate_script(script: &str, files: &FileIndex) -> Option<Vec<Uri>> {
    let path = split_call_path(script);
    let filename = path.file_name()?.to_str()?;

    if let Some(uri) = matches_script_name(filename, files) {
        return Some(vec![uri]);
    }
    matches_conflict(&path, files)
}

fn split_call_path(script: &str) -> PathBuf {
    // Scripts paths can use both forward and backward slashes as separators.
    // Backward slashes cannot be used in escape sequences.
    if script.contains('/') {
        PathBuf::from_iter(script.split('/'))
    } else {
        PathBuf::from_iter(script.split('\\'))
    }
}

fn matches_script_name(filename: &str, files: &FileIndex) -> Option<String> {
    let mut variants: Vec<String> = Vec::with_capacity(SUFFIXES.len());

    // PRACTICE script calls accept the filename without "cmm" file extension.
    if SUFFIXES
        .iter()
        .any(|ext| filename.ends_with(&format!(".{}", ext)))
    {
        variants.push(filename.to_string());
    } else {
        SUFFIXES
            .iter()
            .for_each(|ext| variants.push(format!("{}{}", filename, ext)));
    };

    if let Some(uri) = variants.iter().find_map(|v| files.by_filename.get(v)) {
        Some(uri.to_string())
    } else {
        None
    }
}

fn matches_conflict(script: &Path, files: &FileIndex) -> Option<Vec<String>> {
    let conflicts = files.conflict_resolutions.as_ref()?;

    let mut hits: Vec<Uri> = Vec::new();
    for candidate in find_possible_call_targets(script, &files.directories) {
        for (conflict, uri) in conflicts.0.iter().zip(conflicts.1.iter()) {
            if candidate.ends_with(conflict) {
                hits.push(uri.to_string());
            }
        }
    }
    if hits.len() > 0 { Some(hits) } else { None }
}

fn find_possible_call_targets(script: &Path, directories: &HashSet<PathBuf>) -> Vec<PathBuf> {
    if script.is_absolute() {
        return vec![script.to_path_buf()];
    } else {
        // Resolve relative script calls by finding all combinations that
        // result in an existing path. For complementing the relative script
        // path we are using all directories with PRACTICE scripts in the
        // workspace.
        let mut valid: Vec<PathBuf> = Vec::new();
        for dir in directories {
            if let Ok(filepath) = fs::canonicalize(dir.join(script)) {
                if !valid.contains(&filepath) {
                    valid.push(filepath)
                }
            }
        }
        valid
    }
}
