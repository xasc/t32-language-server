// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use t32_language_server_user_guide_data::format_practice_func_args;

use crate::ls::ReturnCode;
use crate::ls::doc::TextDocData;
use crate::ls::doc::TextDocs;
use crate::ls::lsp::Message;
use crate::ls::request::Notification;
use crate::ls::response::HoverResponse;
use crate::ls::response::NullResponse;
use crate::ls::response::Response;
use crate::ls::tasks::Task;
use crate::ls::tasks::Tasks;
use crate::ls::tasks::trace_doc_unknown;
use crate::ls::tasks::try_schedule;
use crate::protocol::Hover;
use crate::protocol::HoverParams;
use crate::protocol::LogTraceParams;
use crate::protocol::MarkupKind;
use crate::protocol::NumberOrString;
use crate::protocol::Position;
use crate::protocol::TraceValue;
use crate::t32::FunctionMetaDescription;
use crate::t32::HoverTarget;
use crate::t32::validate_hover_target;
use crate::ug::UserGuide;

const URL_DOCS: &'static str = "https://repo.lauterbach.com/pdf";
pub fn process_hover_req(
    id: NumberOrString,
    params: HoverParams,
    trace_level: TraceValue,
    docs: &TextDocs,
    ts: &mut Tasks,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
    if trace_level != TraceValue::Off {
        outgoing.push(Some(log_hover_req(id.clone())));
    }

    let (doc, tree) = match docs.get_doc_data(&params.text_document.uri) {
        Some((doc, tree, _)) => (doc, tree),
        None => {
            if trace_level != TraceValue::Off {
                outgoing.push(Some(trace_doc_unknown(&params.text_document.uri)));
            }
            outgoing.push(Some(Message::Response(Response::NullResponse(
                NullResponse { id },
            ))));
            return Ok(());
        }
    };

    try_schedule(
        &mut ts.runner,
        Task::Hover(
            id,
            TextDocData {
                doc: doc.clone(),
                tree: tree.clone(),
            },
            params.position,
            identify_hover_target,
        ),
        &mut ts.ongoing,
        &mut ts.blocked,
    )?;
    Ok(())
}

pub fn process_hover_result(
    id: &NumberOrString,
    markup: MarkupKind,
    hover_target: Option<HoverTarget>,
    user_guide: &UserGuide,
) -> HoverResponse {
    let Some(HoverTarget::BuiltinFunction { name, full }) = hover_target else {
        return HoverResponse {
            id: id.clone(),
            result: None,
        };
    };

    let Some(desc) = user_guide.functions.get(&name) else {
        return HoverResponse {
            id: id.clone(),
            result: None,
        };
    };

    let contents = info_builtin_function(markup, &desc);

    HoverResponse {
        id: id.clone(),
        result: Some(Hover {
            contents,
            range: Some(full),
        }),
    }
}

fn identify_hover_target(textdoc: TextDocData, position: Position) -> Option<HoverTarget> {
    validate_hover_target(textdoc.doc, textdoc.tree, position)
}

fn info_builtin_function(format: MarkupKind, func: &FunctionMetaDescription) -> String {
    if format == MarkupKind::Markdown {
        format!(
            r"{}({})
   - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
{}

   - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
[{}]({}/{})  
{}
",
            func.name,
            format_practice_func_args(func.params),
            func.operation,
            func.doc,
            URL_DOCS,
            func.doc,
            func.copyright
        )
    } else {
        format!(
            r"{}({})
   - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
{}

   - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
{} <{}/{}>
{}
",
            func.name,
            format_practice_func_args(func.params),
            func.operation,
            func.doc,
            URL_DOCS,
            func.doc,
            func.copyright
        )
    }
    .replace('<', "&lt;")
}

fn log_hover_req(id: NumberOrString) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!("INFO: Received hover request with ID \"{:}\".", id),
            verbose: None,
        },
    })
}
