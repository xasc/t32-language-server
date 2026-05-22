// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{config::T32DefaultDirs, ls::FileIndex, protocol::Uri, t32::SUFFIXES};

#[derive(Debug, PartialEq)]
enum PathPrefixDir {
    HomeDir,
    SystemDir,
    TempDir,
    ScriptDir,
}

#[derive(Debug)]
pub struct PathShorthandDirs<'a> {
    home_dir: Option<&'a Path>,
    script_dir: &'a Path,
    system_dir: Option<&'a Path>,
    temp_dir: Option<&'a Path>,
}

impl<'a> PathShorthandDirs<'a> {
    pub fn new(t32_dirs: &'a T32DefaultDirs, script_dir: &'a Path) -> Self {
        Self {
            home_dir: t32_dirs.home_dir.as_deref(),
            script_dir,
            system_dir: t32_dirs.system_dir.as_deref(),
            temp_dir: t32_dirs.temp_dir.as_deref(),
        }
    }
    fn resolve_shorthand(&self, prefix: &PathPrefixDir) -> Option<&'a Path> {
        match prefix {
            PathPrefixDir::ScriptDir => Some(self.script_dir),
            PathPrefixDir::SystemDir => self.system_dir,
            PathPrefixDir::TempDir => self.temp_dir,
            PathPrefixDir::HomeDir => self.home_dir,
        }
    }
}

pub fn locate_script(
    script: &str,
    files: &FileIndex,
    dirs: &PathShorthandDirs,
) -> Option<Vec<Uri>> {
    let path = split_call_path(script);
    let filename = path.file_name()?.to_str()?;

    if let Some(uri) = matches_script_name(filename, files) {
        return Some(vec![uri]);
    }

    if let Some(prefix) = detect_path_prefix(&path)
        && let Some(dir) = dirs.resolve_shorthand(&prefix)
    {
        let complemented_path = resolve_path_prefix(&path, dir);

        matches_conflict(&complemented_path, files)
    } else {
        matches_conflict(&path, files)
    }
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
    for candidate in find_possible_call_targets(script, files.by_directory.keys()) {
        for (conflict, uri) in conflicts.0.iter().zip(conflicts.1.iter()) {
            if candidate.ends_with(conflict) {
                hits.push(uri.to_string());
            }
        }
    }
    if hits.len() > 0 { Some(hits) } else { None }
}

fn find_possible_call_targets<'a, I>(script: &Path, directories: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = &'a PathBuf>,
{
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

fn resolve_path_prefix(path: &Path, replacement: &Path) -> PathBuf {
    let mut parts = path.components();

    // Drop first segment
    parts.next();
    replacement.join(parts.as_path())
}

fn detect_path_prefix(path: &Path) -> Option<PathPrefixDir> {
    let mut num_tildes: u32 = 0;

    let path_str = path.to_string_lossy();

    for ch in path_str.chars() {
        if ch != '~' {
            break;
        }
        num_tildes += 1;
    }
    debug_assert!(num_tildes <= 4);

    match num_tildes {
        4 => Some(PathPrefixDir::ScriptDir),
        2 => Some(PathPrefixDir::SystemDir),
        1 => Some(PathPrefixDir::HomeDir),
        3 => Some(PathPrefixDir::TempDir),
        _ => None,
    }
}
