// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::protocol::{InitializeParams, NumberOrString};

pub enum Request {
    ExitNotification(ExitNotification),
    InitializeRequest(InitializeRequest),
    ShutdownRequest(ShutdownRequest),
}

pub struct ExitNotification {
    pub id: Option<NumberOrString>,
}

pub struct InitializeRequest {
    pub id: NumberOrString,
    pub params: InitializeParams,
}

pub struct ShutdownRequest {
    pub id: NumberOrString,
}

impl Request {
    pub fn get_id(self) -> Option<NumberOrString> {
        match self {
            Request::ExitNotification(ExitNotification { id, .. }) => id,
            Request::InitializeRequest(InitializeRequest { id, .. }) => Some(id),
            Request::ShutdownRequest(ShutdownRequest { id }) => Some(id),
        }
    }

    pub fn is_request(&self) -> bool {
        match self {
            Request::InitializeRequest(_) | Request::ShutdownRequest(_) => true,
            _ => false,
        }
    }
}
