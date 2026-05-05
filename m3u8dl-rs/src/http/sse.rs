//! Server-Sent Events endpoint: `GET /job/:id/events` streams `ProgressEvent`s as they're
//! emitted by the orchestrator. Replaces the 3-second polling loop in `capture.user.js`
//! with realtime push (UI sees per-segment progress + merge percentage live).
//!
//! First event is always a `snapshot` of current state (so late joiners aren't blank);
//! subsequent events come straight off the per-job broadcast channel. Lagged subscribers
//! get dropped silently — they reconnect and re-read state from the snapshot.

use std::convert::Infallible;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::{Stream, StreamExt, stream};
use tokio_stream::wrappers::BroadcastStream;

use crate::domain::JobId;
use crate::http::dto::{ErrorBody, JobSnapshot};
use crate::http::routes::AppState;

pub async fn job_events(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, Json<ErrorBody>)> {
    let job_id = JobId::from_string(id);
    let h = s.registry.get(&job_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: format!("job {job_id} not found"),
            }),
        )
    })?;

    // Snapshot under lock — released before we await on the stream
    let snapshot_event = {
        let snap = match h.job.lock() {
            Ok(g) => JobSnapshot::from_job(&g),
            Err(_) => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorBody {
                        error: "job mutex poisoned".into(),
                    }),
                ));
            }
        };
        Event::default()
            .event("snapshot")
            .json_data(snap)
            .unwrap_or_else(|_| Event::default().data("{\"error\":\"snapshot serialize failed\"}"))
    };

    let live = BroadcastStream::new(h.sink.subscribe()).filter_map(|r| async move {
        let ev = r.ok()?;
        Event::default().event("progress").json_data(ev).ok()
    });

    let combined = stream::once(async move { snapshot_event })
        .chain(live)
        .map(Ok::<_, Infallible>);

    Ok(Sse::new(combined).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("ping"),
    ))
}
