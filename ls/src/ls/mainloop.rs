// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::time::Instant;

use crate::{
    ReturnCode,
    config::Config,
    ls::{
        RunState, error_task_queue_abort,
        lsp::Message,
        recv_incoming,
        response::{ErrorResponse, Response},
        send_outgoing, tasks,
        transport::StdioChannel,
    },
};

pub fn handle_requests(
    mut g: RunState,
    postponed: Vec<Option<Message>>,
    channel: &mut StdioChannel,
    mut cfg: Config,
) -> Result<(), ReturnCode> {
    debug_assert!(g.tasks.ongoing.len() < 3);
    debug_assert_eq!(g.tasks.blocked.len(), 0);
    debug_assert_eq!(g.tasks.completed.len(), 0);

    let mut incoming: Vec<Option<Message>> = postponed;
    let mut outgoing: Vec<Option<Message>> = Vec::new();

    loop {
        g.backoff.idle(&Instant::now());

        if g.tasks.runner.aborted() {
            channel.send_msg(Message::Response(Response::ErrorResponse(ErrorResponse {
                id: None,
                error: error_task_queue_abort(),
            })));
            return Err(ReturnCode::SoftwareErr);
        }

        if recv_incoming(cfg.trace_level, channel, &mut g.heartbeat, &mut incoming)? {
            g.backoff.clear();
        }

        tasks::recv_responses(cfg.trace_level, &mut incoming, &mut g.tasks, &mut outgoing);

        if tasks::recv_completed_tasks(
            &cfg,
            &mut g.tasks,
            &mut g.docs,
            &mut g.files,
            &mut outgoing,
        )? {
            g.backoff.clear();
        }

        tasks::schedule_tasks(&mut incoming, &mut g, &mut cfg, &mut outgoing)?;

        if !outgoing.is_empty() {
            g.backoff.clear();
        }
        send_outgoing(channel, &mut outgoing);

        if g.exit_requested {
            return Err(if g.shutdown_request_recv {
                ReturnCode::OkExit
            } else {
                ReturnCode::ErrExit
            });
        }
        incoming.clear();
        outgoing.clear();
    }
}
