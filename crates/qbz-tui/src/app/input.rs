//! Input handling for the TUI event loop.
//!
//! Dispatches keyboard and mouse events to the correct handler based on
//! current modal state, global keybindings, and focus section.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use qbz_models::RepeatMode;

use super::App;
use super::state::{ActiveView, FocusSection, ModalType};

// ============ Keyboard Input ============

/// Top-level key dispatch.
///
/// Priority order:
/// 1. Modal override (if `active_modal` is Some, capture ALL keys)
/// 2. Global keybindings (Ctrl+key combos, always active)
/// 3. Playback controls (global when no modal is open)
/// 4. Focus-dependent navigation
pub fn handle_key(app: &mut App, key: KeyEvent) {
    // 1. Modal override
    if app.state.active_modal.is_some() {
        handle_modal_key(app, key);
        return;
    }

    // 2. Global keybindings (Ctrl+key)
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        if handle_global_ctrl(app, key) {
            return;
        }
    }

    // 3. Tab for focus cycling (no modifiers)
    if key.code == KeyCode::Tab && key.modifiers.is_empty() {
        cycle_focus(app);
        return;
    }

    // 4. Playback controls (global, no Ctrl needed)
    if handle_playback_key(app, key) {
        return;
    }

    // 5. Focus-dependent navigation
    match app.state.focus {
        FocusSection::Main => handle_main_key(app, key),
        FocusSection::Sidebar => handle_sidebar_key(app, key),
        FocusSection::Queue => handle_queue_key(app, key),
    }
}

/// Handle Ctrl+key global shortcuts. Returns true if consumed.
fn handle_global_ctrl(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('d') => {
            navigate_to(app, ActiveView::Discover);
            true
        }
        KeyCode::Char('f') => {
            navigate_to(app, ActiveView::Favorites);
            true
        }
        KeyCode::Char('l') => {
            navigate_to(app, ActiveView::Library);
            true
        }
        KeyCode::Char('s') => {
            navigate_to(app, ActiveView::Settings);
            true
        }
        KeyCode::Char('x') => {
            navigate_back(app);
            true
        }
        KeyCode::Char('q') => {
            app.should_quit = true;
            true
        }
        KeyCode::Char('/') => {
            app.state.active_modal = Some(ModalType::Search);
            true
        }
        _ => false,
    }
}

/// Handle playback keys (global when no modal is open). Returns true if consumed.
fn handle_playback_key(app: &mut App, key: KeyEvent) -> bool {
    // Only handle plain keys (no Ctrl/Alt modifiers) for playback
    if !key.modifiers.is_empty()
        && !key.modifiers.contains(KeyModifiers::SHIFT)
    {
        return false;
    }

    match key.code {
        KeyCode::Char(' ') => {
            toggle_play_pause(app);
            true
        }
        KeyCode::Char('n') => {
            // Next track
            let core = app.core.clone();
            tokio::spawn(async move {
                let _ = core.next_track().await;
            });
            true
        }
        KeyCode::Char('p') => {
            // Previous track
            let core = app.core.clone();
            tokio::spawn(async move {
                let _ = core.previous_track().await;
            });
            true
        }
        KeyCode::Left => {
            // Seek back 5s
            let pos = app.state.playback.position_secs;
            let new_pos = pos.saturating_sub(5);
            let _ = app.core.seek(new_pos);
            true
        }
        KeyCode::Right => {
            // Seek forward 5s
            let pos = app.state.playback.position_secs;
            let dur = app.state.playback.duration_secs;
            let new_pos = (pos + 5).min(dur);
            let _ = app.core.seek(new_pos);
            true
        }
        KeyCode::Char('+') | KeyCode::Char('=') => {
            let vol = (app.state.playback.volume + 0.05).min(1.0);
            let _ = app.core.set_volume(vol);
            app.state.playback.volume = vol;
            true
        }
        KeyCode::Char('-') => {
            let vol = (app.state.playback.volume - 0.05).max(0.0);
            let _ = app.core.set_volume(vol);
            app.state.playback.volume = vol;
            true
        }
        KeyCode::Char('s') => {
            let core = app.core.clone();
            tokio::spawn(async move {
                core.toggle_shuffle().await;
            });
            true
        }
        KeyCode::Char('r') => {
            cycle_repeat(app);
            true
        }
        _ => false,
    }
}

/// Cycle repeat mode: Off -> All -> One -> Off
fn cycle_repeat(app: &mut App) {
    let core = app.core.clone();
    let queue = app.core.queue();
    tokio::spawn(async move {
        let current = {
            let q = queue.read().await;
            q.get_repeat()
        };
        let next_mode = match current {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        };
        core.set_repeat_mode(next_mode).await;
    });
}

/// Toggle play/pause based on current state.
fn toggle_play_pause(app: &mut App) {
    if app.state.playback.is_playing {
        let _ = app.core.pause();
    } else {
        let _ = app.core.resume();
    }
}

// ============ Focus Cycling ============

/// Cycle focus: Sidebar -> Main -> Queue -> Sidebar
fn cycle_focus(app: &mut App) {
    app.state.focus = match app.state.focus {
        FocusSection::Sidebar => FocusSection::Main,
        FocusSection::Main => FocusSection::Queue,
        FocusSection::Queue => FocusSection::Sidebar,
    };
    log::debug!("[TUI] Focus changed to {:?}", app.state.focus);
}

// ============ Navigation ============

/// Navigate to a view, pushing current view onto the stack.
fn navigate_to(app: &mut App, view: ActiveView) {
    if app.state.active_view == view {
        return;
    }
    app.state.view_stack.push(app.state.active_view.clone());
    app.state.active_view = view;
    app.state.focus = FocusSection::Main;
    log::debug!(
        "[TUI] Navigated to {:?} (stack depth: {})",
        app.state.active_view,
        app.state.view_stack.len()
    );
}

/// Navigate back (pop view stack).
fn navigate_back(app: &mut App) {
    if let Some(prev) = app.state.view_stack.pop() {
        app.state.active_view = prev;
        app.state.focus = FocusSection::Main;
        log::debug!(
            "[TUI] Navigated back to {:?} (stack depth: {})",
            app.state.active_view,
            app.state.view_stack.len()
        );
    }
}

// ============ Focus-Dependent Handlers ============

/// Handle keys when focus is on the Main panel.
fn handle_main_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Up => {
            adjust_selected_index(app, -1);
        }
        KeyCode::Down => {
            adjust_selected_index(app, 1);
        }
        KeyCode::Enter => {
            // TODO: View-specific activation (Task 10+)
            log::debug!(
                "[TUI] Enter pressed on {:?} index {}",
                app.state.active_view,
                get_selected_index(app),
            );
        }
        KeyCode::Esc => {
            // Esc with no modal: could go back or clear selection
            navigate_back(app);
        }
        _ => {}
    }
}

/// Handle keys when focus is on the Sidebar.
fn handle_sidebar_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Up => {
            app.state.sidebar.selected_index =
                app.state.sidebar.selected_index.saturating_sub(1);
        }
        KeyCode::Down => {
            // Sidebar items are nav entries + playlists; cap at a reasonable max
            let max = 5 + app.state.sidebar.playlists.len();
            if app.state.sidebar.selected_index + 1 < max {
                app.state.sidebar.selected_index += 1;
            }
        }
        KeyCode::Enter => {
            // TODO: Sidebar item activation (navigate to section/playlist)
            log::debug!(
                "[TUI] Sidebar Enter at index {}",
                app.state.sidebar.selected_index
            );
        }
        _ => {}
    }
}

/// Handle keys when focus is on the Queue panel.
fn handle_queue_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Up => {
            app.state.queue.selected_index =
                app.state.queue.selected_index.saturating_sub(1);
        }
        KeyCode::Down => {
            // Queue size isn't known here yet; will be capped by render
            app.state.queue.selected_index += 1;
        }
        KeyCode::Enter => {
            // TODO: Jump to selected queue track (Task 10)
            log::debug!(
                "[TUI] Queue Enter at index {}",
                app.state.queue.selected_index
            );
        }
        _ => {}
    }
}

// ============ Modal Key Handling ============

/// Capture all keys when a modal is open.
fn handle_modal_key(app: &mut App, key: KeyEvent) {
    // Esc always closes any modal
    if key.code == KeyCode::Esc {
        app.state.active_modal = None;
        return;
    }

    match &app.state.active_modal {
        Some(ModalType::Login) => handle_login_modal_key(app, key),
        Some(ModalType::Search) => handle_search_modal_key(app, key),
        Some(ModalType::DevicePicker) => handle_device_picker_key(app, key),
        None => {} // unreachable, guarded above
    }
}

/// Login modal key handler.
fn handle_login_modal_key(app: &mut App, key: KeyEvent) {
    use super::state::LoginField;

    match key.code {
        KeyCode::Tab => {
            // Toggle between email and password fields
            app.state.login.active_field = match app.state.login.active_field {
                LoginField::Email => LoginField::Password,
                LoginField::Password => LoginField::Email,
            };
        }
        KeyCode::Char(ch) => {
            match app.state.login.active_field {
                LoginField::Email => {
                    app.state.login.email.insert(app.state.login.email_cursor, ch);
                    app.state.login.email_cursor += 1;
                }
                LoginField::Password => {
                    app.state.login.password.insert(app.state.login.password_cursor, ch);
                    app.state.login.password_cursor += 1;
                }
            }
        }
        KeyCode::Backspace => {
            match app.state.login.active_field {
                LoginField::Email => {
                    if app.state.login.email_cursor > 0 {
                        app.state.login.email_cursor -= 1;
                        app.state.login.email.remove(app.state.login.email_cursor);
                    }
                }
                LoginField::Password => {
                    if app.state.login.password_cursor > 0 {
                        app.state.login.password_cursor -= 1;
                        app.state.login.password.remove(app.state.login.password_cursor);
                    }
                }
            }
        }
        KeyCode::Enter => {
            if !app.state.login.logging_in {
                // TODO: Trigger actual login via core (Task 20)
                log::debug!("[TUI] Login submitted for {}", app.state.login.email);
            }
        }
        _ => {}
    }
}

/// Search modal key handler.
fn handle_search_modal_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char(ch) => {
            app.state.search.query.insert(app.state.search.cursor_pos, ch);
            app.state.search.cursor_pos += 1;
        }
        KeyCode::Backspace => {
            if app.state.search.cursor_pos > 0 {
                app.state.search.cursor_pos -= 1;
                app.state.search.query.remove(app.state.search.cursor_pos);
            }
        }
        KeyCode::Enter => {
            // TODO: Execute search (Task 15)
            log::debug!("[TUI] Search submitted: {}", app.state.search.query);
        }
        KeyCode::Up => {
            app.state.search.selected_index =
                app.state.search.selected_index.saturating_sub(1);
        }
        KeyCode::Down => {
            app.state.search.selected_index += 1;
        }
        _ => {}
    }
}

/// Device picker modal key handler.
fn handle_device_picker_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Up => {
            app.state.device_picker.selected_index =
                app.state.device_picker.selected_index.saturating_sub(1);
        }
        KeyCode::Down => {
            let max = app.state.device_picker.devices.len().saturating_sub(1);
            if app.state.device_picker.selected_index < max {
                app.state.device_picker.selected_index += 1;
            }
        }
        KeyCode::Enter => {
            // TODO: Switch audio device (Task 20)
            log::debug!(
                "[TUI] Device picker selected index {}",
                app.state.device_picker.selected_index
            );
        }
        _ => {}
    }
}

// ============ Helpers ============

/// Get the selected index for the current active view.
fn get_selected_index(app: &App) -> usize {
    match app.state.active_view {
        ActiveView::Discover => app.state.discover.selected_index,
        ActiveView::Favorites => match app.state.favorites.tab {
            super::state::FavoritesTab::Tracks => app.state.favorites.selected_index_tracks,
            super::state::FavoritesTab::Albums => app.state.favorites.selected_index_albums,
            super::state::FavoritesTab::Artists => app.state.favorites.selected_index_artists,
            super::state::FavoritesTab::Playlists => app.state.favorites.selected_index_playlists,
        },
        ActiveView::Library => app.state.library.selected_index,
        ActiveView::Search => app.state.search.selected_index,
        ActiveView::AlbumDetail => app.state.album_detail.selected_index,
        ActiveView::ArtistDetail => app.state.artist_detail.selected_index,
        ActiveView::PlaylistDetail => app.state.playlist_detail.selected_index,
        ActiveView::Settings => app.state.settings.selected_index,
        ActiveView::Purchases => 0, // TODO: purchases state
    }
}

/// Adjust the selected index for the current active view by `delta`.
fn adjust_selected_index(app: &mut App, delta: i32) {
    match app.state.active_view {
        ActiveView::Discover => {
            apply_delta(&mut app.state.discover.selected_index, delta);
        }
        ActiveView::Favorites => match app.state.favorites.tab {
            super::state::FavoritesTab::Tracks => {
                apply_delta(&mut app.state.favorites.selected_index_tracks, delta);
            }
            super::state::FavoritesTab::Albums => {
                apply_delta(&mut app.state.favorites.selected_index_albums, delta);
            }
            super::state::FavoritesTab::Artists => {
                apply_delta(&mut app.state.favorites.selected_index_artists, delta);
            }
            super::state::FavoritesTab::Playlists => {
                apply_delta(&mut app.state.favorites.selected_index_playlists, delta);
            }
        },
        ActiveView::Library => {
            apply_delta(&mut app.state.library.selected_index, delta);
        }
        ActiveView::Search => {
            apply_delta(&mut app.state.search.selected_index, delta);
        }
        ActiveView::AlbumDetail => {
            apply_delta(&mut app.state.album_detail.selected_index, delta);
        }
        ActiveView::ArtistDetail => {
            apply_delta(&mut app.state.artist_detail.selected_index, delta);
        }
        ActiveView::PlaylistDetail => {
            apply_delta(&mut app.state.playlist_detail.selected_index, delta);
        }
        ActiveView::Settings => {
            apply_delta(&mut app.state.settings.selected_index, delta);
        }
        ActiveView::Purchases => {
            // TODO: purchases selected index
        }
    }
}

/// Apply a signed delta to a usize index (clamped at 0 on the low end).
fn apply_delta(index: &mut usize, delta: i32) {
    if delta < 0 {
        *index = index.saturating_sub(delta.unsigned_abs() as usize);
    } else {
        *index += delta as usize;
    }
}

// ============ Mouse Input ============

/// Top-level mouse dispatch.
pub fn handle_mouse(app: &mut App, mouse: MouseEvent) {
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            // Scroll active list up
            match app.state.focus {
                FocusSection::Main => adjust_selected_index(app, -3),
                FocusSection::Queue => {
                    app.state.queue.selected_index =
                        app.state.queue.selected_index.saturating_sub(3);
                }
                FocusSection::Sidebar => {
                    app.state.sidebar.selected_index =
                        app.state.sidebar.selected_index.saturating_sub(3);
                }
            }
        }
        MouseEventKind::ScrollDown => {
            match app.state.focus {
                FocusSection::Main => adjust_selected_index(app, 3),
                FocusSection::Queue => {
                    app.state.queue.selected_index += 3;
                }
                FocusSection::Sidebar => {
                    app.state.sidebar.selected_index += 3;
                }
            }
        }
        MouseEventKind::Down(_) => {
            // Click hit-testing deferred to Task 22
            log::trace!(
                "[TUI] Mouse click at ({}, {})",
                mouse.column,
                mouse.row
            );
        }
        _ => {}
    }
}
