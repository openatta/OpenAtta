//! WebSocket handler

use axum::{
    extract::{
        ws::{self, WebSocket},
        State, WebSocketUpgrade,
    },
    http::StatusCode,
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};

use atta_types::{Action, AuthzDecision, Resource, ResourceType};

use crate::middleware::CurrentUser;
use crate::server::AppState;

pub async fn ws_upgrade(State(state): State<AppState>, user: CurrentUser, ws: WebSocketUpgrade) -> impl IntoResponse {
    // Validate the user has at least read access before upgrading
    match state.authz.check(&user.actor, Action::Read, &Resource::all(ResourceType::Task)).await {
        Ok(AuthzDecision::Allow) => {}
        _ => {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    ws.on_upgrade(move |socket| handle_ws(state, socket)).into_response()
}

async fn handle_ws(state: AppState, socket: WebSocket) {
    let (mut sender, mut receiver) = socket.split();

    let (client_id, mut rx) = state.ws_hub.add_client().await;

    // Forward hub messages to WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(ws::Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    // Receive from WebSocket (just drain; we don't expect client messages yet)
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(_msg)) = receiver.next().await {
            // Client messages are ignored for now
        }
    });

    // Wait for either side to close
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    state.ws_hub.remove_client(&client_id).await;
}
