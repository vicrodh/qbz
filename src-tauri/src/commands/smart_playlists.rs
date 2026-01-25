//! Smart Playlist Tauri commands
//!
//! Generates playlists based on MusicBrainz artist relationships

use tauri::State;

use crate::api::QobuzClient;
use crate::musicbrainz::smart_playlists::{
    GeneratedPlaylist, PlaylistGenerationConfig, PlaylistRule,
};
use crate::musicbrainz::MusicBrainzSharedState;
use crate::AppState;

/// Preview a smart playlist (get track list without creating)
#[tauri::command]
pub async fn smart_playlist_preview(
    rule: PlaylistRule,
    config: Option<PlaylistGenerationConfig>,
    mb_state: State<'_, MusicBrainzSharedState>,
    app_state: State<'_, AppState>,
) -> Result<GeneratedPlaylist, String> {
    let config = config.unwrap_or_default();

    // Check if MusicBrainz is enabled
    if !mb_state.client.is_enabled().await {
        return Err("MusicBrainz integration is disabled".to_string());
    }

    match &rule {
        PlaylistRule::BandMembers {
            artist_mbid,
            include_past_members,
        } => {
            generate_band_members_playlist(
                artist_mbid,
                *include_past_members,
                &config,
                &mb_state,
                &app_state,
            )
            .await
        }
        PlaylistRule::ArtistGroups { artist_mbid } => {
            generate_artist_groups_playlist(artist_mbid, &config, &mb_state, &app_state).await
        }
        PlaylistRule::CollaboratorNetwork { artist_mbid, depth } => {
            generate_collaborator_playlist(artist_mbid, *depth, &config, &mb_state, &app_state)
                .await
        }
    }
}

/// Generate playlist from band members' solo work
async fn generate_band_members_playlist(
    artist_mbid: &str,
    include_past_members: bool,
    config: &PlaylistGenerationConfig,
    mb_state: &State<'_, MusicBrainzSharedState>,
    app_state: &State<'_, AppState>,
) -> Result<GeneratedPlaylist, String> {
    // Get relationships
    let relationships = mb_state
        .client
        .get_artist_with_relations(artist_mbid)
        .await?;

    // Get the band name for the playlist title
    let band_name = relationships.name.clone();

    // Collect member names
    let mut member_names: Vec<String> = Vec::new();

    if let Some(relations) = &relationships.relations {
        for relation in relations {
            if relation.relation_type == "member of band" {
                if let Some(ref artist) = relation.artist {
                    // Check direction - we want members of this band
                    if relation.direction.as_deref() != Some("backward") {
                        // This is a member
                        let is_past = relation.ended == Some(true);
                        if !is_past || include_past_members {
                            member_names.push(artist.name.clone());
                        }
                    }
                }
            }
        }
    }

    if member_names.is_empty() {
        return Ok(GeneratedPlaylist {
            suggested_name: format!("{} Members", band_name),
            description: "No band members found in MusicBrainz".to_string(),
            rule: PlaylistRule::BandMembers {
                artist_mbid: artist_mbid.to_string(),
                include_past_members,
            },
            included_artists: vec![],
            track_ids: vec![],
            total_tracks_found: 0,
        });
    }

    // Search Qobuz for each member and get their tracks
    let mut all_track_ids: Vec<u64> = Vec::new();
    let mut found_artists: Vec<String> = Vec::new();

    let client = app_state.client.lock().await;

    for member_name in &member_names {
        match search_artist_tracks(&client, member_name, config.max_tracks_per_artist).await {
            Ok(tracks) => {
                if !tracks.is_empty() {
                    found_artists.push(member_name.clone());
                    all_track_ids.extend(tracks);
                }
            }
            Err(e) => {
                log::warn!("Failed to search tracks for {}: {}", member_name, e);
            }
        }

        // Check if we've hit the limit
        if all_track_ids.len() >= config.max_total_tracks {
            break;
        }
    }

    let total_found = all_track_ids.len();
    all_track_ids.truncate(config.max_total_tracks);

    Ok(GeneratedPlaylist {
        suggested_name: format!("{} Members", band_name),
        description: format!(
            "Solo work by members of {}{}",
            band_name,
            if include_past_members {
                " (including past members)"
            } else {
                ""
            }
        ),
        rule: PlaylistRule::BandMembers {
            artist_mbid: artist_mbid.to_string(),
            include_past_members,
        },
        included_artists: found_artists,
        track_ids: all_track_ids,
        total_tracks_found: total_found,
    })
}

/// Generate playlist from groups an artist is/was a member of
async fn generate_artist_groups_playlist(
    artist_mbid: &str,
    config: &PlaylistGenerationConfig,
    mb_state: &State<'_, MusicBrainzSharedState>,
    app_state: &State<'_, AppState>,
) -> Result<GeneratedPlaylist, String> {
    // Get relationships
    let relationships = mb_state
        .client
        .get_artist_with_relations(artist_mbid)
        .await?;

    let artist_name = relationships.name.clone();

    // Collect group names
    let mut group_names: Vec<String> = Vec::new();

    if let Some(relations) = &relationships.relations {
        for relation in relations {
            if relation.relation_type == "member of band" {
                if let Some(ref artist) = relation.artist {
                    // Check direction - "backward" means this artist is a member OF that group
                    if relation.direction.as_deref() == Some("backward") {
                        group_names.push(artist.name.clone());
                    }
                }
            }
        }
    }

    if group_names.is_empty() {
        return Ok(GeneratedPlaylist {
            suggested_name: format!("{}'s Groups", artist_name),
            description: "No groups found in MusicBrainz".to_string(),
            rule: PlaylistRule::ArtistGroups {
                artist_mbid: artist_mbid.to_string(),
            },
            included_artists: vec![],
            track_ids: vec![],
            total_tracks_found: 0,
        });
    }

    // Search Qobuz for each group and get their tracks
    let mut all_track_ids: Vec<u64> = Vec::new();
    let mut found_artists: Vec<String> = Vec::new();

    let client = app_state.client.lock().await;

    for group_name in &group_names {
        match search_artist_tracks(&client, group_name, config.max_tracks_per_artist).await {
            Ok(tracks) => {
                if !tracks.is_empty() {
                    found_artists.push(group_name.clone());
                    all_track_ids.extend(tracks);
                }
            }
            Err(e) => {
                log::warn!("Failed to search tracks for {}: {}", group_name, e);
            }
        }

        if all_track_ids.len() >= config.max_total_tracks {
            break;
        }
    }

    let total_found = all_track_ids.len();
    all_track_ids.truncate(config.max_total_tracks);

    Ok(GeneratedPlaylist {
        suggested_name: format!("{}'s Groups", artist_name),
        description: format!("Tracks from groups {} is/was a member of", artist_name),
        rule: PlaylistRule::ArtistGroups {
            artist_mbid: artist_mbid.to_string(),
        },
        included_artists: found_artists,
        track_ids: all_track_ids,
        total_tracks_found: total_found,
    })
}

/// Generate playlist from collaborators
async fn generate_collaborator_playlist(
    artist_mbid: &str,
    depth: u8,
    config: &PlaylistGenerationConfig,
    mb_state: &State<'_, MusicBrainzSharedState>,
    app_state: &State<'_, AppState>,
) -> Result<GeneratedPlaylist, String> {
    // For now, only support depth=1 (direct collaborators)
    if depth > 1 {
        return Err("Collaborator depth > 1 not yet supported".to_string());
    }

    // Get relationships
    let relationships = mb_state
        .client
        .get_artist_with_relations(artist_mbid)
        .await?;

    let artist_name = relationships.name.clone();

    // Collect collaborator names
    let mut collaborator_names: Vec<String> = Vec::new();

    if let Some(relations) = &relationships.relations {
        for relation in relations {
            if relation.relation_type == "collaboration" {
                if let Some(ref artist) = relation.artist {
                    collaborator_names.push(artist.name.clone());
                }
            }
        }
    }

    if collaborator_names.is_empty() {
        return Ok(GeneratedPlaylist {
            suggested_name: format!("{} Collaborations", artist_name),
            description: "No collaborators found in MusicBrainz".to_string(),
            rule: PlaylistRule::CollaboratorNetwork {
                artist_mbid: artist_mbid.to_string(),
                depth,
            },
            included_artists: vec![],
            track_ids: vec![],
            total_tracks_found: 0,
        });
    }

    // Search Qobuz for each collaborator
    let mut all_track_ids: Vec<u64> = Vec::new();
    let mut found_artists: Vec<String> = Vec::new();

    let client = app_state.client.lock().await;

    for collab_name in &collaborator_names {
        match search_artist_tracks(&client, collab_name, config.max_tracks_per_artist).await {
            Ok(tracks) => {
                if !tracks.is_empty() {
                    found_artists.push(collab_name.clone());
                    all_track_ids.extend(tracks);
                }
            }
            Err(e) => {
                log::warn!("Failed to search tracks for {}: {}", collab_name, e);
            }
        }

        if all_track_ids.len() >= config.max_total_tracks {
            break;
        }
    }

    let total_found = all_track_ids.len();
    all_track_ids.truncate(config.max_total_tracks);

    Ok(GeneratedPlaylist {
        suggested_name: format!("{} Collaborations", artist_name),
        description: format!("Tracks from artists who have collaborated with {}", artist_name),
        rule: PlaylistRule::CollaboratorNetwork {
            artist_mbid: artist_mbid.to_string(),
            depth,
        },
        included_artists: found_artists,
        track_ids: all_track_ids,
        total_tracks_found: total_found,
    })
}

/// Search for an artist on Qobuz and get their top tracks
async fn search_artist_tracks(
    client: &QobuzClient,
    artist_name: &str,
    max_tracks: usize,
) -> Result<Vec<u64>, String> {
    // Search for the artist
    let search_results = client
        .search_artists(artist_name, 5, 0)
        .await
        .map_err(|e| format!("Search failed: {}", e))?;

    if search_results.items.is_empty() {
        return Ok(vec![]);
    }

    // Find exact or close match
    let artist = search_results
        .items
        .iter()
        .find(|a| a.name.to_lowercase() == artist_name.to_lowercase())
        .or_else(|| search_results.items.first());

    let Some(artist) = artist else {
        return Ok(vec![]);
    };

    let artist_id = artist.id;

    // Get the artist's top tracks by searching with artist name
    let track_results = client
        .search_tracks(artist_name, max_tracks as u32, 0)
        .await
        .map_err(|e| format!("Track search failed: {}", e))?;

    // Filter tracks by this specific artist and extract IDs
    let track_ids: Vec<u64> = track_results
        .items
        .iter()
        .filter(|t| {
            // Check if performer matches
            t.performer
                .as_ref()
                .map(|p| p.id == artist_id)
                .unwrap_or(false)
        })
        .map(|t| t.id)
        .take(max_tracks)
        .collect();

    Ok(track_ids)
}

/// Get the artist MBID for a given artist name (helper for frontend)
#[tauri::command]
pub async fn smart_playlist_resolve_artist(
    artist_name: String,
    mb_state: State<'_, MusicBrainzSharedState>,
) -> Result<Option<String>, String> {
    if !mb_state.client.is_enabled().await {
        return Err("MusicBrainz integration is disabled".to_string());
    }

    let result = mb_state.client.search_artist(&artist_name).await?;

    // Find best match with high confidence
    let best_match = result
        .artists
        .iter()
        .find(|a| a.score.unwrap_or(0) >= 90);

    Ok(best_match.map(|a| a.id.clone()))
}

/// Get available playlist types for an artist
#[tauri::command]
pub async fn smart_playlist_get_available_types(
    artist_mbid: String,
    mb_state: State<'_, MusicBrainzSharedState>,
) -> Result<Vec<String>, String> {
    if !mb_state.client.is_enabled().await {
        return Err("MusicBrainz integration is disabled".to_string());
    }

    let relationships = mb_state
        .client
        .get_artist_with_relations(&artist_mbid)
        .await?;

    let mut available: Vec<String> = Vec::new();

    if let Some(relations) = &relationships.relations {
        let mut has_members = false;
        let mut has_groups = false;
        let mut has_collaborators = false;

        for relation in relations {
            match relation.relation_type.as_str() {
                "member of band" => {
                    if relation.direction.as_deref() == Some("backward") {
                        has_groups = true;
                    } else {
                        has_members = true;
                    }
                }
                "collaboration" => {
                    has_collaborators = true;
                }
                _ => {}
            }
        }

        if has_members {
            available.push("band_members".to_string());
        }
        if has_groups {
            available.push("artist_groups".to_string());
        }
        if has_collaborators {
            available.push("collaborator_network".to_string());
        }
    }

    Ok(available)
}
