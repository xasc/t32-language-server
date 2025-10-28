// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use url::Url;

use crate::{
    ReturnCode, config::Workspace, ls::tasks::RenameFileOperations, protocol::Uri, t32::SUFFIXES,
};

#[derive(Debug)]
pub struct WorkspaceMembers {
    pub files: Vec<Url>,
    pub missing_roots: Vec<Uri>,
}

#[derive(Clone, Debug)]
pub struct FileIndex {
    pub by_filename: BTreeMap<String, Url>,
    pub by_directory: BTreeMap<PathBuf, Vec<String>>,
    pub conflict_resolutions: Option<(Vec<PathBuf>, Vec<Url>)>,
}

#[derive(Debug)]
pub struct ResolvedRenameFileOperations {
    pub renamed: RenameFileOperations,
    pub missing_files: Vec<Uri>,
    pub missing_dirs: Vec<Uri>,
}

impl FileIndex {
    pub fn new() -> Self {
        FileIndex {
            by_filename: BTreeMap::new(),
            by_directory: BTreeMap::new(),
            conflict_resolutions: None,
        }
    }

    pub fn update(&mut self, renamed: &RenameFileOperations) {
        debug_assert_eq!(renamed.old.len(), renamed.new.len());

        let mut unresolved: Vec<PathBuf> = Vec::new();
        for (old, new) in renamed.old.iter().zip(renamed.new.iter()) {
            let old_path = convert_uri_to_path(old);
            let new_path = convert_uri_to_path(new);

            if let Some(idx) = unresolved.iter().position(|u| *u == old_path) {
                unresolved.swap_remove(idx);
            }

            match self.update_lookup_by_filename(&old_path, &new_path) {
                Some(mut deferred) => {
                    debug_assert!(
                        !self
                            .by_filename
                            .contains_key(new_path.file_name().unwrap().to_str().unwrap())
                    );
                    unresolved.append(&mut deferred);
                }
                None => (),
            }
            self.update_lookup_by_directory(&old_path, &new_path);
        }

        if unresolved.is_empty() {
            return;
        } else if let Some(resolutions) = &self.conflict_resolutions {
            for uri in resolutions.1.iter() {
                unresolved.push(uri.to_file_path().expect("Must be well formed."));
            }
        }
        self.update_conflict_resolutions(unresolved);
    }

    pub fn contains(&self, file: &Path) -> bool {
        let filename = if let Some(name) = file.file_name()
            && let Some(name) = name.to_str()
        {
            name
        } else {
            return false;
        };

        if let Some(uri) = self.by_filename.get(filename)
            && uri.to_file_path().expect("Must contain file path.") == file
        {
            return true;
        }

        if let Some(resolutions) = &self.conflict_resolutions {
            resolutions
                .0
                .iter()
                .position(|p| file.ends_with(p))
                .is_some()
        } else {
            false
        }
    }

    #[expect(unused)]
    fn remove(&mut self, file: &Path) {
        let dir = file
            .parent()
            .expect("Must be absolute file path.")
            .to_path_buf();
        let filename = file
            .file_name()
            .expect("Must contain filename.")
            .to_str()
            .unwrap();

        if self.by_filename.remove(filename).is_none()
            && let Some(resolutions) = &mut self.conflict_resolutions
        {
            if let Some(idx) = resolutions.0.iter().position(|p| file.ends_with(p)) {
                resolutions.0.swap_remove(idx);
                resolutions.1.swap_remove(idx);

                if resolutions.0.is_empty() {
                    self.conflict_resolutions = None;
                }
            }
        }

        if let Some(files) = self.by_directory.get_mut(&dir)
            && let Some(idx) = files.iter().position(|f| *f == filename)
        {
            files.swap_remove(idx);
            if files.is_empty() {
                self.by_directory.remove(&dir);
            }
        }
    }

    fn update_lookup_by_filename(
        &mut self,
        old_path: &Path,
        new_path: &Path,
    ) -> Option<Vec<PathBuf>> {
        let old_filename = old_path
            .file_name()
            .expect("Path must have been validated.")
            .to_str()
            .unwrap();
        let new_filename = new_path
            .file_name()
            .expect("Path must have been validated.")
            .to_str()
            .unwrap();

        // No new conflicts can be introduced, because the filename remains the same.
        // Existing conflicts need to be checked though...
        if old_filename == new_filename {
            if let Some(uri) = self.by_filename.get_mut(old_filename) {
                let path = Url::from_file_path(new_path).expect("Must be well formed.");
                if path != *uri {
                    *uri = path;
                }
                None
            } else if let Some(resolutions) = &mut self.conflict_resolutions
                && let Some(ii) = resolutions.0.iter().position(|p| old_path.ends_with(p))
            {
                resolutions.0.swap_remove(ii);
                if resolutions.0.is_empty() {
                    self.conflict_resolutions = None;
                } else {
                    resolutions.1.swap_remove(ii);
                }
                Some(vec![new_path.to_path_buf()])
            } else {
                unreachable!("Misses must have been removed earlier.");
            }
        }
        // New script filename creates a new conflict.
        else if let Some(duplicate) = self.by_filename.remove(new_filename) {
            if self.by_filename.remove(old_filename).is_none() {
                if let Some(resolutions) = &mut self.conflict_resolutions
                    && let Some(ii) = resolutions.0.iter().position(|p| old_path.ends_with(p))
                {
                    resolutions.0.swap_remove(ii);
                    if resolutions.0.is_empty() {
                        self.conflict_resolutions = None;
                    } else {
                        resolutions.1.swap_remove(ii);
                    }
                } else {
                    unreachable!("Misses must have been removed earlier.");
                }
            }
            Some(vec![
                duplicate.to_file_path().expect("Must be well formed."),
                new_path.to_path_buf(),
            ])
        }
        // New script filename may create a conflict.
        else {
            if self.by_filename.remove(old_filename).is_none() {
                if let Some(resolutions) = &mut self.conflict_resolutions
                    && let Some(ii) = resolutions.0.iter().position(|p| old_path.ends_with(p))
                {
                    resolutions.0.swap_remove(ii);
                    if resolutions.0.is_empty() {
                        self.conflict_resolutions = None;
                    } else {
                        resolutions.1.swap_remove(ii);
                    }
                }
            }
            Some(vec![new_path.to_path_buf()])
        }
    }

    fn update_lookup_by_directory(&mut self, old: &Path, new: &Path) {
        let old_dir = old.parent().expect("Must be absolute file path.");
        let new_dir = new.parent().expect("Must be absolute file path.");

        let old_filename = old
            .file_name()
            .expect("Must be absolute file path.")
            .to_str()
            .unwrap();

        let new_filename = new
            .file_name()
            .expect("Must be absolute file path.")
            .to_str()
            .unwrap();

        if old_dir == new_dir {
            if let Some(files) = self.by_directory.get_mut(old_dir) {
                if files.iter().any(|f| *f == *new_filename) {
                    return;
                }

                if let Some(idx) = files.iter().position(|p| *p == old_filename) {
                    if old_filename != new_filename {
                        files[idx] = new_filename.to_string();
                    }
                } else {
                    unreachable!("Misses must have been removed earlier.");
                }
            }
        } else if let Some(files) = self.by_directory.get_mut(old_dir) {
            if let Some(idx) = files.iter().position(|p| *p == old_filename) {
                files.swap_remove(idx);
            } else {
                unreachable!("Misses must have been removed earlier.");
            }

            if let Some(files) = self.by_directory.get_mut(new_dir) {
                if files.iter().any(|f| *f == *new_filename) {
                    return;
                }
                files.push(new_filename.to_string());
            } else {
                self.by_directory
                    .insert(new_dir.to_path_buf(), vec![new_filename.to_string()]);
            }
        } else {
            unreachable!("Misses must have been removed earlier.");
        }
    }

    fn update_conflict_resolutions(&mut self, mut files: Vec<PathBuf>) {
        let mut filenames: Vec<String> = Vec::with_capacity(files.len());
        let mut uris: Vec<Url> = Vec::with_capacity(files.len());

        for file in files.iter() {
            filenames.push(
                file.file_name()
                    .expect("Must be valid file path.")
                    .to_str()
                    .unwrap()
                    .to_string(),
            );
            uris.push(Url::from_file_path(file).expect("Must be well formed."));
        }

        let mut keep: Vec<u8> = Vec::with_capacity(filenames.len());
        for (ii, name) in filenames.iter().enumerate() {
            let num = filenames.iter().filter(|f| *f == name).count();
            debug_assert!(num > 0);

            keep.push((num - 1) as u8);
            if num == 1 {
                self.by_filename.insert(name.clone(), uris[ii].clone());
            }
        }

        let mut idx: usize = 0;
        filenames.retain(|_| {
            idx += 1;
            keep[idx - 1] != 0
        });

        let mut idx: usize = 0;
        files.retain(|_| {
            idx += 1;
            keep[idx - 1] != 0
        });

        let mut idx: usize = 0;
        uris.retain(|_| {
            idx += 1;
            keep[idx - 1] != 0
        });

        debug_assert_eq!(filenames.len(), files.len());
        debug_assert_eq!(filenames.len(), uris.len());

        if filenames.is_empty() {
            self.conflict_resolutions = None;
        } else {
            self.conflict_resolutions = Some(resolve_path_conflicts(filenames, files, uris));
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
            match convert_uri_into_dir_path(&uri) {
                Ok(dir) => roots.push(dir),
                Err(_) => missing_roots.push(uri.to_string()),
            }
        }
        Workspace::Folders(folders) => {
            for folder in folders.as_ref().unwrap() {
                match convert_uri_into_dir_path(&folder.uri) {
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
    let mut unique_names: BTreeMap<String, Url> = BTreeMap::new();
    let mut directories: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
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

        if let Some(dirname) = script.parent() {
            if directories.contains_key(dirname) {
                let files = directories.get_mut(dirname).unwrap();
                files.push(filename.clone());
            } else {
                directories.insert(dirname.to_path_buf(), vec![filename.clone()]);
            }
        }

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
            by_directory: directories,
            conflict_resolutions: None,
        }
    } else {
        let ((filenames, filepaths), uris) = conflicts;
        let resolution = resolve_path_conflicts(filenames, filepaths, uris);

        FileIndex {
            by_filename: unique_names,
            by_directory: directories,
            conflict_resolutions: Some(resolution),
        }
    }
}

pub fn rename_files(
    renamed: RenameFileOperations,
    file_idx: &mut FileIndex,
) -> ResolvedRenameFileOperations {
    debug_assert_eq!(renamed.old.len(), renamed.new.len());

    if renamed.old.is_empty() {
        return ResolvedRenameFileOperations {
            renamed: RenameFileOperations {
                old: Vec::with_capacity(renamed.old.len()),
                new: Vec::with_capacity(renamed.old.len()),
            },
            missing_dirs: Vec::new(),
            missing_files: Vec::new(),
        };
    }

    let changes = resolve_rename_operations(renamed, file_idx);
    file_idx.update(&changes.renamed);

    changes
}

fn convert_uri_into_dir_path(uri: &str) -> Result<PathBuf, ReturnCode> {
    let dir = convert_uri_to_path(uri);
    if dir.try_exists().is_ok_and(|p| p) && dir.is_dir() {
        Ok(dir)
    } else {
        Err(ReturnCode::NoInputErr)
    }
}

fn convert_uri_to_path(uri: &str) -> PathBuf {
    // If it is not a valid URI, then it might be plain path.
    // A direct path lookup might therefore be successful.
    match Url::parse(&uri) {
        Ok(url) => match Url::to_file_path(&url) {
            Ok(path) => path,
            Err(_) => std::path::PathBuf::from(uri),
        },
        Err(_) => std::path::PathBuf::from(uri),
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

fn resolve_rename_operations(
    renamed: RenameFileOperations,
    file_idx: &mut FileIndex,
) -> ResolvedRenameFileOperations {
    debug_assert!(!renamed.old.is_empty());
    debug_assert_eq!(renamed.old.len(), renamed.new.len());

    let mut changes = RenameFileOperations {
        old: Vec::with_capacity(renamed.old.len()),
        new: Vec::with_capacity(renamed.old.len()),
    };
    let mut missing_files: Vec<Uri> = Vec::new();
    let mut missing_dirs: Vec<Uri> = Vec::new();

    for (old, new) in renamed.old.into_iter().zip(renamed.new.into_iter()) {
        // Based on the reported server capabilities we should get valid
        // URIs from the client.
        let path = convert_uri_to_path(&old);
        if path
            .extension()
            .is_some_and(|ext| ext.to_str().is_some_and(|e| SUFFIXES.contains(&e)))
        {
            if file_idx.contains(&path) {
                changes.old.push(old);
                changes.new.push(new);
            } else {
                missing_files.push(old);
            }
        } else if let Some(files) = file_idx.by_directory.get(&path) {
            for file in files {
                changes.old.push(
                    Url::from_file_path(path.join(file))
                        .expect("Components must be well formed.")
                        .to_string(),
                );

                let path = convert_uri_to_path(&new);
                changes.new.push(
                    Url::from_file_path(path.join(file))
                        .expect("Components must be well formed.")
                        .to_string(),
                );
            }
        } else {
            missing_dirs.push(old);
        }
    }
    debug_assert_eq!(changes.old.len(), changes.new.len());

    ResolvedRenameFileOperations {
        renamed: changes,
        missing_files,
        missing_dirs,
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use std::path;

    use url::Url;

    fn files() -> Vec<Url> {
        vec![
            Url::from_file_path(path::absolute("tests/samples/c.cmm").expect("File must exist."))
                .unwrap(),
            Url::from_file_path(
                path::absolute("tests/samples/same.cmm").expect("File must exist."),
            )
            .unwrap(),
            Url::from_file_path(path::absolute("tests/samples/a/a.cmm").expect("File must exist."))
                .unwrap(),
            Url::from_file_path(
                path::absolute("tests/samples/a/same.cmm").expect("File must exist."),
            )
            .unwrap(),
            Url::from_file_path(
                path::absolute("tests/samples/a/d/d.cmmt").expect("File must exist."),
            )
            .unwrap(),
            Url::from_file_path(path::absolute("tests/samples/b/b.cmm").expect("File must exist."))
                .unwrap(),
            Url::from_file_path(
                path::absolute("tests/samples/b/same.cmm").expect("File must exist."),
            )
            .unwrap(),
        ]
    }

    fn create_file_idx() -> FileIndex {
        let files = files();
        index_files(files)
    }

    #[test]
    fn can_update_file_idx() {
        let mut file_idx = create_file_idx();

        let old_files = [
            "tests/samples/c.cmm",
            "tests/samples/same.cmm",
            "tests/samples/a/a.cmm",
            "tests/samples/a/d/d.cmmt",
        ];
        let old_dir = "tests/samples/b";

        let new_files = [
            "tests/samples/c1.cmm",
            "tests/samples/a.cmm",
            "tests/samples/a/a1.cmm",
            "tests/samples/a/d/d1.cmmt",
        ];
        let new_dir = "tests/samples/b1";

        let mut renamed = RenameFileOperations {
            old: Vec::with_capacity(old_files.len() + old_dir.len()),
            new: Vec::with_capacity(new_files.len() + new_dir.len()),
        };

        for (old, new) in old_files.iter().zip(new_files.iter()) {
            renamed.old.push(
                Url::from_file_path(path::absolute(old).expect("File must exist."))
                    .unwrap()
                    .to_string(),
            );
            renamed.new.push(
                Url::from_file_path(path::absolute(new).expect("File must exist."))
                    .unwrap()
                    .to_string(),
            );
        }

        renamed.old.push(
            Url::from_directory_path(path::absolute(old_dir).expect("Directory must exist."))
                .unwrap()
                .to_string(),
        );
        renamed.new.push(
            Url::from_directory_path(path::absolute(new_dir).expect("Directory must exist."))
                .unwrap()
                .to_string(),
        );

        let changes = rename_files(renamed.clone(), &mut file_idx);

        for new in new_files
            .iter()
            .chain(["tests/samples/b1/b.cmm", "tests/samples/b1/same.cmm"].iter())
        {
            let path = path::absolute(&new).expect("File must exist.");
            let target = Url::from_file_path(path.clone()).unwrap().to_string();

            assert!(changes.renamed.new.contains(&target));
            assert!(file_idx.contains(&path));
        }

        for old in old_files
            .iter()
            .chain(["tests/samples/b/b.cmm", "tests/samples/b/same.cmm"].iter())
        {
            let path = path::absolute(&old).expect("File must exist.");
            let target = Url::from_file_path(path.clone()).unwrap().to_string();

            assert!(changes.renamed.old.contains(&target));
            assert!(!file_idx.contains(&path));
        }
    }
}
