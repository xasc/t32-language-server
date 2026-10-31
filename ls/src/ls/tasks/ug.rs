// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::time::Duration;

use crate::ls::Message;
use crate::ls::Notification;
use crate::protocol::LogTraceParams;
use crate::protocol::NumberOrString;
pub fn error_user_guide_data(id: NumberOrString, details: Option<String>) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!("ERROR: User guide data import with ID {} has failed.", id,),
            verbose: details,
        },
    })
}

pub fn trace_user_guide_data_import(
    duration: Duration,
    id: NumberOrString,
    details: Option<usize>,
) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: User guide data import with ID {} completed in {:.4} seconds.",
                id,
                duration.as_secs_f32()
            ),
            verbose: if let Some(size) = details {
                Some(format!(
                    "User guide data with a total size of {} KB was loaded.",
                    size / 1024
                ))
            } else {
                None
            },
        },
    })
}
