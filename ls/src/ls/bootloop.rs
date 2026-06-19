// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::time::Instant;

use crate::{
    ReturnCode,
    config::Config,
    ls::{
        self, ErrorResponse, FileIndex, InitState, Message, Response, RunState, Tasks, TextDocs,
        Workspace,
        tasks::{self, TaskDone, startup},
        transport::StdioChannel,
    },
    protocol::TraceValue,
};
pub fn start(
    mut g: InitState,
    channel: &mut StdioChannel,
    cfg: &mut Config,
) -> Result<(RunState, Vec<Option<Message>>), ReturnCode> {
    debug_assert_eq!(g.tasks.ongoing.len(), 0);

    match cfg.workspace {
        Workspace::Root(Some(_)) | Workspace::Folders(Some(_)) => (),
        _ => {
            return Ok((
                RunState::from_init(g, TextDocs::from_workspace(Vec::new()), FileIndex::new()),
                Vec::new(),
            ));
        }
    }

    let mut outgoing: Vec<Option<Message>> = Vec::new();

    startup::discover_files(
        cfg,
        cfg.workspace.clone(),
        &mut g.tasks.counters,
        &mut g.tasks.ongoing,
        &mut outgoing,
    );

    let mut incoming: Vec<Option<Message>> = Vec::new();
    let mut postponed: Vec<Option<Message>> = Vec::new();

    let files: FileIndex = loop {
        g.backoff.idle(&Instant::now());

        if g.tasks.runner.aborted() {
            channel.send_msg(Message::Response(Response::ErrorResponse(ErrorResponse {
                id: None,
                error: ls::error_task_queue_abort(),
            })));
            return Err(ReturnCode::SoftwareErr);
        }

        if ls::recv_incoming(cfg.trace_level, channel, &mut g.heartbeat, &mut incoming)? {
            g.backoff.clear();
        }

        tasks::recv_responses(cfg.trace_level, &mut incoming, &mut g.tasks, &mut outgoing);

        if recv_completed_tasks(&cfg, &mut g.tasks, &mut outgoing)? {
            g.backoff.clear();
        }

        let files = schedule_tasks(&mut incoming, &mut g, cfg, &mut outgoing, &mut postponed)?;

        if !outgoing.is_empty() {
            g.backoff.clear();
        }
        ls::send_outgoing(channel, &mut outgoing);

        if g.exit_requested {
            return Err(if g.shutdown_request_recv {
                ReturnCode::OkExit
            } else {
                ReturnCode::ErrExit
            });
        }
        incoming.clear();
        outgoing.clear();

        if files.is_some() {
            break files.unwrap();
        }
    };

    Ok((RunState::from_init(g, TextDocs::new(), files), postponed))
}

pub fn recv_completed_tasks(
    cfg: &Config,
    ts: &mut Tasks,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<bool, ReturnCode> {
    let mut completed: Vec<TaskDone> = Vec::new();
    for done in ts.runner.rx.try_iter() {
        completed.push(done);
    }

    for done in ts.completed.iter_mut() {
        completed.push(done.take().expect("No empty slots allowed."));
    }
    ts.completed.clear();

    let compl_recv = !completed.is_empty();

    for done in completed {
        startup::process_completed_task(done, cfg, ts, outgoing);
    }
    Ok(compl_recv)
}

fn schedule_tasks(
    incoming: &mut [Option<Message>],
    g: &mut InitState,
    cfg: &mut Config,
    outgoing: &mut Vec<Option<Message>>,
    postponed: &mut Vec<Option<Message>>,
) -> Result<Option<FileIndex>, ReturnCode> {
    for msg in incoming {
        let msg = msg.take().expect("No empty slots in list.");

        if cfg.trace_level != TraceValue::Off && msg.is_notification() {
            outgoing.push(Some(Message::Notification(ls::log_notif(
                msg.get_notification(),
            ))));
        }
        startup::process_msg(msg, g, cfg, outgoing, postponed);
    }

    let files = startup::progress_multi_part_tasks(cfg, &mut g.tasks, outgoing)?;

    Ok(files)
}
