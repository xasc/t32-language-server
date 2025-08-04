// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use url::Url;

use crate::{
    ReturnCode,
    config::Workspace,
    ls::{
        doc::TextDocs,
        tasks::{RenameFileOperations, Task, TaskDone, Tasks, try_schedule},
        workspace::{FileIndex, WorkspaceMembers, index_files, locate_files},
    },
    protocol::FileRename,
    t32::SUFFIXES,
};

pub fn discover_files(
    tasks: &mut Tasks,
    workspace: Workspace,
) -> Result<WorkspaceMembers, ReturnCode> {
    let discover = Task::WorkspaceFileDiscovery(workspace.clone(), &SUFFIXES, locate_files);
    try_schedule(
        &mut tasks.runner,
        discover,
        &mut tasks.ongoing,
        &mut tasks.blocked,
    )?;

    let members = match tasks.runner.rx.recv() {
        Ok(TaskDone::WorkspaceFileDiscovery(m)) => Ok(m),
        Ok(_) => unreachable!("No other tasks must be pending."),
        Err(_) => Err(ReturnCode::UnavailableErr),
    };
    tasks.ongoing.clear();

    members
}

pub fn categorize_files(tasks: &mut Tasks, files: Vec<Url>) -> Result<FileIndex, ReturnCode> {
    let indexer = Task::WorkspaceFileIndexNew(files, index_files);
    try_schedule(
        &mut tasks.runner,
        indexer,
        &mut tasks.ongoing,
        &mut tasks.blocked,
    )?;

    let file_index = match tasks.runner.rx.recv() {
        Ok(TaskDone::WorkspaceFileIndexNew(idx)) => Ok(idx),
        Ok(_) => unreachable!("No other tasks must be pending."),
        Err(_) => Err(ReturnCode::UnavailableErr),
    };
    tasks.ongoing.clear();

    file_index
}

pub fn process_files_did_rename_notif(renamed: Vec<FileRename>, _docs: &mut TextDocs) {
    let _job = Task::DidRenameFiles {
        renamed: RenameFileOperations::from(renamed),
    };
    todo!();
}
