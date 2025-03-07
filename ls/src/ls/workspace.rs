// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{
    fs,
    path::{Path, PathBuf},
};

use url::Url;

use crate::{ReturnCode, config::Workspace, protocol::Uri};

#[derive(Debug)]
pub struct WorkspaceMembers {
    pub files: Vec<Url>,
    pub missing_roots: Vec<Uri>,
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
