use aspen::semantics::Host;
use aspen::syntax::Node;
use aspen::{Context, Location, Range, Source, URI};
use clap::{App, ArgMatches};
use futures::future::{AbortHandle, Abortable};
use log::info;
use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{Cancel, DidChangeTextDocument, PublishDiagnostics, DidOpenTextDocument};
use lsp_types::{request::GotoDefinition, DidChangeTextDocumentParams, GotoDefinitionResponse, InitializeParams, NumberOrString, PublishDiagnosticsParams, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions, Url, WorkspaceCapability, WorkspaceFolderCapability, DidOpenTextDocumentParams};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub fn app() -> App<'static, 'static> {
    App::new("server")
}

pub async fn main(_matches: &ArgMatches<'_>) -> clap::Result<()> {
    flexi_logger::Logger::with_str("info").start().unwrap();

    let (connection, io_threads) = Connection::stdio();
    let connection = Arc::new(connection);

    let mut text_document_sync = TextDocumentSyncOptions::default();
    text_document_sync.open_close = Some(true);
    text_document_sync.change = Some(TextDocumentSyncKind::Incremental);

    let mut capabilities = ServerCapabilities::default();
    capabilities.definition_provider = Some(true);
    capabilities.text_document_sync = Some(TextDocumentSyncCapability::Options(text_document_sync));
    capabilities.workspace = Some(WorkspaceCapability {
        workspace_folders: Some(WorkspaceFolderCapability {
            supported: Some(true),
            change_notifications: None,
        }),
    });

    let initialization_params: InitializeParams = serde_json::from_value(
        connection
            .initialize(serde_json::to_value(&mut capabilities).unwrap())
            .unwrap(),
    )
    .unwrap();

    let context = match initialization_params.root_uri {
        Some(url) if url.scheme() == "file" => {
            Context::infer_from(url.path().into()).await.unwrap()
        }
        _ => Context::infer().await.unwrap(),
    };

    let root_dir = context.root_dir().unwrap();

    info!("Starting Aspen Language Server in {}", root_dir.display());

    let host = Host::from(
        context,
        Source::files(format!("{}/**/*.aspen", root_dir.display())).await,
    )
    .await;
    let state = ServerState::new(host, connection.clone());

    for module in state.host.modules().await {
        state.schedule_check(module.uri().clone()).await;
    }

    for msg in &connection.receiver {
        if let Message::Request(req) = &msg {
            if connection.handle_shutdown(req).unwrap() {
                break;
            }
        }
        let state = state.clone();
        tokio::task::spawn(async move { state.handle_msg(msg).await });
    }
    io_threads.join().unwrap();

    Ok(())
}

struct ServerState {
    host: Host,
    connection: Arc<Connection>,
    tasks: Mutex<HashMap<RequestId, AbortHandle>>,
    scheduled_check: Mutex<HashMap<URI, AbortHandle>>,
}

impl ServerState {
    pub fn new(host: Host, connection: Arc<Connection>) -> Arc<ServerState> {
        Arc::new(ServerState {
            host,
            connection,
            tasks: Mutex::new(HashMap::new()),
            scheduled_check: Mutex::new(HashMap::new()),
        })
    }

    async fn schedule_check(&self, uri: URI) {
        let mut schedule = self.scheduled_check.lock().await;
        if let Some(handle) = schedule.get(&uri) {
            handle.abort();
        }

        let (abort_handle, reg) = AbortHandle::new_pair();
        schedule.insert(uri.clone(), abort_handle);

        let host = self.host.clone();
        let connection = self.connection.clone();
        tokio::task::spawn(Abortable::new(
            async move {
                let diagnostics = match host.get(&uri).await {
                    Some(m) => m
                        .diagnostics()
                        .await
                        .into_iter()
                        .map(|d| lsp_types::Diagnostic {
                            range: range_to_lsp_range(d.range()),
                            severity: None,
                            code: None,
                            source: None,
                            message: d.message().into(),
                            related_information: None,
                            tags: None,
                        })
                        .collect(),
                    None => vec![],
                };
                connection
                    .sender
                    .send(Message::Notification(Notification::new(
                        <PublishDiagnostics as lsp_types::notification::Notification>::METHOD
                            .into(),
                        PublishDiagnosticsParams {
                            uri: Url::parse(uri.uri()).unwrap(),
                            diagnostics,
                            version: None,
                        },
                    )))
                    .unwrap();
            },
            reg,
        ));
    }

    pub async fn handle_msg(&self, msg: Message) {
        match msg {
            Message::Request(req) => {
                let rid = req.id.clone();
                let (abort_handle, reg) = AbortHandle::new_pair();
                self.tasks.lock().await.insert(rid.clone(), abort_handle);
                Abortable::new(
                    async move {
                        self.handle_request(req).await;

                        self.tasks.lock().await.remove(&rid);
                    },
                    reg,
                )
                .await
                .unwrap_or(());
            }
            Message::Response(resp) => {
                info!("Unknown response: {:?}", resp);
            }
            Message::Notification(not) => self.handle_notification(not).await,
        }
    }

    async fn handle_request(&self, req: Request) {
        const INTERNAL_ERROR: i32 = -32603;

        let req = match cast_request::<GotoDefinition>(req) {
            Err(req) => req,
            Ok((id, params)) => {
                let uri = params
                    .text_document_position_params
                    .text_document
                    .uri
                    .as_str()
                    .into();
                let module = self.host.get(&uri).await;
                let mut result: Option<GotoDefinitionResponse> = None;
                if let Some(module) = module {
                    let location = lsp_position_to_location(
                        &module.source,
                        params.text_document_position_params.position,
                    );

                    if let Some(nav) = module.navigate().to_location(&location) {
                        if let Some(reference) = nav.up_to_cast(|n| n.as_reference_expression()) {
                            if let Some(dec) = module.declaration_referenced_by(reference).await {
                                result = Some(GotoDefinitionResponse::Scalar(lsp_types::Location {
                                    uri: params
                                        .text_document_position_params
                                        .text_document
                                        .uri
                                        .clone(),
                                    range: range_to_lsp_range(dec.range()),
                                }))
                            }
                        }

                        if let Some(reference) =
                            nav.up_to_cast(|n| n.as_reference_type_expression())
                        {
                            if let Some(dec) =
                                module.declaration_referenced_by_type(reference).await
                            {
                                result = Some(GotoDefinitionResponse::Scalar(lsp_types::Location {
                                    uri: params
                                        .text_document_position_params
                                        .text_document
                                        .uri
                                        .clone(),
                                    range: range_to_lsp_range(dec.range()),
                                }))
                            }
                        }
                    }
                }
                return self
                    .connection
                    .sender
                    .send(Message::Response(Response::new_ok(id, result)))
                    .unwrap();
            }
        };

        info!("Unknown request: {:?}", req);

        self.connection
            .sender
            .send(Message::Response(Response::new_err(
                req.id,
                INTERNAL_ERROR,
                "Request handler not implemented".into(),
            )))
            .unwrap();
    }

    async fn handle_notification(&self, not: Notification) {
        let not = match cast_notification::<Cancel>(not) {
            Err(not) => not,
            Ok(cancel) => {
                let id: RequestId = match cancel.id {
                    NumberOrString::String(s) => s.into(),
                    NumberOrString::Number(s) => s.into(),
                };

                if let Some(abort) = self.tasks.lock().await.remove(&id) {
                    abort.abort();
                    const REQUEST_CANCELLED: i32 = -32800;
                    self.connection
                        .sender
                        .send(Message::Response(Response::new_err(
                            id,
                            REQUEST_CANCELLED,
                            "Request was cancelled by the client".into(),
                        )))
                        .unwrap();
                }

                return;
            }
        };

        let not = match cast_notification::<DidChangeTextDocument>(not) {
            Err(not) => not,
            Ok(DidChangeTextDocumentParams {
                   text_document,
                   content_changes,
               }) => {
                let uri: URI = text_document.uri.as_str().into();
                let module = self.host.get(&uri).await;
                if let Some(module) = module {
                    self.host
                        .apply_edits(
                            &uri,
                            content_changes.into_iter().map(|c| {
                                let range = c.range.map(|r| lsp_range_to_range(&module.source, r));

                                (range, c.text)
                            }),
                        )
                        .await;
                }
                self.schedule_check(uri).await;
                return;
            }
        };

        let not = match cast_notification::<DidOpenTextDocument>(not) {
            Err(not) => not,
            Ok(DidOpenTextDocumentParams {
                text_document,
            }) => {
                let source = Source::new(text_document.uri.as_str(), text_document.text);
                let uri = source.uri().clone();
                self.host.set(source).await;
                self.schedule_check(uri).await;
                return;
            }
        };

        info!("Unknown notification: {:?}", not);
    }
}

fn cast_notification<N>(not: Notification) -> Result<N::Params, Notification>
where
    N: lsp_types::notification::Notification,
    N::Params: serde::de::DeserializeOwned,
{
    not.extract(N::METHOD)
}

fn cast_request<R>(req: Request) -> Result<(RequestId, R::Params), Request>
where
    R: lsp_types::request::Request,
    R::Params: serde::de::DeserializeOwned,
{
    req.extract(R::METHOD)
}

fn range_to_lsp_range(range: Range) -> lsp_types::Range {
    lsp_types::Range {
        start: location_to_lsp_position(range.start),
        end: location_to_lsp_position(range.end),
    }
}

fn location_to_lsp_position(location: Location) -> lsp_types::Position {
    lsp_types::Position {
        line: location.line as u64 - 1,
        character: location.character as u64 - 1,
    }
}

fn lsp_range_to_range(source: &Arc<Source>, range: lsp_types::Range) -> Range {
    Range {
        start: lsp_position_to_location(source, range.start),
        end: lsp_position_to_location(source, range.end),
    }
}

fn lsp_position_to_location(source: &Arc<Source>, position: lsp_types::Position) -> Location {
    source.location_at_coords(position.line as usize + 1, position.character as usize + 1)
}
