//! Integrations wizard section.

pub fn run_integrations_wizard() -> Result<(), String> {
    println!("\n=== Integrations ===\n");

    // ListenBrainz
    println!("ListenBrainz token (leave empty to skip):");
    let mut lb_token = String::new();
    std::io::stdin().read_line(&mut lb_token).unwrap_or_default();
    let lb_token = lb_token.trim();

    if !lb_token.is_empty() {
        // Save to the V2 cache database
        let data_dir = dirs::data_dir()
            .ok_or("Cannot determine data directory")?
            .join("qbz");

        // Find the user's data dir (from last_user_id)
        let user_dir = find_user_data_dir(&data_dir)?;
        let cache_dir = user_dir.join("cache");
        std::fs::create_dir_all(&cache_dir).ok();
        let db_path = cache_dir.join("listenbrainz_v2.db");

        let conn = rusqlite::Connection::open(&db_path)
            .map_err(|e| format!("DB error: {}", e))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; \
             CREATE TABLE IF NOT EXISTS config (key TEXT PRIMARY KEY, value TEXT NOT NULL)"
        ).map_err(|e| format!("Schema error: {}", e))?;
        conn.execute(
            "INSERT OR REPLACE INTO config (key, value) VALUES ('token', ?1)",
            rusqlite::params![lb_token],
        ).map_err(|e| format!("Save error: {}", e))?;

        println!("ListenBrainz token saved.\n");
    } else {
        println!("ListenBrainz skipped.\n");
    }

    // MusicBrainz
    println!("Enable MusicBrainz enrichment? [Y/n]:");
    let mut mb_input = String::new();
    std::io::stdin().read_line(&mut mb_input).unwrap_or_default();
    let mb_enabled = !mb_input.trim().eq_ignore_ascii_case("n");
    println!("MusicBrainz: {}\n", if mb_enabled { "enabled" } else { "disabled" });

    println!("Integrations configured.\n");
    Ok(())
}

fn find_user_data_dir(global_data: &std::path::Path) -> Result<std::path::PathBuf, String> {
    // Read last_user_id
    let marker = global_data.join("last_user_id");
    if marker.exists() {
        if let Ok(uid) = std::fs::read_to_string(&marker) {
            let uid = uid.trim();
            if !uid.is_empty() {
                let user_dir = global_data.join("users").join(uid);
                if user_dir.exists() {
                    return Ok(user_dir);
                }
            }
        }
    }
    Err("No user session found. Run 'qbzd login' first.".to_string())
}
