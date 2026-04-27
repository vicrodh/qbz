use super::commands::{
    build_qconnect_file_audio_quality_snapshot, classify_qconnect_audio_quality,
    determine_queue_lookup_report_strategy,
    should_skip_renderer_report_due_to_stale_snapshot,
};
use super::queue_resolution::{
    resolve_controller_queue_item_from_snapshots, resolve_queue_item_ids_from_queue_state,
    QconnectRemoteSkipDirection,
};
use super::session::{
    find_unique_renderer_id, refresh_local_renderer_id, QconnectFileAudioQualitySnapshot,
};
use super::transport::{
    decode_hex_channel, default_qconnect_device_info, parse_subscribe_channels,
};
use super::{
    normalize_volume_to_fraction, QconnectHandoffIntent, QconnectOutboundCommandType,
    QconnectRendererInfo, QconnectSessionState, QconnectTrackOrigin, AUDIO_QUALITY_HIRES_LEVEL1,
};
use qbz_models::RepeatMode;
use qconnect_app::{
    resolve_handoff_intent, QConnectQueueState, QConnectRendererState, QueueCommandType,
};
use qconnect_core::QueueItem;
use serde_json::json;

#[test]
fn decodes_hex_channels() {
    assert_eq!(decode_hex_channel("02").expect("decode"), vec![0x02]);
    assert_eq!(
        decode_hex_channel("0A0B").expect("decode"),
        vec![0x0A, 0x0B]
    );
}

#[test]
fn parses_multiple_channels() {
    let channels =
        parse_subscribe_channels(vec!["02".to_string(), "0A0B".to_string()]).expect("channels");
    assert_eq!(channels, vec![vec![0x02], vec![0x0A, 0x0B]]);
}

#[test]
fn normalizes_renderer_volume() {
    assert!((normalize_volume_to_fraction(58) - 0.58).abs() < f32::EPSILON);
    assert!((normalize_volume_to_fraction(-5) - 0.0).abs() < f32::EPSILON);
    assert!((normalize_volume_to_fraction(125) - 1.0).abs() < f32::EPSILON);
}

#[test]
fn maps_outbound_command_type_to_protocol_command_type() {
    assert_eq!(
        QconnectOutboundCommandType::JoinSession.to_queue_command_type(),
        QueueCommandType::CtrlSrvrJoinSession
    );
    assert_eq!(
        QconnectOutboundCommandType::SetPlayerState.to_queue_command_type(),
        QueueCommandType::CtrlSrvrSetPlayerState
    );
    assert_eq!(
        QconnectOutboundCommandType::SetActiveRenderer.to_queue_command_type(),
        QueueCommandType::CtrlSrvrSetActiveRenderer
    );
    assert_eq!(
        QconnectOutboundCommandType::SetVolume.to_queue_command_type(),
        QueueCommandType::CtrlSrvrSetVolume
    );
    assert_eq!(
        QconnectOutboundCommandType::AskForRendererState.to_queue_command_type(),
        QueueCommandType::CtrlSrvrAskForRendererState
    );
}

#[test]
fn flags_commands_that_require_remote_queue_admission() {
    assert!(QconnectOutboundCommandType::QueueAddTracks.requires_remote_queue_admission());
    assert!(QconnectOutboundCommandType::QueueLoadTracks.requires_remote_queue_admission());
    assert!(QconnectOutboundCommandType::QueueInsertTracks.requires_remote_queue_admission());
    assert!(QconnectOutboundCommandType::SetQueueState.requires_remote_queue_admission());
    assert!(QconnectOutboundCommandType::AutoplayLoadTracks.requires_remote_queue_admission());
    assert!(!QconnectOutboundCommandType::QueueRemoveTracks.requires_remote_queue_admission());
    assert!(!QconnectOutboundCommandType::ClearQueue.requires_remote_queue_admission());
    assert!(!QconnectOutboundCommandType::SetVolume.requires_remote_queue_admission());
}

#[test]
fn maps_qconnect_track_origin_to_core_origin_and_handoff() {
    let local_core_origin = QconnectTrackOrigin::LocalLibrary.into_core_origin();
    assert_eq!(
        QconnectHandoffIntent::from_core(resolve_handoff_intent(local_core_origin)),
        QconnectHandoffIntent::ContinueLocally
    );

    let qobuz_core_origin = QconnectTrackOrigin::QobuzOnline.into_core_origin();
    assert_eq!(
        QconnectHandoffIntent::from_core(resolve_handoff_intent(qobuz_core_origin)),
        QconnectHandoffIntent::SendToConnect
    );
}

#[test]
fn refreshes_local_renderer_id_from_exact_device_uuid_match() {
    let local_device_uuid = super::resolve_qconnect_device_uuid();
    let mut session = QconnectSessionState {
        renderers: vec![
            QconnectRendererInfo {
                renderer_id: 1,
                device_uuid: Some("peer-device".to_string()),
                friendly_name: Some("BlitzPhone16ProMax".to_string()),
                brand: Some("Apple".to_string()),
                model: Some("iPhone".to_string()),
                device_type: Some(6),
            },
            QconnectRendererInfo {
                renderer_id: 6,
                device_uuid: Some(local_device_uuid),
                friendly_name: Some("QBZ Desktop".to_string()),
                brand: Some("QBZ".to_string()),
                model: Some("QBZ".to_string()),
                device_type: Some(5),
            },
        ],
        ..Default::default()
    };

    refresh_local_renderer_id(&mut session);

    assert_eq!(session.local_renderer_id, Some(6));
}

#[test]
fn refreshes_local_renderer_id_from_unique_fingerprint_when_uuid_missing() {
    // Use the runtime-resolved local device info so the test stays correct
    // regardless of hostname / env-var-driven device name overrides.
    let local_device_info = default_qconnect_device_info();
    let mut session = QconnectSessionState {
        renderers: vec![
            QconnectRendererInfo {
                renderer_id: 1,
                device_uuid: None,
                friendly_name: Some("BlitzPhone16ProMax".to_string()),
                brand: Some("Apple".to_string()),
                model: Some("iPhone".to_string()),
                device_type: Some(6),
            },
            QconnectRendererInfo {
                renderer_id: 6,
                device_uuid: None,
                friendly_name: local_device_info.friendly_name.clone(),
                brand: local_device_info.brand.clone(),
                model: local_device_info.model.clone(),
                device_type: local_device_info.device_type,
            },
        ],
        ..Default::default()
    };

    refresh_local_renderer_id(&mut session);

    assert_eq!(session.local_renderer_id, Some(6));
}

#[test]
fn does_not_guess_local_renderer_id_when_fingerprint_is_ambiguous() {
    let mut session = QconnectSessionState {
        renderers: vec![
            QconnectRendererInfo {
                renderer_id: 6,
                device_uuid: None,
                friendly_name: Some("QBZ Desktop".to_string()),
                brand: Some("QBZ".to_string()),
                model: Some("QBZ".to_string()),
                device_type: Some(5),
            },
            QconnectRendererInfo {
                renderer_id: 9,
                device_uuid: None,
                friendly_name: Some("QBZ Desktop".to_string()),
                brand: Some("QBZ".to_string()),
                model: Some("QBZ".to_string()),
                device_type: Some(5),
            },
        ],
        ..Default::default()
    };

    refresh_local_renderer_id(&mut session);

    assert_eq!(session.local_renderer_id, None);
    assert_eq!(
        find_unique_renderer_id(&session, |renderer| renderer.device_type == Some(5)),
        None
    );
}

#[test]
fn skips_renderer_report_when_local_track_and_renderer_snapshot_disagree() {
    assert!(should_skip_renderer_report_due_to_stale_snapshot(
        Some(388712168),
        None,
        None,
        Some(193849747),
    ));
}

#[test]
fn does_not_skip_renderer_report_when_snapshot_matches_local_track() {
    assert!(!should_skip_renderer_report_due_to_stale_snapshot(
        Some(388712168),
        None,
        None,
        Some(388712168),
    ));
}

#[test]
fn does_not_skip_renderer_report_once_current_queue_item_id_is_resolved() {
    assert!(!should_skip_renderer_report_due_to_stale_snapshot(
        Some(388712168),
        None,
        Some(42),
        Some(193849747),
    ));
}

#[test]
fn detects_queue_lookup_track_transition() {
    assert_eq!(
        determine_queue_lookup_report_strategy(
            None,
            Some(57608710),
            Some(59952963),
            Some(59952963_i32),
            Some(1),
            Some(1),
            Some(2),
        ),
        Some("queue_lookup_track_transition"),
    );
}

#[test]
fn detects_queue_lookup_queue_drift_when_next_item_changes() {
    assert_eq!(
        determine_queue_lookup_report_strategy(
            None,
            Some(123452387),
            Some(123452387),
            Some(123452387_i32),
            Some(1),
            Some(0),
            Some(12),
        ),
        Some("queue_lookup_queue_drift"),
    );
}

#[test]
fn does_not_force_queue_lookup_when_renderer_snapshot_matches_queue() {
    assert_eq!(
        determine_queue_lookup_report_strategy(
            None,
            Some(123452387),
            Some(123452387),
            Some(123452387_i32),
            Some(1),
            Some(123452387_i32),
            Some(1),
        ),
        None,
    );
}

#[test]
fn keeps_reporting_queue_item_ids_while_local_renderer_is_active() {
    assert!(super::should_report_queue_item_ids_for_renderer_state(
        None,
        None,
        true,
        Some(12),
    ));
}

#[test]
fn does_not_force_queue_item_ids_for_peer_renderer_without_explicit_lookup() {
    assert!(!super::should_report_queue_item_ids_for_renderer_state(
        None,
        None,
        false,
        Some(12),
    ));
}

#[test]
fn maps_qconnect_loop_mode_to_repeat_mode() {
    assert_eq!(
        super::qconnect_repeat_mode_from_loop_mode(0),
        Some(RepeatMode::Off)
    );
    assert_eq!(
        super::qconnect_repeat_mode_from_loop_mode(1),
        Some(RepeatMode::Off)
    );
    assert_eq!(
        super::qconnect_repeat_mode_from_loop_mode(2),
        Some(RepeatMode::One)
    );
    assert_eq!(
        super::qconnect_repeat_mode_from_loop_mode(3),
        Some(RepeatMode::All)
    );
    assert_eq!(super::qconnect_repeat_mode_from_loop_mode(99), None);
}

#[test]
fn resolves_current_and_next_queue_item_ids_from_queue_order() {
    let queue: QConnectQueueState = serde_json::from_value(json!({
        "version": { "major": 4, "minor": 1 },
        "queue_items": [
            { "track_context_uuid": "ctx", "track_id": 59952963, "queue_item_id": 59952963 },
            { "track_context_uuid": "ctx", "track_id": 57608710, "queue_item_id": 1 },
            { "track_context_uuid": "ctx", "track_id": 2013968, "queue_item_id": 2 }
        ],
        "shuffle_mode": false,
        "shuffle_order": null,
        "autoplay_mode": false,
        "autoplay_loading": false,
        "autoplay_items": [],
        "updated_at_ms": 0
    }))
    .expect("queue state");

    assert_eq!(
        resolve_queue_item_ids_from_queue_state(&queue, 57608710),
        (Some(1), Some(2), Some(2013968)),
    );
}

#[test]
fn normalizes_placeholder_current_queue_item_id_to_zero() {
    let queue: QConnectQueueState = serde_json::from_value(json!({
        "version": { "major": 8, "minor": 2 },
        "queue_items": [
            { "track_context_uuid": "ctx", "track_id": 126886853, "queue_item_id": 126886853 },
            { "track_context_uuid": "ctx", "track_id": 123452387, "queue_item_id": 10 },
            { "track_context_uuid": "ctx", "track_id": 126886854, "queue_item_id": 1 }
        ],
        "shuffle_mode": false,
        "shuffle_order": null,
        "autoplay_mode": false,
        "autoplay_loading": false,
        "autoplay_items": [],
        "updated_at_ms": 0
    }))
    .expect("queue state");

    assert_eq!(
        resolve_queue_item_ids_from_queue_state(&queue, 126886853),
        (Some(0), Some(10), Some(123452387)),
    );
}

#[test]
fn builds_effective_remote_renderer_snapshot_from_session_cursor() {
    let queue: QConnectQueueState = serde_json::from_value(json!({
        "version": { "major": 4, "minor": 1 },
        "queue_items": [
            { "track_context_uuid": "ctx", "track_id": 126886862, "queue_item_id": 126886862 },
            { "track_context_uuid": "ctx", "track_id": 25584418, "queue_item_id": 1 },
            { "track_context_uuid": "ctx", "track_id": 25120807, "queue_item_id": 2 }
        ],
        "shuffle_mode": false,
        "shuffle_order": null,
        "autoplay_mode": false,
        "autoplay_loading": false,
        "autoplay_items": [],
        "updated_at_ms": 0
    }))
    .expect("queue state");
    let renderer_state = super::QconnectSessionRendererState {
        active: Some(true),
        playing_state: Some(super::PLAYING_STATE_PLAYING),
        current_position_ms: Some(19_999),
        current_queue_item_id: Some(0),
        updated_at_ms: 12_345,
        ..Default::default()
    };

    let snapshot = super::session::build_session_renderer_snapshot(&queue, Some(&renderer_state), None);

    assert_eq!(snapshot.active, Some(true));
    assert_eq!(snapshot.playing_state, Some(super::PLAYING_STATE_PLAYING));
    assert_eq!(snapshot.current_position_ms, Some(19_999));
    assert_eq!(
        snapshot
            .current_track
            .as_ref()
            .map(|item| (item.track_id, item.queue_item_id)),
        Some((126886862, 0)),
    );
    assert_eq!(
        snapshot
            .next_track
            .as_ref()
            .map(|item| (item.track_id, item.queue_item_id)),
        Some((25584418, 1)),
    );
}

#[test]
fn session_renderer_snapshot_uses_session_loop_mode_when_renderer_loop_mode_missing() {
    let queue: QConnectQueueState = serde_json::from_value(json!({
        "version": { "major": 10, "minor": 1 },
        "queue_items": [
            { "track_context_uuid": "ctx", "track_id": 126886862, "queue_item_id": 126886862 }
        ],
        "shuffle_mode": false,
        "shuffle_order": null,
        "autoplay_mode": false,
        "autoplay_loading": false,
        "autoplay_items": [],
        "updated_at_ms": 0
    }))
    .expect("queue state");

    let snapshot = super::session::build_session_renderer_snapshot(
        &queue,
        Some(&super::QconnectSessionRendererState::default()),
        Some(2),
    );

    assert_eq!(snapshot.loop_mode, Some(2));
}

#[test]
fn effective_renderer_snapshot_prefers_session_cursor_over_stale_app_snapshot() {
    let queue: QConnectQueueState = serde_json::from_value(json!({
        "version": { "major": 10, "minor": 1 },
        "queue_items": [
            { "track_context_uuid": "ctx", "track_id": 126886862, "queue_item_id": 126886862 },
            { "track_context_uuid": "ctx", "track_id": 25584418, "queue_item_id": 1 },
            { "track_context_uuid": "ctx", "track_id": 25120807, "queue_item_id": 2 },
            { "track_context_uuid": "ctx", "track_id": 25584411, "queue_item_id": 3 }
        ],
        "shuffle_mode": false,
        "shuffle_order": null,
        "autoplay_mode": false,
        "autoplay_loading": false,
        "autoplay_items": [],
        "updated_at_ms": 0
    }))
    .expect("queue state");
    let base_renderer = QConnectRendererState {
        active: Some(true),
        playing_state: Some(super::PLAYING_STATE_PAUSED),
        current_position_ms: Some(3_000),
        current_track: Some(QueueItem {
            track_context_uuid: "ctx".to_string(),
            track_id: 126886862,
            queue_item_id: 0,
        }),
        next_track: Some(QueueItem {
            track_context_uuid: "ctx".to_string(),
            track_id: 25584418,
            queue_item_id: 1,
        }),
        updated_at_ms: 111,
        ..Default::default()
    };
    let renderer_state = super::QconnectSessionRendererState {
        active: Some(true),
        playing_state: Some(super::PLAYING_STATE_PAUSED),
        current_position_ms: Some(15_000),
        current_queue_item_id: Some(2),
        updated_at_ms: 222,
        ..Default::default()
    };

    let snapshot = super::build_effective_renderer_snapshot(
        &queue,
        &base_renderer,
        Some(&renderer_state),
        None,
    );

    assert_eq!(snapshot.current_position_ms, Some(15_000));
    assert_eq!(snapshot.updated_at_ms, 222);
    assert_eq!(
        snapshot
            .current_track
            .as_ref()
            .map(|item| (item.track_id, item.queue_item_id)),
        Some((25120807, 2)),
    );
    assert_eq!(
        snapshot
            .next_track
            .as_ref()
            .map(|item| (item.track_id, item.queue_item_id)),
        Some((25584411, 3)),
    );
}

#[test]
fn effective_renderer_snapshot_preserves_authoritative_renderer_next_track() {
    let queue: QConnectQueueState = serde_json::from_value(json!({
        "version": { "major": 22, "minor": 4 },
        "queue_items": [
            { "track_context_uuid": "ctx", "track_id": 43013244, "queue_item_id": 0 },
            { "track_context_uuid": "ctx", "track_id": 43013245, "queue_item_id": 1 },
            { "track_context_uuid": "ctx", "track_id": 43013246, "queue_item_id": 2 },
            { "track_context_uuid": "ctx", "track_id": 43013247, "queue_item_id": 3 },
            { "track_context_uuid": "ctx", "track_id": 43013248, "queue_item_id": 4 },
            { "track_context_uuid": "ctx", "track_id": 43013249, "queue_item_id": 5 },
            { "track_context_uuid": "ctx", "track_id": 43013250, "queue_item_id": 6 },
            { "track_context_uuid": "ctx", "track_id": 43013251, "queue_item_id": 7 },
            { "track_context_uuid": "ctx", "track_id": 43013252, "queue_item_id": 8 }
        ],
        "shuffle_mode": true,
        "shuffle_order": [0, 3, 6, 4, 5, 1, 7, 8, 2],
        "autoplay_mode": false,
        "autoplay_loading": false,
        "autoplay_items": [],
        "updated_at_ms": 0
    }))
    .expect("queue state");
    let base_renderer = QConnectRendererState {
        active: Some(true),
        playing_state: Some(super::PLAYING_STATE_PLAYING),
        current_position_ms: Some(41_000),
        current_track: Some(QueueItem {
            track_context_uuid: "ctx".to_string(),
            track_id: 43013244,
            queue_item_id: 0,
        }),
        next_track: Some(QueueItem {
            track_context_uuid: "ctx".to_string(),
            track_id: 43013251,
            queue_item_id: 7,
        }),
        updated_at_ms: 123,
        ..Default::default()
    };

    let snapshot = super::build_effective_renderer_snapshot(&queue, &base_renderer, None, None);

    assert_eq!(
        snapshot
            .current_track
            .as_ref()
            .map(|item| (item.track_id, item.queue_item_id)),
        Some((43013244, 0)),
    );
    assert_eq!(
        snapshot
            .next_track
            .as_ref()
            .map(|item| (item.track_id, item.queue_item_id)),
        Some((43013251, 7)),
    );
}

#[test]
fn visible_queue_projection_respects_remote_shuffle_order() {
    let queue: QConnectQueueState = serde_json::from_value(json!({
        "version": { "major": 40, "minor": 1 },
        "queue_items": [
            { "track_context_uuid": "ctx", "track_id": 101, "queue_item_id": 0 },
            { "track_context_uuid": "ctx", "track_id": 102, "queue_item_id": 1 },
            { "track_context_uuid": "ctx", "track_id": 103, "queue_item_id": 2 },
            { "track_context_uuid": "ctx", "track_id": 104, "queue_item_id": 3 },
            { "track_context_uuid": "ctx", "track_id": 105, "queue_item_id": 4 }
        ],
        "shuffle_mode": true,
        "shuffle_order": [0, 3, 1, 4, 2],
        "autoplay_mode": false,
        "autoplay_loading": false,
        "autoplay_items": [],
        "updated_at_ms": 0
    }))
    .expect("queue state");
    let renderer = QConnectRendererState {
        current_track: Some(QueueItem {
            track_context_uuid: "ctx".to_string(),
            track_id: 101,
            queue_item_id: 0,
        }),
        next_track: Some(QueueItem {
            track_context_uuid: "ctx".to_string(),
            track_id: 104,
            queue_item_id: 3,
        }),
        ..Default::default()
    };

    let projection = super::build_visible_queue_projection(&queue, &renderer);

    assert_eq!(
        projection
            .current_track
            .as_ref()
            .map(|item| (item.track_id, item.queue_item_id)),
        Some((101, 0)),
    );
    assert_eq!(
        projection
            .upcoming_tracks
            .iter()
            .map(|item| item.queue_item_id)
            .collect::<Vec<u64>>(),
        vec![3, 1, 4, 2],
    );
}

#[test]
fn visible_queue_projection_can_infer_current_from_next_anchor() {
    let queue: QConnectQueueState = serde_json::from_value(json!({
        "version": { "major": 41, "minor": 1 },
        "queue_items": [
            { "track_context_uuid": "ctx", "track_id": 201, "queue_item_id": 0 },
            { "track_context_uuid": "ctx", "track_id": 202, "queue_item_id": 1 },
            { "track_context_uuid": "ctx", "track_id": 203, "queue_item_id": 2 },
            { "track_context_uuid": "ctx", "track_id": 204, "queue_item_id": 3 }
        ],
        "shuffle_mode": true,
        "shuffle_order": [0, 3, 1, 2],
        "autoplay_mode": false,
        "autoplay_loading": false,
        "autoplay_items": [],
        "updated_at_ms": 0
    }))
    .expect("queue state");
    let renderer = QConnectRendererState {
        current_track: None,
        next_track: Some(QueueItem {
            track_context_uuid: "ctx".to_string(),
            track_id: 204,
            queue_item_id: 3,
        }),
        ..Default::default()
    };

    let projection = super::build_visible_queue_projection(&queue, &renderer);

    assert_eq!(
        projection
            .current_track
            .as_ref()
            .map(|item| (item.track_id, item.queue_item_id)),
        Some((201, 0)),
    );
    assert_eq!(
        projection
            .upcoming_tracks
            .iter()
            .map(|item| item.queue_item_id)
            .collect::<Vec<u64>>(),
        vec![3, 1, 2],
    );
}

#[test]
fn resolves_core_shuffle_order_with_current_and_renderer_next_anchor() {
    let queue: QConnectQueueState = serde_json::from_value(json!({
        "version": { "major": 31, "minor": 2 },
        "queue_items": [
            { "track_context_uuid": "ctx", "track_id": 72930174, "queue_item_id": 0 },
            { "track_context_uuid": "ctx", "track_id": 72930175, "queue_item_id": 1 },
            { "track_context_uuid": "ctx", "track_id": 72930176, "queue_item_id": 2 },
            { "track_context_uuid": "ctx", "track_id": 72930177, "queue_item_id": 3 },
            { "track_context_uuid": "ctx", "track_id": 72930178, "queue_item_id": 4 },
            { "track_context_uuid": "ctx", "track_id": 72930179, "queue_item_id": 5 },
            { "track_context_uuid": "ctx", "track_id": 72930180, "queue_item_id": 6 },
            { "track_context_uuid": "ctx", "track_id": 72930181, "queue_item_id": 7 },
            { "track_context_uuid": "ctx", "track_id": 72930182, "queue_item_id": 8 },
            { "track_context_uuid": "ctx", "track_id": 72930183, "queue_item_id": 9 }
        ],
        "shuffle_mode": true,
        "shuffle_order": [8, 5, 1, 9, 3, 4, 0, 6, 2, 7],
        "autoplay_mode": false,
        "autoplay_loading": false,
        "autoplay_items": [],
        "updated_at_ms": 0
    }))
    .expect("queue state");

    assert_eq!(
        super::queue_resolution::resolve_core_shuffle_order(
            &queue,
            Some(0),
            Some(72930174),
            Some(8),
            Some(72930182)
        ),
        Some(vec![0, 8, 5, 1, 9, 3, 4, 6, 2, 7]),
    );
}

#[test]
fn resolves_core_shuffle_order_keeps_current_first_for_resumed_remote_shuffle() {
    let queue: QConnectQueueState = serde_json::from_value(json!({
        "version": { "major": 30, "minor": 2 },
        "queue_items": [
            { "track_context_uuid": "ctx", "track_id": 43013244, "queue_item_id": 0 },
            { "track_context_uuid": "ctx", "track_id": 43013245, "queue_item_id": 1 },
            { "track_context_uuid": "ctx", "track_id": 43013246, "queue_item_id": 2 },
            { "track_context_uuid": "ctx", "track_id": 43013247, "queue_item_id": 3 },
            { "track_context_uuid": "ctx", "track_id": 43013248, "queue_item_id": 4 },
            { "track_context_uuid": "ctx", "track_id": 43013249, "queue_item_id": 5 },
            { "track_context_uuid": "ctx", "track_id": 43013250, "queue_item_id": 6 },
            { "track_context_uuid": "ctx", "track_id": 43013251, "queue_item_id": 7 },
            { "track_context_uuid": "ctx", "track_id": 43013252, "queue_item_id": 8 }
        ],
        "shuffle_mode": true,
        "shuffle_order": [0, 3, 6, 4, 5, 1, 7, 8, 2],
        "autoplay_mode": false,
        "autoplay_loading": false,
        "autoplay_items": [],
        "updated_at_ms": 0
    }))
    .expect("queue state");

    assert_eq!(
        super::queue_resolution::resolve_core_shuffle_order(
            &queue,
            Some(8),
            Some(43013252),
            Some(2),
            Some(43013246)
        ),
        Some(vec![8, 2, 0, 3, 6, 4, 5, 1, 7]),
    );
}

#[test]
fn reloads_remote_track_only_when_track_id_changed() {
    let playback_state = qbz_player::PlaybackState {
        is_playing: false,
        position: 0,
        duration: 279,
        track_id: 193849747,
        volume: 1.0,
    };

    // Same track: do not reload, even if buffering still in progress.
    assert!(!super::track_loading::should_reload_remote_track(
        &playback_state,
        193849747,
    ));
    // Different track: reload.
    assert!(super::track_loading::should_reload_remote_track(
        &playback_state,
        126886862,
    ));
}

#[test]
fn resolves_next_queue_item_id_from_shuffle_order() {
    let queue: QConnectQueueState = serde_json::from_value(json!({
        "version": { "major": 4, "minor": 1 },
        "queue_items": [
            { "track_context_uuid": "ctx", "track_id": 10, "queue_item_id": 100 },
            { "track_context_uuid": "ctx", "track_id": 20, "queue_item_id": 200 },
            { "track_context_uuid": "ctx", "track_id": 30, "queue_item_id": 300 }
        ],
        "shuffle_mode": true,
        "shuffle_order": [2, 0, 1],
        "autoplay_mode": false,
        "autoplay_loading": false,
        "autoplay_items": [],
        "updated_at_ms": 0
    }))
    .expect("queue state");

    assert_eq!(
        resolve_queue_item_ids_from_queue_state(&queue, 10),
        (Some(100), Some(200), Some(20)),
    );
}

#[test]
fn classifies_24_bit_streams_as_hires_level1() {
    assert_eq!(
        classify_qconnect_audio_quality(44_100, 24),
        AUDIO_QUALITY_HIRES_LEVEL1
    );
    assert_eq!(
        build_qconnect_file_audio_quality_snapshot(96_000, 24, 2),
        Some(QconnectFileAudioQualitySnapshot {
            sampling_rate: 96_000,
            bit_depth: 24,
            nb_channels: 2,
            audio_quality: AUDIO_QUALITY_HIRES_LEVEL1,
        }),
    );
}

#[test]
fn materialization_reapplies_same_version_when_shuffle_order_changes() {
    let previous: QConnectQueueState = serde_json::from_value(json!({
        "version": { "major": 28, "minor": 2 },
        "queue_items": [
            { "track_context_uuid": "ctx", "track_id": 1, "queue_item_id": 0 },
            { "track_context_uuid": "ctx", "track_id": 2, "queue_item_id": 1 },
            { "track_context_uuid": "ctx", "track_id": 3, "queue_item_id": 2 }
        ],
        "shuffle_mode": true,
        "shuffle_order": null,
        "autoplay_mode": false,
        "autoplay_loading": false,
        "autoplay_items": [],
        "updated_at_ms": 10
    }))
    .expect("previous queue state");

    let next: QConnectQueueState = serde_json::from_value(json!({
        "version": { "major": 28, "minor": 2 },
        "queue_items": [
            { "track_context_uuid": "ctx", "track_id": 1, "queue_item_id": 0 },
            { "track_context_uuid": "ctx", "track_id": 2, "queue_item_id": 1 },
            { "track_context_uuid": "ctx", "track_id": 3, "queue_item_id": 2 }
        ],
        "shuffle_mode": true,
        "shuffle_order": [0, 2, 1],
        "autoplay_mode": false,
        "autoplay_loading": false,
        "autoplay_items": [],
        "updated_at_ms": 20
    }))
    .expect("next queue state");

    assert!(super::corebridge::queue_state_needs_materialization(
        Some(&previous),
        &next
    ));
}

#[test]
fn materialization_skips_identical_snapshot_even_if_timestamp_changes() {
    let previous: QConnectQueueState = serde_json::from_value(json!({
        "version": { "major": 28, "minor": 2 },
        "queue_items": [
            { "track_context_uuid": "ctx", "track_id": 1, "queue_item_id": 0 },
            { "track_context_uuid": "ctx", "track_id": 2, "queue_item_id": 1 },
            { "track_context_uuid": "ctx", "track_id": 3, "queue_item_id": 2 }
        ],
        "shuffle_mode": true,
        "shuffle_order": [0, 2, 1],
        "autoplay_mode": false,
        "autoplay_loading": false,
        "autoplay_items": [],
        "updated_at_ms": 10
    }))
    .expect("previous queue state");

    let next: QConnectQueueState = serde_json::from_value(json!({
        "version": { "major": 28, "minor": 2 },
        "queue_items": [
            { "track_context_uuid": "ctx", "track_id": 1, "queue_item_id": 0 },
            { "track_context_uuid": "ctx", "track_id": 2, "queue_item_id": 1 },
            { "track_context_uuid": "ctx", "track_id": 3, "queue_item_id": 2 }
        ],
        "shuffle_mode": true,
        "shuffle_order": [0, 2, 1],
        "autoplay_mode": false,
        "autoplay_loading": false,
        "autoplay_items": [],
        "updated_at_ms": 20
    }))
    .expect("next queue state");

    assert!(!super::corebridge::queue_state_needs_materialization(
        Some(&previous),
        &next
    ));
}

#[test]
fn resolves_remote_next_target_using_renderer_next_queue_item_id() {
    let queue: QConnectQueueState = serde_json::from_value(json!({
        "version": { "major": 8, "minor": 2 },
        "queue_items": [
            { "track_context_uuid": "ctx", "track_id": 126886853, "queue_item_id": 126886853 },
            { "track_context_uuid": "ctx", "track_id": 123452387, "queue_item_id": 10 },
            { "track_context_uuid": "ctx", "track_id": 126886854, "queue_item_id": 1 }
        ],
        "shuffle_mode": false,
        "shuffle_order": null,
        "autoplay_mode": false,
        "autoplay_loading": false,
        "autoplay_items": [],
        "updated_at_ms": 0
    }))
    .expect("queue state");
    let renderer: QConnectRendererState = serde_json::from_value(json!({
        "current_track": { "track_context_uuid": "ctx", "track_id": 126886853, "queue_item_id": 126886853 },
        "next_track": { "track_context_uuid": "ctx", "track_id": 126886854, "queue_item_id": 1 },
        "current_position_ms": 64_000,
        "playing_state": 2,
        "updated_at_ms": 0
    }))
    .expect("renderer state");

    assert_eq!(
        resolve_controller_queue_item_from_snapshots(
            &queue,
            &renderer,
            QconnectRemoteSkipDirection::Next,
        ),
        super::queue_resolution::QconnectControllerQueueItemResolution {
            target_queue_item_id: Some(1),
            strategy: "renderer_next_queue_item_id_verified",
            queue_index: Some(2),
            matched_track_id: Some(126886854),
            matched_queue_item_id: Some(1),
        }
    );
}

#[test]
fn resolves_remote_previous_to_restart_first_cloud_item_when_mid_track() {
    let queue: QConnectQueueState = serde_json::from_value(json!({
        "version": { "major": 8, "minor": 2 },
        "queue_items": [
            { "track_context_uuid": "ctx", "track_id": 126886853, "queue_item_id": 126886853 },
            { "track_context_uuid": "ctx", "track_id": 123452387, "queue_item_id": 10 },
            { "track_context_uuid": "ctx", "track_id": 126886854, "queue_item_id": 1 }
        ],
        "shuffle_mode": false,
        "shuffle_order": null,
        "autoplay_mode": false,
        "autoplay_loading": false,
        "autoplay_items": [],
        "updated_at_ms": 0
    }))
    .expect("queue state");
    let renderer: QConnectRendererState = serde_json::from_value(json!({
        "current_track": { "track_context_uuid": "ctx", "track_id": 126886853, "queue_item_id": 126886853 },
        "next_track": { "track_context_uuid": "ctx", "track_id": 126886854, "queue_item_id": 1 },
        "current_position_ms": 64_000,
        "playing_state": 2,
        "updated_at_ms": 0
    }))
    .expect("renderer state");

    assert_eq!(
        resolve_controller_queue_item_from_snapshots(
            &queue,
            &renderer,
            QconnectRemoteSkipDirection::Previous,
        ),
        super::queue_resolution::QconnectControllerQueueItemResolution {
            target_queue_item_id: Some(0),
            strategy: "restart_current_queue_item",
            queue_index: Some(0),
            matched_track_id: Some(126886853),
            matched_queue_item_id: Some(0),
        }
    );
}

#[test]
fn resolves_remote_previous_to_prior_item_when_near_track_start() {
    let queue: QConnectQueueState = serde_json::from_value(json!({
        "version": { "major": 8, "minor": 2 },
        "queue_items": [
            { "track_context_uuid": "ctx", "track_id": 126886853, "queue_item_id": 126886853 },
            { "track_context_uuid": "ctx", "track_id": 123452387, "queue_item_id": 10 },
            { "track_context_uuid": "ctx", "track_id": 126886854, "queue_item_id": 1 }
        ],
        "shuffle_mode": false,
        "shuffle_order": null,
        "autoplay_mode": false,
        "autoplay_loading": false,
        "autoplay_items": [],
        "updated_at_ms": 0
    }))
    .expect("queue state");
    let renderer: QConnectRendererState = serde_json::from_value(json!({
        "current_track": { "track_context_uuid": "ctx", "track_id": 123452387, "queue_item_id": 10 },
        "next_track": { "track_context_uuid": "ctx", "track_id": 126886854, "queue_item_id": 1 },
        "current_position_ms": 2_000,
        "playing_state": 2,
        "updated_at_ms": 0
    }))
    .expect("renderer state");

    assert_eq!(
        resolve_controller_queue_item_from_snapshots(
            &queue,
            &renderer,
            QconnectRemoteSkipDirection::Previous,
        ),
        super::queue_resolution::QconnectControllerQueueItemResolution {
            target_queue_item_id: Some(0),
            strategy: "queue_item_before_current",
            queue_index: Some(0),
            matched_track_id: Some(126886853),
            matched_queue_item_id: Some(0),
        }
    );
}

#[test]
fn resolves_remote_previous_to_prior_item_even_mid_track() {
    let queue: QConnectQueueState = serde_json::from_value(json!({
        "version": { "major": 8, "minor": 2 },
        "queue_items": [
            { "track_context_uuid": "ctx", "track_id": 126886853, "queue_item_id": 126886853 },
            { "track_context_uuid": "ctx", "track_id": 123452387, "queue_item_id": 10 },
            { "track_context_uuid": "ctx", "track_id": 126886854, "queue_item_id": 1 }
        ],
        "shuffle_mode": false,
        "shuffle_order": null,
        "autoplay_mode": false,
        "autoplay_loading": false,
        "autoplay_items": [],
        "updated_at_ms": 0
    }))
    .expect("queue state");
    let renderer: QConnectRendererState = serde_json::from_value(json!({
        "current_track": { "track_context_uuid": "ctx", "track_id": 123452387, "queue_item_id": 10 },
        "next_track": { "track_context_uuid": "ctx", "track_id": 126886854, "queue_item_id": 1 },
        "current_position_ms": 64_000,
        "playing_state": 2,
        "updated_at_ms": 0
    }))
    .expect("renderer state");

    assert_eq!(
        resolve_controller_queue_item_from_snapshots(
            &queue,
            &renderer,
            QconnectRemoteSkipDirection::Previous,
        ),
        super::queue_resolution::QconnectControllerQueueItemResolution {
            target_queue_item_id: Some(0),
            strategy: "queue_item_before_current",
            queue_index: Some(0),
            matched_track_id: Some(126886853),
            matched_queue_item_id: Some(0),
        }
    );
}
