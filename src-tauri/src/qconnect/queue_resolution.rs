//! Queue cursor abstractions and index/order resolution helpers used to
//! map between cloud-side queue/renderer state and local queue indices.
//!
//! The QConnect protocol mixes queue_item_id, track_id, and ordering
//! data across multiple separate frames; this module owns the lookups
//! that reconcile those signals into a single coherent cursor.

use qconnect_app::{QConnectQueueState, QConnectRendererState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QconnectOrderedQueueCursor {
    Queue(usize),
    Autoplay(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QconnectRemoteSkipDirection {
    Next,
    Previous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct QconnectControllerQueueItemResolution {
    pub(super) target_queue_item_id: Option<u64>,
    pub(super) strategy: &'static str,
    pub(super) queue_index: Option<usize>,
    pub(super) matched_track_id: Option<u64>,
    pub(super) matched_queue_item_id: Option<u64>,
}

pub(super) fn is_valid_ordered_queue_shuffle_order(order: &[usize], track_count: usize) -> bool {
    if order.len() != track_count {
        return false;
    }
    let mut seen = vec![false; track_count];
    for &index in order {
        if index >= track_count || seen[index] {
            return false;
        }
        seen[index] = true;
    }
    true
}

pub(super) fn ordered_queue_cursors(
    queue: &QConnectQueueState,
) -> Vec<QconnectOrderedQueueCursor> {
    let mut cursors = if queue.shuffle_mode {
        queue
            .shuffle_order
            .as_ref()
            .filter(|order| is_valid_ordered_queue_shuffle_order(order, queue.queue_items.len()))
            .map(|order| {
                order
                    .iter()
                    .copied()
                    .map(QconnectOrderedQueueCursor::Queue)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| {
                queue
                    .queue_items
                    .iter()
                    .enumerate()
                    .map(|(index, _)| QconnectOrderedQueueCursor::Queue(index))
                    .collect::<Vec<_>>()
            })
    } else {
        queue
            .queue_items
            .iter()
            .enumerate()
            .map(|(index, _)| QconnectOrderedQueueCursor::Queue(index))
            .collect::<Vec<_>>()
    };

    cursors.extend(
        queue
            .autoplay_items
            .iter()
            .enumerate()
            .map(|(index, _)| QconnectOrderedQueueCursor::Autoplay(index)),
    );
    cursors
}

pub(super) fn queue_item_track_id_for_cursor(
    queue: &QConnectQueueState,
    cursor: QconnectOrderedQueueCursor,
) -> Option<u64> {
    match cursor {
        QconnectOrderedQueueCursor::Queue(index) => {
            queue.queue_items.get(index).map(|item| item.track_id)
        }
        QconnectOrderedQueueCursor::Autoplay(index) => {
            queue.autoplay_items.get(index).map(|item| item.track_id)
        }
    }
}

pub(super) fn normalized_queue_item_id_for_cursor(
    queue: &QConnectQueueState,
    cursor: QconnectOrderedQueueCursor,
) -> Option<u64> {
    match cursor {
        QconnectOrderedQueueCursor::Queue(index) => Some(
            normalize_current_queue_item_id_from_queue_state(queue, index),
        ),
        QconnectOrderedQueueCursor::Autoplay(index) => queue
            .autoplay_items
            .get(index)
            .map(|item| item.queue_item_id),
    }
}

pub(super) fn find_cursor_index_by_queue_item_id(
    cursors: &[QconnectOrderedQueueCursor],
    queue: &QConnectQueueState,
    queue_item_id: Option<u64>,
) -> Option<usize> {
    let queue_item_id = queue_item_id?;
    cursors.iter().position(|cursor| {
        normalized_queue_item_id_for_cursor(queue, *cursor) == Some(queue_item_id)
            || match cursor {
                QconnectOrderedQueueCursor::Queue(index) => queue
                    .queue_items
                    .get(*index)
                    .map(|item| item.queue_item_id == queue_item_id)
                    .unwrap_or(false),
                QconnectOrderedQueueCursor::Autoplay(index) => queue
                    .autoplay_items
                    .get(*index)
                    .map(|item| item.queue_item_id == queue_item_id)
                    .unwrap_or(false),
            }
    })
}

pub(super) fn find_cursor_index_by_track_id(
    cursors: &[QconnectOrderedQueueCursor],
    queue: &QConnectQueueState,
    track_id: Option<u64>,
) -> Option<usize> {
    let track_id = track_id?;
    cursors
        .iter()
        .position(|cursor| queue_item_track_id_for_cursor(queue, *cursor) == Some(track_id))
}

fn find_cursor_index_by_track_id_before(
    cursors: &[QconnectOrderedQueueCursor],
    queue: &QConnectQueueState,
    track_id: Option<u64>,
    end_exclusive: usize,
) -> Option<usize> {
    let track_id = track_id?;
    if end_exclusive == 0 {
        return None;
    }

    for index in (0..end_exclusive).rev() {
        if queue_item_track_id_for_cursor(queue, cursors[index]) == Some(track_id) {
            return Some(index);
        }
    }

    None
}

fn resolve_current_cursor_index_from_snapshots(
    queue: &QConnectQueueState,
    renderer: &QConnectRendererState,
    cursors: &[QconnectOrderedQueueCursor],
) -> (Option<usize>, &'static str) {
    let current_queue_index = find_cursor_index_by_queue_item_id(
        cursors,
        queue,
        renderer
            .current_track
            .as_ref()
            .map(|item| item.queue_item_id),
    );
    if current_queue_index.is_some() {
        return (
            current_queue_index,
            "renderer_current_queue_item_id_verified",
        );
    }

    let next_queue_index = find_cursor_index_by_queue_item_id(
        cursors,
        queue,
        renderer.next_track.as_ref().map(|item| item.queue_item_id),
    );
    let track_index_before_next = next_queue_index.and_then(|next_index| {
        find_cursor_index_by_track_id_before(
            cursors,
            queue,
            renderer.current_track.as_ref().map(|item| item.track_id),
            next_index,
        )
    });
    if track_index_before_next.is_some() {
        return (
            track_index_before_next,
            "queue_track_id_before_renderer_next",
        );
    }

    let current_track_index = find_cursor_index_by_track_id(
        cursors,
        queue,
        renderer.current_track.as_ref().map(|item| item.track_id),
    );
    if current_track_index.is_some() {
        return (current_track_index, "queue_track_id_match");
    }

    if let Some(next_index) = next_queue_index {
        if next_index > 0 {
            return (Some(next_index - 1), "queue_item_before_renderer_next");
        }
    }

    (None, "no_current_queue_item")
}

pub(super) fn resolve_controller_queue_item_from_snapshots(
    queue: &QConnectQueueState,
    renderer: &QConnectRendererState,
    direction: QconnectRemoteSkipDirection,
) -> QconnectControllerQueueItemResolution {
    let cursors = ordered_queue_cursors(queue);
    if cursors.is_empty() {
        return QconnectControllerQueueItemResolution {
            target_queue_item_id: None,
            strategy: "no_queue_items",
            queue_index: None,
            matched_track_id: None,
            matched_queue_item_id: None,
        };
    }

    let (current_index, _current_strategy) =
        resolve_current_cursor_index_from_snapshots(queue, renderer, &cursors);

    let (target_index, strategy) = match direction {
        QconnectRemoteSkipDirection::Next => {
            let next_index = find_cursor_index_by_queue_item_id(
                &cursors,
                queue,
                renderer.next_track.as_ref().map(|item| item.queue_item_id),
            );
            if let Some(next_index) = next_index {
                (Some(next_index), "renderer_next_queue_item_id_verified")
            } else if let Some(current_index) = current_index {
                if current_index + 1 < cursors.len() {
                    (Some(current_index + 1), "queue_item_after_current")
                } else {
                    (None, "no_next_queue_item")
                }
            } else {
                (None, "no_next_queue_item")
            }
        }
        QconnectRemoteSkipDirection::Previous => {
            if let Some(current_index) = current_index {
                if current_index > 0 {
                    (Some(current_index - 1), "queue_item_before_current")
                } else {
                    (Some(current_index), "restart_current_queue_item")
                }
            } else {
                (None, "no_previous_queue_item")
            }
        }
    };

    let Some(target_index) = target_index else {
        return QconnectControllerQueueItemResolution {
            target_queue_item_id: None,
            strategy,
            queue_index: None,
            matched_track_id: None,
            matched_queue_item_id: None,
        };
    };

    let cursor = cursors[target_index];
    let matched_track_id = queue_item_track_id_for_cursor(queue, cursor);
    let matched_queue_item_id = normalized_queue_item_id_for_cursor(queue, cursor);

    QconnectControllerQueueItemResolution {
        target_queue_item_id: matched_queue_item_id,
        strategy,
        queue_index: Some(target_index),
        matched_track_id,
        matched_queue_item_id,
    }
}

pub(super) fn resolve_queue_item_ids_from_queue_state(
    queue: &QConnectQueueState,
    track_id: u64,
) -> (Option<u64>, Option<u64>, Option<u64>) {
    if let Some(current_index) = queue
        .queue_items
        .iter()
        .position(|item| item.track_id == track_id)
    {
        let current_qid = normalize_current_queue_item_id_from_queue_state(queue, current_index);
        let next_item = if queue.shuffle_mode {
            queue
                .shuffle_order
                .as_ref()
                .and_then(|order| {
                    order
                        .iter()
                        .position(|queue_index| *queue_index == current_index)
                        .and_then(|order_index| order.get(order_index + 1))
                        .and_then(|queue_index| queue.queue_items.get(*queue_index))
                })
                .or_else(|| queue.queue_items.get(current_index + 1))
                .or_else(|| queue.autoplay_items.first())
        } else {
            queue
                .queue_items
                .get(current_index + 1)
                .or_else(|| queue.autoplay_items.first())
        };

        return (
            Some(current_qid),
            next_item.map(|item| item.queue_item_id),
            next_item.map(|item| item.track_id),
        );
    }

    if let Some(current_index) = queue
        .autoplay_items
        .iter()
        .position(|item| item.track_id == track_id)
    {
        let current_item = &queue.autoplay_items[current_index];
        let next_item = queue.autoplay_items.get(current_index + 1);
        return (
            Some(current_item.queue_item_id),
            next_item.map(|item| item.queue_item_id),
            next_item.map(|item| item.track_id),
        );
    }

    (None, None, None)
}

pub(super) fn dedupe_track_ids(queue_state: &QConnectQueueState) -> Vec<u64> {
    let mut unique = Vec::with_capacity(queue_state.queue_items.len());
    for item in &queue_state.queue_items {
        if !unique.contains(&item.track_id) {
            unique.push(item.track_id);
        }
    }
    unique
}

pub(super) fn resolve_remote_start_index(
    queue_state: &QConnectQueueState,
    renderer_queue_item_id: Option<u64>,
    renderer_track_id: Option<u64>,
) -> Option<usize> {
    if let Some(queue_item_id) = renderer_queue_item_id {
        if let Some(index) = queue_state
            .queue_items
            .iter()
            .position(|item| item.queue_item_id == queue_item_id)
        {
            return Some(index);
        }
    }

    if let Some(track_id) = renderer_track_id {
        if let Some(index) = queue_state
            .queue_items
            .iter()
            .position(|item| item.track_id == track_id)
        {
            return Some(index);
        }
    }

    None
}

pub(super) fn resolve_core_shuffle_order(
    queue_state: &QConnectQueueState,
    renderer_queue_item_id: Option<u64>,
    renderer_track_id: Option<u64>,
    renderer_next_queue_item_id: Option<u64>,
    renderer_next_track_id: Option<u64>,
) -> Option<Vec<usize>> {
    if !queue_state.shuffle_mode {
        return None;
    }

    let raw_order = queue_state.shuffle_order.as_ref().filter(|order| {
        is_valid_ordered_queue_shuffle_order(order, queue_state.queue_items.len())
    });

    if raw_order.is_none() {
        log::debug!(
            "[QConnect] resolve_core_shuffle_order: raw_order invalid or absent, items={} order={:?}",
            queue_state.queue_items.len(),
            queue_state.shuffle_order,
        );
        return None;
    }
    let raw_order = raw_order.unwrap();

    let current_index =
        resolve_remote_start_index(queue_state, renderer_queue_item_id, renderer_track_id);
    let next_index = resolve_remote_start_index(
        queue_state,
        renderer_next_queue_item_id,
        renderer_next_track_id,
    );

    let mut ordered = Vec::with_capacity(queue_state.queue_items.len());
    if let Some(index) = current_index {
        ordered.push(index);
    }
    if let Some(index) = next_index {
        if !ordered.contains(&index) {
            ordered.push(index);
        }
    }
    for &index in raw_order {
        if !ordered.contains(&index) {
            ordered.push(index);
        }
    }
    for index in 0..queue_state.queue_items.len() {
        if !ordered.contains(&index) {
            ordered.push(index);
        }
    }

    log::debug!(
        "[QConnect] resolve_core_shuffle_order: result={:?} current={:?} next={:?}",
        ordered, current_index, next_index,
    );

    Some(ordered)
}

fn is_cloud_placeholder_current_queue_item(
    queue: &QConnectQueueState,
    current_index: usize,
) -> bool {
    let Some(current_item) = queue.queue_items.get(current_index) else {
        return false;
    };

    current_index == 0
        && current_item.queue_item_id == current_item.track_id
        && queue
            .queue_items
            .iter()
            .skip(1)
            .any(|item| item.queue_item_id < current_item.queue_item_id)
}

pub(super) fn normalize_current_queue_item_id_from_queue_state(
    queue: &QConnectQueueState,
    current_index: usize,
) -> u64 {
    if is_cloud_placeholder_current_queue_item(queue, current_index) {
        0
    } else {
        queue.queue_items[current_index].queue_item_id
    }
}
