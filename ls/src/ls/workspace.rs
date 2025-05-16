// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use url::Url;

use crate::{ReturnCode, config::Workspace, protocol::Uri, t32::SUFFIXES};

#[derive(Debug)]
pub struct WorkspaceMembers {
    pub files: Vec<Url>,
    pub missing_roots: Vec<Uri>,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct FileIndex {
    pub by_filename: HashMap<String, Url>,
    pub conflict_resolutions: Option<(Vec<PathBuf>, Vec<Url>)>,
    pub directories: HashSet<PathBuf>,
}

impl FileIndex {
    pub fn new() -> Self {
        FileIndex {
            by_filename: HashMap::new(),
            conflict_resolutions: None,
            directories: HashSet::new(),
        }
    }
}

pub fn locate_files(workspace: &Workspace, suffixes: &[&str]) -> WorkspaceMembers {
    debug_assert!(
        matches!(workspace, Workspace::Root(Some(_)))
            || matches!(workspace, Workspace::Folders(Some(_)))
    );

    let mut roots: Vec<PathBuf> = Vec::new();
    let mut files: Vec<Url> = Vec::new();
    let mut missing_roots: Vec<Uri> = Vec::new();

    if match workspace {
        Workspace::Root(uri) => uri.is_none(),
        Workspace::Folders(folders) => folders.is_none(),
    } {
        return WorkspaceMembers {
            files,
            missing_roots,
        };
    }

    match workspace {
        Workspace::Root(uri) => {
            let uri = uri.as_ref().unwrap();
            match convert_uri_into_path(&uri) {
                Ok(dir) => roots.push(dir),
                Err(_) => missing_roots.push(uri.to_string()),
            }
        }
        Workspace::Folders(folders) => {
            for folder in folders.as_ref().unwrap() {
                match convert_uri_into_path(&folder.uri) {
                    Ok(dir) => roots.push(dir),
                    Err(_) => missing_roots.push(folder.uri.to_string()),
                }
            }
        }
    }

    for root in roots {
        walk_dir(&root, suffixes, &mut files);
    }
    WorkspaceMembers {
        files,
        missing_roots,
    }
}

pub fn index_files(files: Vec<Url>) -> FileIndex {
    let mut unique_names: HashMap<String, Url> = HashMap::new();
    let mut directories: HashSet<PathBuf> = HashSet::new();
    let mut conflicts: ((Vec<String>, Vec<PathBuf>), Vec<Url>) =
        ((Vec::new(), Vec::new()), Vec::new());

    for uri in files.into_iter() {
        let script = uri.to_file_path();
        if script.is_err() {
            continue;
        }
        let script = script.unwrap();

        let filename = script.file_name();
        if filename.is_none() {
            continue;
        }

        if let Some(dirname) = script.parent() {
            directories.insert(dirname.to_path_buf());
        }

        let filename = filename.unwrap().to_str();
        if filename.is_none() {
            continue;
        }

        let filename = filename.unwrap().to_string();
        debug_assert!(
            SUFFIXES
                .iter()
                .find_map(|s| (*s == script.extension().unwrap().to_str().unwrap()).then_some(s))
                .is_some()
        );

        if unique_names.contains_key(&filename) {
            let conflict = unique_names.remove(&filename).unwrap();

            conflicts.0.0.push(filename.clone());
            conflicts.0.1.push(conflict.to_file_path().unwrap());
            conflicts.1.push(conflict);

            conflicts.0.0.push(filename);
            conflicts.0.1.push(script);
            conflicts.1.push(uri);
        } else if conflicts.0.0.contains(&filename) {
            conflicts.0.0.push(filename);
            conflicts.0.1.push(script);
            conflicts.1.push(uri);
        } else {
            unique_names.insert(filename, uri);
        }
    }

    if conflicts.1.is_empty() {
        FileIndex {
            by_filename: unique_names,
            conflict_resolutions: None,
            directories,
        }
    } else {
        let ((filenames, filepaths), uris) = conflicts;
        let resolution = resolve_path_conflicts(filenames, filepaths, uris);

        FileIndex {
            by_filename: unique_names,
            conflict_resolutions: Some(resolution),
            directories,
        }
    }
}

fn convert_uri_into_path(uri: &str) -> Result<PathBuf, ReturnCode> {
    // If it is not a valid URI, then it might be plain path.
    // A direct path lookup might therefore be successful.
    let dir = match Url::parse(&uri) {
        Ok(url) => match Url::to_file_path(&url) {
            Ok(path) => path,
            Err(_) => std::path::PathBuf::from(uri),
        },
        Err(_) => std::path::PathBuf::from(uri),
    };

    if dir.try_exists().is_ok_and(|p| p) && dir.is_dir() {
        Ok(dir)
    } else {
        Err(ReturnCode::NoInputErr)
    }
}

fn walk_dir(root: &Path, suffixes: &[&str], files: &mut Vec<Url>) {
    if let Ok(it) = fs::read_dir(root) {
        for el in it.filter(|x| x.is_ok()) {
            let el = el.unwrap().path();

            if el.is_symlink() {
                continue;
            }
            if el.is_dir() {
                walk_dir(&el, suffixes, files);
            } else if let Some(ext) = el.extension() {
                if ext.to_str().is_some_and(|x| suffixes.contains(&x)) {
                    files.push(Url::from_file_path(el).expect("Path must be well formed."));
                }
            }
        }
    }
}

fn resolve_path_conflicts(
    filenames: Vec<String>,
    filepaths: Vec<PathBuf>,
    uris: Vec<Url>,
) -> (Vec<PathBuf>, Vec<Url>) {
    debug_assert!(
        filenames.len() == filepaths.len() && filenames.len() == uris.len() && filenames.len() > 0
    );

    let len = uris.len();
    let mut resolution: (Vec<PathBuf>, Vec<Url>) =
        (Vec::with_capacity(len), Vec::with_capacity(len));

    for ((filename, filepath), uri) in filenames.iter().zip(filepaths.iter()).zip(uris.into_iter())
    {
        resolution.0.push(find_shortest_unique_path_suffix(
            filename,
            filepath,
            (&filenames, &filepaths),
        ));
        resolution.1.push(uri);
    }
    resolution
}

fn find_shortest_unique_path_suffix(
    filename: &str,
    filepath: &Path,
    conflicts: (&Vec<String>, &Vec<PathBuf>),
) -> PathBuf {
    let mut num: usize = 0;
    for (name, path) in conflicts.0.iter().zip(conflicts.1.iter()) {
        if name != filename || path == filepath {
            continue;
        }

        for (ii, (a, b)) in path.iter().rev().zip(filepath.iter().rev()).enumerate() {
            num = num.max(ii + 1);

            if a != b {
                break;
            }
        }
    }
    debug_assert!(num > 0);

    let parts: Vec<&OsStr> = filepath.iter().collect();

    let mut suffix = PathBuf::with_capacity(num);
    let begin = parts.len() - num;

    suffix.push(parts[begin]);
    for part in parts[(begin + 1)..].iter() {
        suffix.push(part)
    }
    suffix
}
