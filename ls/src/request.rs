// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::protocol::{InitializeParams, InitializedParams, NumberOrString};

#[derive(Debug)]
pub enum Request {
    ExitNotification(ExitNotification),
    InitializedNotification(InitializedNotification),
    InitializeRequest(InitializeRequest),
    ShutdownRequest(ShutdownRequest),
}

#[derive(Debug)]
pub struct ExitNotification {}

#[derive(Debug)]
pub struct InitializedNotification {
    #[allow(dead_code)]
    pub params: InitializedParams,
}

#[derive(Debug)]
pub struct InitializeRequest {
    pub id: NumberOrString,
    pub params: InitializeParams,
}

#[derive(Debug)]
pub struct ShutdownRequest {
    pub id: NumberOrString,
}

impl Request {
    pub fn get_id(self) -> Option<NumberOrString> {
        match self {
            Request::ExitNotification(_) => None,
            Request::InitializedNotification(_) => None,
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
