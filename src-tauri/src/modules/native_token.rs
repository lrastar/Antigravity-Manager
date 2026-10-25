use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

static BACKGROUND_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeUsageRecord {
    pub id: i64,
    pub record_key: String,
    pub session_id: String,
    pub conversation_id: String,
    pub response_id: Option<String>,
    pub model: String,
    pub model_normalized: String,
    pub model_raw: String,
    pub timestamp: i64,
    /// Raw uncached input tokens (Protobuf tag 2)
    pub uncached_input_tokens: u64,
    /// Unified native input tokens = uncached_input_tokens + cached_tokens + cache_write_tokens
    pub input_tokens: u64,
    /// Cache read tokens (Protobuf tag 5)
    pub cached_tokens: u64,
    /// Cache creation / write tokens (Protobuf tag 4)
    pub cache_write_tokens: u64,
    /// Total output tokens, including reasoning_tokens (Protobuf tag 3)
    pub output_tokens: u64,
    /// Thinking / reasoning tokens, subset of output_tokens (Protobuf tag 9)
    pub reasoning_tokens: u64,
    /// Unified total tokens = input_tokens + output_tokens
    pub total_tokens: u64,
    #[serde(default)]
    pub duration_ms: Option<f64>,
    #[serde(default)]
    pub tokens_per_second: Option<f64>,
    pub account_email: String,
    pub attribution_type: String,   // "auto" | "manual" | "unassigned"
    pub attribution_method: String, // Alias of attribution_type for frontend compatibility
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeCollectorStatus {
    pub enabled: bool,
    pub baseline_established_at: Option<i64>,
    pub enabled_since: Option<i64>,
    pub last_scanned_at: Option<i64>,
    pub last_error: Option<String>,
    pub last_error_at: Option<i64>,
    pub total_native_records: u64,
    pub unassigned_records: u64,
    pub unassigned_tokens: u64,
    pub active_observation_account: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeRecordsPage {
    pub records: Vec<NativeUsageRecord>,
    pub total_count: u64,
    pub unassigned_count: u64,
    pub page: i64,
    pub page_size: i64,
    pub total_pages: i64,
}

#[derive(Debug, Clone)]
pub struct ParsedNativeGeneration {
    pub record_key: String,
    pub session_id: String,
    pub gen_idx: i64,
    pub response_id: Option<String>,
    pub model: String,
    pub model_raw: String,
    pub gen_start_time: i64,
    pub timestamp: i64,
    /// Raw uncached input tokens (tag 2)
    pub input_tokens: u64,
    /// Cache read tokens (tag 5)
    pub cached_tokens: u64,
    /// Cache creation/write tokens (tag 4)
    pub cache_write_tokens: u64,
    /// Output tokens including thinking (tag 3)
    pub output_tokens: u64,
    /// Thinking tokens, subset of output (tag 9)
    pub reasoning_tokens: u64,
    pub raw_json: String,
    pub duration_ms: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct DbExtractionOutput {
    pub records: Vec<ParsedNativeGeneration>,
    /// All generation slot keys (`idx:{session_id}:{gen_idx}` and `record_key`) present in `gen_metadata`,
    /// including in-flight generations without usage or timestamps yet.
    pub baseline_keys: Vec<String>,
    pub anomalies: Vec<String>,
}

/// Initialize native token tracking tables and schema migrations in token_stats.db
pub fn init_native_tables(conn: &Connection) -> Result<(), String> {
    // 1. Native usage table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS native_token_usage (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            record_key TEXT UNIQUE NOT NULL,
            session_id TEXT NOT NULL,
            response_id TEXT,
            model TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            cached_tokens INTEGER NOT NULL DEFAULT 0,
            cache_write_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            reasoning_tokens INTEGER NOT NULL DEFAULT 0,
            total_tokens INTEGER NOT NULL DEFAULT 0,
            duration_ms REAL,
            account_email TEXT NOT NULL DEFAULT '',
            attribution_type TEXT NOT NULL DEFAULT 'unassigned',
            user_modified INTEGER NOT NULL DEFAULT 0,
            raw_json TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL
        )",
        [],
    )
    .map_err(|e| format!("Failed to create native_token_usage: {e}"))?;

    // Idempotent column migration for existing databases
    match conn.execute("ALTER TABLE native_token_usage ADD COLUMN duration_ms REAL", []) {
        Ok(_) => {},
        Err(error) if error.to_string().contains("duplicate column name") => {},
        Err(error) => return Err(error.to_string()),
    }
    let _ = conn.execute(
        "ALTER TABLE native_token_usage ADD COLUMN user_modified INTEGER NOT NULL DEFAULT 0",
        [],
    );

    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_native_timestamp ON native_token_usage (timestamp DESC)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_native_account ON native_token_usage (account_email)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_native_model ON native_token_usage (model)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_native_session ON native_token_usage (session_id)",
        [],
    );

    // 2. Collector state table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS native_collector_state (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            enabled INTEGER NOT NULL DEFAULT 0,
            baseline_established_at INTEGER,
            enabled_since INTEGER,
            last_scanned_at INTEGER,
            last_error TEXT,
            last_error_at INTEGER
        )",
        [],
    )
    .map_err(|e| format!("Failed to create native_collector_state: {e}"))?;

    let _ = conn.execute(
        "ALTER TABLE native_collector_state ADD COLUMN enabled_since INTEGER",
        [],
    );

    // Ensure state row 1 exists
    let _ = conn.execute(
        "INSERT OR IGNORE INTO native_collector_state (id, enabled, baseline_established_at, enabled_since, last_scanned_at, last_error, last_error_at)
         VALUES (1, 0, NULL, NULL, NULL, NULL, NULL)",
        [],
    );

    // 3. Baseline records table (keys excluded because they existed before enabling)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS native_baseline_records (
            record_key TEXT PRIMARY KEY,
            discovered_at INTEGER NOT NULL
        )",
        [],
    )
    .map_err(|e| format!("Failed to create native_baseline_records: {e}"))?;

    // 4. Account observation windows table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS account_observation_windows (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            account_email TEXT NOT NULL,
            start_time INTEGER NOT NULL,
            end_time INTEGER,
            last_verified_at INTEGER,
            source TEXT NOT NULL,
            main_pid INTEGER,
            main_start_time INTEGER,
            ls_pid INTEGER,
            ls_start_time INTEGER,
            created_at INTEGER NOT NULL
        )",
        [],
    )
    .map_err(|e| format!("Failed to create account_observation_windows: {e}"))?;

    let _ = conn.execute(
        "ALTER TABLE account_observation_windows ADD COLUMN last_verified_at INTEGER",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE account_observation_windows ADD COLUMN main_pid INTEGER",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE account_observation_windows ADD COLUMN main_start_time INTEGER",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE account_observation_windows ADD COLUMN ls_pid INTEGER",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE account_observation_windows ADD COLUMN ls_start_time INTEGER",
        [],
    );

    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_obs_time ON account_observation_windows (start_time, end_time)",
        [],
    );

    Ok(())
}

/// Close any unclosed observation window on Manager startup so Manager offline periods
/// are never covered by a stale observation window.
pub fn invalidate_unclosed_windows_on_startup_with_conn(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "UPDATE account_observation_windows
         SET end_time = COALESCE(last_verified_at, start_time)
         WHERE end_time IS NULL",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Verify that the strict native classic client credential matches the expected managed account email.
/// Does not use cached/inferred detection or stale fallback files.
pub fn verify_strict_classic_credential(expected_email: &str) -> bool {
    let trimmed_email = expected_email.trim();
    if trimmed_email.is_empty() {
        return false;
    }

    // 1. Find managed account to obtain its canonical refresh_token
    let accounts = match crate::modules::account::list_accounts() {
        Ok(list) => list,
        Err(_) => return false,
    };
    let Some(account) = accounts
        .iter()
        .find(|a| a.email.trim().eq_ignore_ascii_case(trimmed_email))
    else {
        return false;
    };
    let expected_refresh = account.token.refresh_token.trim();
    if expected_refresh.is_empty() {
        return false;
    }

    // 2. If ~/.gemini/google_accounts.json exists and specifies an active email, it must not conflict
    if let Some(home) = dirs::home_dir() {
        let google_accounts_path = home.join(".gemini").join("google_accounts.json");
        if google_accounts_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&google_accounts_path) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(active) = json.get("active").and_then(|v| v.as_str()) {
                        let active_trimmed = active.trim();
                        if !active_trimmed.is_empty()
                            && !active_trimmed.eq_ignore_ascii_case(trimmed_email)
                        {
                            return false;
                        }
                    }
                }
            }
        }
    }

    // 3. Strictly read from system keyring (gemini:antigravity) without file fallback
    if let Ok(stored) = crate::modules::integration::read_from_system_keyring_only() {
        return stored.refresh_token.trim() == expected_refresh;
    }

    // 4. Legacy (< 2.0.0) fallback only if version is explicitly detected as < 2.0.0
    if let Ok(ver) = crate::modules::version::get_antigravity_version_with_path(Some("classic"), None) {
        if crate::modules::version::compare_version(&ver.short_version, "2.0.0")
            == std::cmp::Ordering::Less
        {
            if let Ok(db_path) = crate::modules::db::get_db_path(Some("classic")) {
                if db_path.exists() {
                    let uri = format!("file:{}?mode=ro", db_path.to_string_lossy());
                    if let Ok(conn) = Connection::open_with_flags(
                        &uri,
                        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
                    ) {
                        if let Ok(val) = conn.query_row::<String, _, _>(
                            "SELECT value FROM ItemTable WHERE key = 'antigravityAuthStatus'",
                            [],
                            |r| r.get(0),
                        ) {
                            if val.contains(trimmed_email) {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }

    false
}

/// Continuously verify open observation windows against live process identity and strict credentials.
/// If the tracked process exited/restarted or credentials changed externally, closes the window at
/// `COALESCE(last_verified_at, start_time)`.
pub fn verify_active_observation_window_with_hooks<FProc, FCred>(
    conn: &Connection,
    now: i64,
    get_identity: FProc,
    verify_cred: FCred,
) -> Result<(), String>
where
    FProc: Fn() -> Option<crate::modules::process::ClassicProcessIdentity>,
    FCred: Fn(&str) -> bool,
{
    let mut stmt = conn
        .prepare(
            "SELECT id, account_email, start_time, last_verified_at, main_pid, main_start_time, ls_pid, ls_start_time
             FROM account_observation_windows
             WHERE end_time IS NULL",
        )
        .map_err(|e| e.to_string())?;

    let open_windows: Vec<(
        i64,
        String,
        i64,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
    )> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();

    if open_windows.is_empty() {
        return Ok(());
    }

    let current_identity = get_identity();

    for (id, email, start_time, last_verified_at, main_pid, main_start_time, ls_pid, ls_start_time) in
        open_windows
    {
        let fallback_end = last_verified_at.unwrap_or(start_time);

        // Windows without bound process identity are never valid
        let (Some(expected_main_pid), Some(expected_main_start)) = (main_pid, main_start_time) else {
            let _ = conn.execute(
                "UPDATE account_observation_windows SET end_time = ?1 WHERE id = ?2",
                params![fallback_end, id],
            );
            continue;
        };

        let Some(ref live) = current_identity else {
            // Native client is no longer running (or ambiguous multiple instances)
            let _ = conn.execute(
                "UPDATE account_observation_windows SET end_time = ?1 WHERE id = ?2",
                params![fallback_end, id],
            );
            continue;
        };

        // Main process identity must match
        if live.main_pid as i64 != expected_main_pid
            || live.main_start_time as i64 != expected_main_start
        {
            let _ = conn.execute(
                "UPDATE account_observation_windows SET end_time = ?1 WHERE id = ?2",
                params![fallback_end, id],
            );
            continue;
        }

        // If language_server was already bound, its PID and start_time must also still match
        if let (Some(expected_ls_pid), Some(expected_ls_start)) = (ls_pid, ls_start_time) {
            if live.ls_pid.map(|p| p as i64) != Some(expected_ls_pid)
                || live.ls_start_time.map(|t| t as i64) != Some(expected_ls_start)
            {
                let _ = conn.execute(
                    "UPDATE account_observation_windows SET end_time = ?1 WHERE id = ?2",
                    params![fallback_end, id],
                );
                continue;
            }
        }

        // Strict credential verification must still match the observed account
        if !verify_cred(&email) {
            let _ = conn.execute(
                "UPDATE account_observation_windows SET end_time = ?1 WHERE id = ?2",
                params![fallback_end, id],
            );
            continue;
        }

        // Still valid! Advance last_verified_at to now (and bind ls_pid if newly spawned)
        let new_ls_pid = ls_pid.or_else(|| live.ls_pid.map(|p| p as i64));
        let new_ls_start = ls_start_time.or_else(|| live.ls_start_time.map(|t| t as i64));
        let _ = conn.execute(
            "UPDATE account_observation_windows
             SET last_verified_at = ?1, ls_pid = ?2, ls_start_time = ?3
             WHERE id = ?4",
            params![now, new_ls_pid, new_ls_start, id],
        );
    }

    Ok(())
}

pub fn verify_active_observation_window(conn: &Connection, now: i64) -> Result<(), String> {
    verify_active_observation_window_with_hooks(
        conn,
        now,
        crate::modules::process::get_classic_process_identity,
        verify_strict_classic_credential,
    )
}

/// Resolve account attribution for a generation spanning `[gen_start_time, timestamp]`.
/// Returns `(account_email, attribution_type)`:
/// - `("email", "auto")` if and only if `[gen_start_time, timestamp]` lies strictly within a single
///   verified observation window `[start_time, COALESCE(end_time, last_verified_at, start_time)]`
///   and does not cross or touch any conflicting window boundary.
/// - `("", "unassigned")` otherwise.
pub fn resolve_record_attribution(
    conn: &Connection,
    gen_start_time: i64,
    timestamp: i64,
) -> (String, String) {
    let start_ts = gen_start_time.min(timestamp);
    let end_ts = gen_start_time.max(timestamp);

    let Ok(mut stmt) = conn.prepare(
        "SELECT account_email, start_time, COALESCE(end_time, last_verified_at, start_time) AS eff_end, end_time
         FROM account_observation_windows
         WHERE main_pid IS NOT NULL",
    ) else {
        return (String::new(), "unassigned".to_string());
    };

    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, Option<i64>>(3)?,
        ))
    }) else {
        return (String::new(), "unassigned".to_string());
    };

    let windows: Vec<(String, i64, i64, Option<i64>)> = rows.flatten().collect();

    // 1. If the generation crosses any window's start_time or end_time boundary, it is ambiguous -> unassigned
    for (_, w_start, w_eff_end, w_end) in &windows {
        if start_ts < *w_start && *w_start <= end_ts {
            return (String::new(), "unassigned".to_string());
        }
        let boundary_end = w_end.unwrap_or(*w_eff_end);
        if start_ts <= boundary_end && boundary_end < end_ts {
            return (String::new(), "unassigned".to_string());
        }
    }

    // 2. Find all windows that completely contain [start_ts, end_ts]
    let mut matching_emails: Vec<String> = Vec::new();
    for (email, w_start, w_eff_end, _) in &windows {
        if *w_start <= start_ts && end_ts <= *w_eff_end && !email.trim().is_empty() {
            matching_emails.push(email.trim().to_string());
        }
    }

    matching_emails.dedup();
    if matching_emails.len() == 1 {
        (matching_emails.remove(0), "auto".to_string())
    } else {
        (String::new(), "unassigned".to_string())
    }
}

/// Get native collector status
pub fn get_status() -> Result<NativeCollectorStatus, String> {
    let conn = crate::modules::token_stats::connect_db()?;
    let now = chrono::Local::now().timestamp();
    let _ = verify_active_observation_window(&conn, now);
    get_status_with_conn(&conn)
}

pub fn get_status_with_conn(conn: &Connection) -> Result<NativeCollectorStatus, String> {
    let (
        enabled,
        baseline_established_at,
        enabled_since,
        last_scanned_at,
        last_error,
        last_error_at,
    ): (
        i64,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<String>,
        Option<i64>,
    ) = conn
        .query_row(
            "SELECT enabled, baseline_established_at, enabled_since, last_scanned_at, last_error, last_error_at
             FROM native_collector_state WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap_or((0, None, None, None, None, None));

    let total_native_records: u64 = conn
        .query_row("SELECT COUNT(*) FROM native_token_usage", [], |row| row.get(0))
        .unwrap_or(0);

    let (unassigned_records, unassigned_tokens): (u64, u64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(total_tokens), 0)
             FROM native_token_usage
             WHERE account_email = '' OR attribution_type = 'unassigned'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap_or((0, 0));

    let active_observation_account: Option<String> = conn
        .query_row(
            "SELECT account_email FROM account_observation_windows
             WHERE end_time IS NULL AND main_pid IS NOT NULL
             ORDER BY start_time DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();

    Ok(NativeCollectorStatus {
        enabled: enabled == 1,
        baseline_established_at: baseline_established_at.or(enabled_since),
        enabled_since: enabled_since.or(baseline_established_at),
        last_scanned_at,
        last_error,
        last_error_at,
        total_native_records,
        unassigned_records,
        unassigned_tokens,
        active_observation_account,
    })
}

/// Enable or disable native collector
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let conn = crate::modules::token_stats::connect_db()?;
    let now = chrono::Local::now().timestamp();
    set_enabled_with_conn(&conn, enabled, now, get_conversations_dir().as_deref())
}

pub fn set_enabled_with_conn(
    conn: &Connection,
    enabled: bool,
    now: i64,
    conv_dir: Option<&Path>,
) -> Result<(), String> {
    if enabled {
        let baseline_established: Option<i64> = conn
            .query_row(
                "SELECT baseline_established_at FROM native_collector_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        if let Some(dir) = conv_dir {
            establish_baseline_in_dir(conn, dir, now)?;
        }

        if baseline_established.is_none() {
            tracing::info!("[NativeToken] Establishing initial baseline and time boundary at {}...", now);
            conn.execute(
                "UPDATE native_collector_state
                 SET enabled = 1, baseline_established_at = ?1, enabled_since = ?1
                 WHERE id = 1",
                params![now],
            )
            .map_err(|e| e.to_string())?;
        } else {
            tracing::info!(
                "[NativeToken] Re-enabling collector; establishing new enabled_since boundary at {}...",
                now
            );
            conn.execute(
                "UPDATE native_collector_state
                 SET enabled = 1, enabled_since = ?1
                 WHERE id = 1",
                params![now],
            )
            .map_err(|e| e.to_string())?;
        }
        // Note: Never initialize an observation window from cached/inferred status here.
        // Observation windows are strictly opened only upon verified classic account switches.
    } else {
        close_active_observation_window_with_conn(conn, now)?;
        conn.execute(
            "UPDATE native_collector_state SET enabled = 0 WHERE id = 1",
            [],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Close active observation window (called when switching accounts, CLI credential change, or shutting down)
pub fn close_active_observation_window() -> Result<(), String> {
    let conn = crate::modules::token_stats::connect_db()?;
    let now = chrono::Local::now().timestamp();
    close_active_observation_window_with_conn(&conn, now)
}

pub fn close_active_observation_window_with_conn(conn: &Connection, now: i64) -> Result<(), String> {
    conn.execute(
        "UPDATE account_observation_windows
         SET end_time = MIN(?1, COALESCE(last_verified_at, start_time)) WHERE end_time IS NULL",
        params![now],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Open an observation window ONLY after a classic client switch completes AND both the respawned
/// native process identity and strict system credential match the target account.
pub fn open_observation_window(account_email: &str, source: &str) -> Result<(), String> {
    if account_email.trim().is_empty() {
        return Ok(());
    }
    let conn = crate::modules::token_stats::connect_db()?;
    let now = chrono::Local::now().timestamp();

    // Always close any previous window first
    let _ = close_active_observation_window_with_conn(&conn, now);

    // 1. Wait for and verify unambiguous classic process identity
    let Some(identity) = crate::modules::process::wait_for_classic_process_identity(5) else {
        tracing::warn!(
            "[NativeToken] Skipping observation window for {}: classic process identity could not be verified",
            account_email
        );
        return Ok(());
    };

    // 2. Verify strict credential readback matches target account
    if !verify_strict_classic_credential(account_email) {
        tracing::warn!(
            "[NativeToken] Skipping observation window for {}: strict credential verification did not match",
            account_email
        );
        return Ok(());
    }

    let open_now = chrono::Local::now().timestamp().max(now);
    open_verified_observation_window_with_conn(
        &conn,
        account_email.trim(),
        open_now,
        source,
        &identity,
    )
}

pub fn open_verified_observation_window_with_conn(
    conn: &Connection,
    account_email: &str,
    now: i64,
    source: &str,
    identity: &crate::modules::process::ClassicProcessIdentity,
) -> Result<(), String> {
    let _ = close_active_observation_window_with_conn(conn, now);

    conn.execute(
        "INSERT INTO account_observation_windows (
            account_email, start_time, end_time, last_verified_at, source,
            main_pid, main_start_time, ls_pid, ls_start_time, created_at
         ) VALUES (?1, ?2, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?2)",
        params![
            account_email.trim(),
            now,
            source,
            identity.main_pid as i64,
            identity.main_start_time as i64,
            identity.ls_pid.map(|p| p as i64),
            identity.ls_start_time.map(|t| t as i64),
        ],
    )
    .map_err(|e| e.to_string())?;

    tracing::info!(
        "[NativeToken] Opened verified observation window for {} at {} (main_pid={}, ls_pid={:?})",
        account_email,
        now,
        identity.main_pid,
        identity.ls_pid
    );
    Ok(())
}

/// Returns the conversations directory: %USERPROFILE%\.gemini\antigravity\conversations
pub fn get_conversations_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".gemini").join("antigravity").join("conversations"))
}

pub fn establish_baseline() -> Result<(), String> {
    let conn = crate::modules::token_stats::connect_db()?;
    let now = chrono::Local::now().timestamp();
    establish_baseline_with_conn(&conn, now)
}

pub fn establish_baseline_with_conn(conn: &Connection, now: i64) -> Result<(), String> {
    let _ = conn.execute(
        "UPDATE native_collector_state
         SET baseline_established_at = COALESCE(baseline_established_at, ?1),
             enabled_since = ?1
         WHERE id = 1",
        params![now],
    );
    if let Some(conv_dir) = get_conversations_dir() {
        establish_baseline_in_dir(conn, &conv_dir, now)?;
    }
    Ok(())
}

/// Snapshot all current conversation records (including in-flight generations) into `native_baseline_records`
pub fn establish_baseline_in_dir(conn: &Connection, conv_dir: &Path, now: i64) -> Result<(), String> {
    if !conv_dir.exists() {
        return Ok(());
    }

    let entries = match std::fs::read_dir(conv_dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    let mut stmt = conn
        .prepare(
            "INSERT OR IGNORE INTO native_baseline_records (record_key, discovered_at)
             VALUES (?1, ?2)",
        )
        .map_err(|e| e.to_string())?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("db") {
            if let Ok(output) = inspect_conversation_db(&path) {
                for key in output.baseline_keys {
                    let already_recorded: bool = conn
                        .query_row(
                            "SELECT COUNT(*) FROM native_token_usage WHERE record_key = ?1",
                            params![key],
                            |row| row.get::<_, i64>(0),
                        )
                        .map(|c| c > 0)
                        .unwrap_or(false);

                    if !already_recorded {
                        let _ = stmt.execute(params![key, now]);
                    }
                }
            }
        }
    }

    Ok(())
}

/// Incremental scan of native conversations
pub fn scan_native_conversations() -> Result<NativeCollectorStatus, String> {
    let conn = crate::modules::token_stats::connect_db()?;
    let now = chrono::Local::now().timestamp();

    // First verify active observation window against live process & strict credentials
    let _ = verify_active_observation_window(&conn, now);

    let Some(conv_dir) = get_conversations_dir() else {
        return get_status_with_conn(&conn);
    };

    scan_conversations_in_dir(&conn, &conv_dir, now)
}

pub fn scan_conversations_in_dir(
    conn: &Connection,
    conv_dir: &Path,
    now: i64,
) -> Result<NativeCollectorStatus, String> {
    // Verify collector is enabled and obtain time boundaries
    let (enabled, baseline_established_at, enabled_since): (bool, Option<i64>, Option<i64>) = conn
        .query_row(
            "SELECT enabled, baseline_established_at, enabled_since FROM native_collector_state WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? == 1,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            },
        )
        .unwrap_or((false, None, None));

    if !enabled {
        return get_status_with_conn(conn);
    }

    let cutoff_time = enabled_since.or(baseline_established_at).unwrap_or(now);

    if !conv_dir.exists() {
        let _ = conn.execute(
            "UPDATE native_collector_state SET last_scanned_at = ?1 WHERE id = 1",
            params![now],
        );
        return get_status_with_conn(conn);
    }

    let entries = match std::fs::read_dir(conv_dir) {
        Ok(e) => e,
        Err(e) => {
            let err_msg = format!("Failed to read conversations dir: {e}");
            let _ = conn.execute(
                "UPDATE native_collector_state SET last_error = ?1, last_error_at = ?2 WHERE id = 1",
                params![err_msg, now],
            );
            return Err(err_msg);
        }
    };

    let mut anomalies: Vec<String> = Vec::new();
    let mut new_records_count = 0;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("db") {
            match inspect_conversation_db(&path) {
                Ok(output) => {
                    for anomaly in output.anomalies {
                        anomalies.push(anomaly);
                    }

                    for r in output.records {
                        let slot_key = format!("idx:{}:{}", r.session_id, r.gen_idx);

                        // Check if this record is already stored in native_token_usage
                        let already_captured: bool = conn
                            .query_row(
                                "SELECT COUNT(*) FROM native_token_usage WHERE record_key = ?1",
                                params![r.record_key],
                                |row| row.get::<_, i64>(0),
                            )
                            .map(|c| c > 0)
                            .unwrap_or(false);

                        if !already_captured {
                            // 1. Reject if either record_key or slot_key (idx:session:gen_idx) is in baseline
                            let in_baseline: bool = conn
                                .query_row(
                                    "SELECT COUNT(*) FROM native_baseline_records WHERE record_key = ?1 OR record_key = ?2",
                                    params![r.record_key, slot_key],
                                    |row| row.get::<_, i64>(0),
                                )
                                .map(|c| c > 0)
                                .unwrap_or(false);

                            if in_baseline {
                                continue;
                            }

                            // 2. Reject if generation started or completed before cutoff_time (enabled_since / baseline_established_at).
                            //    Also add to baseline so late-recovered historical DBs stay permanently excluded.
                            if r.timestamp < cutoff_time || r.gen_start_time < cutoff_time {
                                let _ = conn.execute(
                                    "INSERT OR IGNORE INTO native_baseline_records (record_key, discovered_at) VALUES (?1, ?2)",
                                    params![r.record_key, now],
                                );
                                let _ = conn.execute(
                                    "INSERT OR IGNORE INTO native_baseline_records (record_key, discovered_at) VALUES (?1, ?2)",
                                    params![slot_key, now],
                                );
                                continue;
                            }
                        }

                        // 3. Determine strict account attribution based on [gen_start_time, timestamp]
                        let (account_email, attribution_type) =
                            resolve_record_attribution(conn, r.gen_start_time, r.timestamp);

                        let total_tokens =
                            (r.input_tokens + r.cached_tokens + r.cache_write_tokens)
                                + r.output_tokens;

                        // 4. Upsert into native_token_usage (streaming updates overwrite token counts,
                        //    while preserving manual or unassigned state against auto-reattribution).
                        let res = conn.execute(
                            "INSERT INTO native_token_usage (
                                record_key, session_id, response_id, model, timestamp,
                                input_tokens, cached_tokens, cache_write_tokens, output_tokens, reasoning_tokens,
                                total_tokens, account_email, attribution_type, user_modified, raw_json, created_at, duration_ms
                             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 0, ?14, ?15, ?16)
                             ON CONFLICT(record_key) DO UPDATE SET
                                model = excluded.model,
                                timestamp = excluded.timestamp,
                                input_tokens = excluded.input_tokens,
                                cached_tokens = excluded.cached_tokens,
                                cache_write_tokens = excluded.cache_write_tokens,
                                output_tokens = excluded.output_tokens,
                                reasoning_tokens = excluded.reasoning_tokens,
                                total_tokens = excluded.total_tokens,
                                duration_ms = excluded.duration_ms,
                                raw_json = excluded.raw_json,
                                account_email = CASE
                                    WHEN native_token_usage.user_modified = 1
                                      OR native_token_usage.attribution_type IN ('manual', 'unassigned')
                                    THEN native_token_usage.account_email
                                    ELSE excluded.account_email
                                END,
                                attribution_type = CASE
                                    WHEN native_token_usage.user_modified = 1
                                      OR native_token_usage.attribution_type IN ('manual', 'unassigned')
                                    THEN native_token_usage.attribution_type
                                    ELSE excluded.attribution_type
                                END",
                            params![
                                r.record_key,
                                r.session_id,
                                r.response_id,
                                r.model,
                                r.timestamp,
                                r.input_tokens,
                                r.cached_tokens,
                                r.cache_write_tokens,
                                r.output_tokens,
                                r.reasoning_tokens,
                                total_tokens,
                                account_email,
                                attribution_type,
                                r.raw_json,
                                now,
                                r.duration_ms
                            ],
                        );

                        if let Ok(inserted) = res {
                            if inserted > 0 {
                                new_records_count += 1;
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("[NativeToken] Failed to parse db {}: {}", path.display(), e);
                    anomalies.push(e);
                }
            }
        }
    }

    let scan_error = if anomalies.is_empty() {
        None
    } else {
        Some(anomalies.join("; "))
    };

    let _ = conn.execute(
        "UPDATE native_collector_state SET last_scanned_at = ?1, last_error = ?2, last_error_at = ?3 WHERE id = 1",
        params![
            now,
            scan_error,
            if scan_error.is_some() { Some(now) } else { None }
        ],
    );

    if new_records_count > 0 {
        tracing::info!(
            "[NativeToken] Scan completed: captured/updated {} generation records",
            new_records_count
        );
    }

    get_status_with_conn(conn)
}

/// Start background scanner thread (runs every 30s)
pub fn start_background_scanner() {
    if BACKGROUND_RUNNING.swap(true, Ordering::SeqCst) {
        return; // Already running
    }

    // Invalidate any observation window left open from a previous Manager run
    if let Ok(conn) = crate::modules::token_stats::connect_db() {
        let _ = invalidate_unclosed_windows_on_startup_with_conn(&conn);
    }

    spawn_on_app_runtime(async move {
        tracing::info!("[NativeToken] Background scanner task started (30s interval)");
        // Perform an immediate initial scan on startup (e.g. for offline backfill)
        tokio::task::spawn_blocking(|| {
            let _ = scan_native_conversations();
        })
        .await
        .ok();

        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            tokio::task::spawn_blocking(|| {
                let _ = scan_native_conversations();
            })
            .await
            .ok();
        }
    });
}

fn spawn_on_app_runtime<F>(task: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    tauri::async_runtime::spawn(task);
}

/// Manually assign a native record to an account
pub fn assign_record_account(record_key: &str, account_email: &str) -> Result<(), String> {
    let conn = crate::modules::token_stats::connect_db()?;
    assign_record_account_with_conn(&conn, record_key, account_email)
}

pub fn assign_record_account_with_conn(
    conn: &Connection,
    record_key: &str,
    account_email: &str,
) -> Result<(), String> {
    conn.execute(
        "UPDATE native_token_usage
         SET account_email = ?1, attribution_type = 'manual', user_modified = 1
         WHERE record_key = ?2",
        params![account_email.trim(), record_key],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Unassign a native record (set back to unassigned and protect from auto-reattribution)
pub fn unassign_record_account(record_key: &str) -> Result<(), String> {
    let conn = crate::modules::token_stats::connect_db()?;
    unassign_record_account_with_conn(&conn, record_key)
}

pub fn unassign_record_account_with_conn(conn: &Connection, record_key: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE native_token_usage
         SET account_email = '', attribution_type = 'unassigned', user_modified = 1
         WHERE record_key = ?1",
        params![record_key],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Batch assign records to an account
pub fn batch_assign_record_account(
    record_keys: &[String],
    account_email: &str,
) -> Result<(), String> {
    let mut conn = crate::modules::token_stats::connect_db()?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    {
        let mut stmt = tx
            .prepare(
                "UPDATE native_token_usage
                 SET account_email = ?1, attribution_type = 'manual', user_modified = 1
                 WHERE record_key = ?2",
            )
            .map_err(|e| e.to_string())?;
        for key in record_keys {
            stmt.execute(params![account_email.trim(), key])
                .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

/// Batch unassign records
pub fn batch_unassign_record_account(record_keys: &[String]) -> Result<(), String> {
    let mut conn = crate::modules::token_stats::connect_db()?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    {
        let mut stmt = tx
            .prepare(
                "UPDATE native_token_usage
                 SET account_email = '', attribution_type = 'unassigned', user_modified = 1
                 WHERE record_key = ?1",
            )
            .map_err(|e| e.to_string())?;
        for key in record_keys {
            stmt.execute(params![key]).map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

/// List native records for management
pub fn list_native_records(
    limit: i64,
    offset: i64,
    search: Option<String>,
) -> Result<NativeRecordsPage, String> {
    let conn = crate::modules::token_stats::connect_db()?;
    list_native_records_with_conn(&conn, limit, offset, search)
}

pub fn list_native_records_with_conn(
    conn: &Connection,
    limit: i64,
    offset: i64,
    search: Option<String>,
) -> Result<NativeRecordsPage, String> {
    let limit = limit.clamp(1, 100);
    let offset = offset.max(0);

    let (where_clause, params_vec): (String, Vec<Box<dyn rusqlite::ToSql>>) = match search {
        Some(ref s) if !s.trim().is_empty() => {
            let trimmed = s.trim();
            if trimmed.eq_ignore_ascii_case("unassigned") {
                (
                    "WHERE (account_email = '' OR attribution_type = 'unassigned')".to_string(),
                    vec![],
                )
            } else {
                let pattern = format!("%{}%", trimmed);
                (
                    "WHERE model LIKE ?1 OR account_email LIKE ?1 OR record_key LIKE ?1 OR session_id LIKE ?1".to_string(),
                    vec![Box::new(pattern)],
                )
            }
        }
        _ => (String::new(), vec![]),
    };

    let count_sql = format!("SELECT COUNT(*) FROM native_token_usage {}", where_clause);
    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let total_count: u64 = conn
        .query_row(&count_sql, params_refs.as_slice(), |row| row.get(0))
        .unwrap_or(0);

    let unassigned_count: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM native_token_usage WHERE account_email = '' OR attribution_type = 'unassigned'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let query_sql = format!(
        "SELECT id, record_key, session_id, response_id, model, timestamp,
                input_tokens, cached_tokens, cache_write_tokens, output_tokens, reasoning_tokens,
                total_tokens, account_email, attribution_type, raw_json, created_at, duration_ms
         FROM native_token_usage
         {}
         ORDER BY timestamp DESC, id DESC
         LIMIT ?{} OFFSET ?{}",
        where_clause,
        params_vec.len() + 1,
        params_vec.len() + 2
    );

    let mut query_params: Vec<Box<dyn rusqlite::ToSql>> = params_vec;
    query_params.push(Box::new(limit));
    query_params.push(Box::new(offset));
    let query_refs: Vec<&dyn rusqlite::ToSql> = query_params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&query_sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(query_refs.as_slice(), |row| {
            let id: i64 = row.get(0)?;
            let record_key: String = row.get(1)?;
            let session_id: String = row.get(2)?;
            let response_id: Option<String> = row.get(3)?;
            let model: String = row.get(4)?;
            let timestamp: i64 = row.get(5)?;
            let uncached_input_tokens: u64 = row.get(6)?;
            let cached_tokens: u64 = row.get(7)?;
            let cache_write_tokens: u64 = row.get(8)?;
            let output_tokens: u64 = row.get(9)?;
            let reasoning_tokens: u64 = row.get(10)?;
            let total_tokens: u64 = row.get(11)?;
            let account_email: String = row.get(12)?;
            let attribution_type: String = row.get(13)?;
            let raw_json: String = row.get(14)?;
            let created_at: i64 = row.get(15)?;
            let duration_ms: Option<f64> = row.get(16)?;

            let model_raw = serde_json::from_str::<serde_json::Value>(&raw_json)
                .ok()
                .and_then(|v| v.get("model").and_then(|m| m.as_str()).map(|s| s.to_string()))
                .unwrap_or_else(|| model.clone());

            let unified_input_tokens =
                uncached_input_tokens + cached_tokens + cache_write_tokens;

            Ok(NativeUsageRecord {
                id,
                record_key,
                conversation_id: session_id.clone(),
                session_id,
                response_id,
                model_normalized: model.clone(),
                model,
                model_raw,
                timestamp,
                uncached_input_tokens,
                input_tokens: unified_input_tokens,
                cached_tokens,
                cache_write_tokens,
                output_tokens,
                reasoning_tokens,
                total_tokens,
                duration_ms,
                tokens_per_second: crate::modules::token_throughput::tokens_per_second(output_tokens, duration_ms),
                account_email,
                attribution_method: attribution_type.clone(),
                attribution_type,
                created_at,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut records = Vec::new();
    for r in rows {
        records.push(r.map_err(|e| e.to_string())?);
    }

    let page = (offset / limit) + 1;
    let total_pages = if total_count == 0 {
        1
    } else {
        ((total_count as i64) + limit - 1) / limit
    };

    Ok(NativeRecordsPage {
        records,
        total_count,
        unassigned_count,
        page,
        page_size: limit,
        total_pages,
    })
}

// =========================================================================
// Protobuf Wire Format Decoder & DB Extraction
// =========================================================================

#[derive(Debug, Clone)]
enum ProtoValue<'a> {
    Varint(u64),
    Fixed64(&'a [u8]),
    Bytes(&'a [u8]),
    Fixed32(&'a [u8]),
}

fn parse_proto_fields<'a>(data: &'a [u8]) -> Vec<(u32, ProtoValue<'a>)> {
    let mut fields = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        let mut shift = 0;
        let mut tag: u64 = 0;
        let mut ok = false;
        while pos < data.len() {
            let b = data[pos];
            pos += 1;
            tag |= ((b & 0x7f) as u64) << shift;
            if (b & 0x80) == 0 {
                ok = true;
                break;
            }
            shift += 7;
            if shift >= 64 {
                break;
            }
        }
        if !ok {
            break;
        }

        let field_num = (tag >> 3) as u32;
        let wire_type = (tag & 0x07) as u8;
        if field_num == 0 || wire_type > 5 {
            break;
        }

        match wire_type {
            0 => {
                // Varint
                let mut val: u64 = 0;
                let mut v_shift = 0;
                let mut v_ok = false;
                while pos < data.len() {
                    let b = data[pos];
                    pos += 1;
                    val |= ((b & 0x7f) as u64) << v_shift;
                    if (b & 0x80) == 0 {
                        v_ok = true;
                        break;
                    }
                    v_shift += 7;
                    if v_shift >= 64 {
                        break;
                    }
                }
                if v_ok {
                    fields.push((field_num, ProtoValue::Varint(val)));
                } else {
                    break;
                }
            }
            1 => {
                // 64-bit
                if pos + 8 <= data.len() {
                    fields.push((field_num, ProtoValue::Fixed64(&data[pos..pos + 8])));
                    pos += 8;
                } else {
                    break;
                }
            }
            2 => {
                // Length-delimited
                let mut len: u64 = 0;
                let mut l_shift = 0;
                let mut l_ok = false;
                while pos < data.len() {
                    let b = data[pos];
                    pos += 1;
                    len |= ((b & 0x7f) as u64) << l_shift;
                    if (b & 0x80) == 0 {
                        l_ok = true;
                        break;
                    }
                    l_shift += 7;
                    if l_shift >= 64 {
                        break;
                    }
                }
                if l_ok {
                    let length = len as usize;
                    if pos + length <= data.len() {
                        fields.push((field_num, ProtoValue::Bytes(&data[pos..pos + length])));
                        pos += length;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            5 => {
                // 32-bit
                if pos + 4 <= data.len() {
                    fields.push((field_num, ProtoValue::Fixed32(&data[pos..pos + 4])));
                    pos += 4;
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
    fields
}

fn get_first_varint(fields: &[(u32, ProtoValue)], tag: u32) -> Option<u64> {
    for (f_num, val) in fields {
        if *f_num == tag {
            if let ProtoValue::Varint(v) = val {
                return Some(*v);
            }
        }
    }
    None
}

fn get_first_bytes<'a>(fields: &'a [(u32, ProtoValue<'a>)], tag: u32) -> Option<&'a [u8]> {
    for (f_num, val) in fields {
        if *f_num == tag {
            if let ProtoValue::Bytes(b) = val {
                return Some(*b);
            }
        }
    }
    None
}

fn get_first_string(fields: &[(u32, ProtoValue)], tag: u32) -> Option<String> {
    get_first_bytes(fields, tag).and_then(|b| String::from_utf8(b.to_vec()).ok())
}

/// chat[11] + chat[12] are protobuf Durations (seconds + nanos), including TTFT.
/// Keep missing, incomplete or invalid timing unavailable instead of using file/scan time.
fn generation_duration_ms(chat: &[(u32, ProtoValue<'_>)]) -> Option<f64> {
    let duration = |tag| {
        let bytes = get_first_bytes(chat, tag)?;
        let fields = parse_proto_fields(bytes);
        if fields.iter().any(|(field, value)| matches!(field, 1 | 2) && !matches!(value, ProtoValue::Varint(_))) {
            return None;
        }
        if !bytes.is_empty() && fields.is_empty() {
            return None;
        }
        let seconds = get_first_varint(&fields, 1).unwrap_or(0);
        let nanos = get_first_varint(&fields, 2).unwrap_or(0);
        // Protobuf Duration's valid seconds range; negative signed varints are also rejected.
        if seconds > 315_576_000_000 || nanos >= 1_000_000_000 {
            return None;
        }
        Some(seconds as f64 * 1000.0 + nanos as f64 / 1_000_000.0)
    };
    let total = duration(11)? + duration(12)?;
    (total.is_finite() && total > 0.0).then_some(total)
}

fn get_generation_step_indices(fields: &[(u32, ProtoValue<'_>)]) -> Result<Vec<i64>, String> {
    let mut indices = Vec::new();
    for (field, value) in fields {
        if *field != 2 {
            continue;
        }
        match value {
            ProtoValue::Varint(index) => {
                let index = i64::try_from(*index).map_err(|_| "step index overflows i64")?;
                indices.push(index);
            }
            ProtoValue::Bytes(packed) => {
                let mut pos = 0;
                while pos < packed.len() {
                    let mut value = 0u64;
                    let mut shift = 0;
                    let mut complete = false;
                    while pos < packed.len() && shift < 64 {
                        let byte = packed[pos];
                        pos += 1;
                        if shift == 63 && byte & 0x7e != 0 {
                            return Err("packed step index overflows u64".to_string());
                        }
                        value |= ((byte & 0x7f) as u64) << shift;
                        if byte & 0x80 == 0 {
                            complete = true;
                            break;
                        }
                        shift += 7;
                    }
                    if !complete {
                        return Err("malformed packed step indices".to_string());
                    }
                    indices.push(i64::try_from(value).map_err(|_| "step index overflows i64")?);
                }
            }
            _ => return Err("invalid wire type for step indices".to_string()),
        }
    }
    Ok(indices)
}

/// Normalize model names (e.g. gemini-3.8-flash-n -> gemini-3.8-flash)
pub fn normalize_native_model(raw: &str) -> String {
    let lower = raw.trim().to_lowercase();
    if lower == "gemini-3.8-flash-n" {
        return "gemini-3.8-flash".to_string();
    }
    if lower == "gemini-4-argon-trial" {
        return "gemini-4-argon".to_string();
    }
    if lower == "claude-opus-5-5-medium" {
        return "claude-opus-5-5".to_string();
    }
    raw.trim().to_string()
}

/// Extract all valid generation records from a conversation SQLite database
pub fn extract_native_generations_from_db(
    db_path: &Path,
) -> Result<Vec<ParsedNativeGeneration>, String> {
    let output = inspect_conversation_db(db_path)?;
    if output.records.is_empty() && !output.anomalies.is_empty() {
        return Err(output.anomalies.join("; "));
    }
    Ok(output.records)
}

/// Inspect a conversation SQLite database (strictly read-only) and return parsed generations,
/// all baseline slot keys (including in-flight generations), and any data/timestamp anomalies.
pub fn inspect_conversation_db(db_path: &Path) -> Result<DbExtractionOutput, String> {
    let session_id = db_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    let uri = format!("file:{}?mode=ro", db_path.to_string_lossy());
    let conn = Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| format!("Failed to open DB read-only {}: {e}", db_path.display()))?;

    conn.pragma_update(None, "busy_timeout", 3000)
        .map_err(|e| e.to_string())?;

    let mut output = DbExtractionOutput::default();

    // Check if gen_metadata table exists
    let has_gen_metadata: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='gen_metadata'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !has_gen_metadata {
        output
            .anomalies
            .push(format!("Missing gen_metadata table in {}", session_id));
        return Ok(output);
    }

    // Step timestamps and response IDs let each generation resolve only its own steps.
    let mut step_metadata: HashMap<i64, (Option<i64>, Option<String>)> = HashMap::new();
    let mut response_steps: HashMap<String, Vec<i64>> = HashMap::new();
    let has_steps: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='steps'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if has_steps {
        if let Ok(mut step_stmt) =
            conn.prepare("SELECT idx, metadata FROM steps WHERE metadata IS NOT NULL")
        {
            let step_rows = step_stmt.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            });
            if let Ok(rows) = step_rows {
                for (s_idx, s_meta) in rows.flatten() {
                    let fields = parse_proto_fields(&s_meta);
                    let timestamp = get_first_bytes(&fields, 1)
                        .and_then(|bytes| get_first_varint(&parse_proto_fields(bytes), 1))
                        .filter(|sec| *sec > 1_000_000_000 && *sec < 5_000_000_000)
                        .map(|sec| sec as i64);
                    let response_id = get_first_bytes(&fields, 9)
                        .and_then(|bytes| get_first_string(&parse_proto_fields(bytes), 11))
                        .filter(|id| !id.trim().is_empty());
                    if let Some(id) = &response_id {
                        response_steps.entry(id.clone()).or_default().push(s_idx);
                    }
                    step_metadata.insert(s_idx, (timestamp, response_id));
                }
            }
        }
    }

    // Read gen_metadata rows
    let mut stmt = conn
        .prepare("SELECT idx, data FROM gen_metadata ORDER BY idx ASC")
        .map_err(|e| format!("Failed to prepare gen_metadata: {e}"))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(|e| format!("Query gen_metadata failed: {e}"))?;

    // Deduplicate within the same DB by record_key while preserving order (latest row wins)
    let mut dedup_index: HashMap<String, usize> = HashMap::new();

    for row_res in rows {
        let (gen_idx, data) = match row_res {
            Ok(pair) => pair,
            Err(e) => {
                output.anomalies.push(format!(
                    "Corrupted gen_metadata row in {}: {e}",
                    session_id
                ));
                continue;
            }
        };

        // Always record the slot key so in-flight generations at baseline time are excluded
        let slot_key = format!("idx:{}:{}", session_id, gen_idx);
        output.baseline_keys.push(slot_key);

        let top_fields = parse_proto_fields(&data);
        let Some(f1_bytes) = get_first_bytes(&top_fields, 1) else {
            output.anomalies.push(format!(
                "Unrecognized gen_metadata protobuf structure in {} idx {}",
                session_id, gen_idx
            ));
            continue;
        };

        let f1 = parse_proto_fields(f1_bytes);
        let duration_ms = generation_duration_ms(&f1);

        // 1. Model name
        let raw_model = get_first_string(&f1, 19).unwrap_or_else(|| "unknown".to_string());
        let model = normalize_native_model(&raw_model);

        // 2. Metadata key-values (tag 20)
        let mut kvs: HashMap<String, String> = HashMap::new();
        for (f_num, val) in &f1 {
            if *f_num == 20 {
                if let ProtoValue::Bytes(kv_bytes) = val {
                    let kv_fields = parse_proto_fields(kv_bytes);
                    if let (Some(k), Some(v)) = (
                        get_first_string(&kv_fields, 1),
                        get_first_string(&kv_fields, 2),
                    ) {
                        kvs.insert(k, v);
                    }
                }
            }
        }

        let cascade_id = kvs
            .get("cascade_id")
            .cloned()
            .unwrap_or_else(|| session_id.clone());
        let request_id = kvs.get("request_id").cloned().unwrap_or_default();

        // 3. Usage tokens in field 4 or field 17 -> subfield 2
        // Verified Protobuf field layout:
        //   Tag 2 = uncached_input_tokens
        //   Tag 3 = output_tokens (total output, includes thinking/reasoning tokens)
        //   Tag 4 = cache_write_tokens (cache creation input tokens)
        //   Tag 5 = cached_tokens (cache read input tokens)
        //   Tag 6 = API provider enum (NOT tokens!)
        //   Tag 9 = reasoning_tokens (thinking tokens, subset of output_tokens)
        //   Tag 10 = visible completion tokens (subset of output_tokens)
        //   Tag 11 = response_id
        let has_usage_fields = |u: &[(u32, ProtoValue)]| {
            get_first_varint(u, 2).is_some()
                || get_first_varint(u, 3).is_some()
                || get_first_varint(u, 4).is_some()
                || get_first_varint(u, 5).is_some()
        };

        let extract_usage = |u: &[(u32, ProtoValue)]| {
            (
                get_first_varint(u, 2).unwrap_or(0), // uncached_input_tokens
                get_first_varint(u, 3).unwrap_or(0), // output_tokens
                get_first_varint(u, 5).unwrap_or(0), // cached_tokens (cache read)
                get_first_varint(u, 4).unwrap_or(0), // cache_write_tokens (cache creation)
                get_first_varint(u, 9).unwrap_or(0), // reasoning_tokens (thinking)
                get_first_string(u, 11),             // response_id
            )
        };

        let mut usage_info: Option<(u64, u64, u64, u64, u64, Option<String>)> = None;
        if let Some(f4_bytes) = get_first_bytes(&f1, 4) {
            let u = parse_proto_fields(f4_bytes);
            if has_usage_fields(&u) {
                usage_info = Some(extract_usage(&u));
            }
        }

        if usage_info.is_none() {
            if let Some(f17_bytes) = get_first_bytes(&f1, 17) {
                let f17 = parse_proto_fields(f17_bytes);
                if let Some(f17_sub_bytes) = get_first_bytes(&f17, 2) {
                    let u = parse_proto_fields(f17_sub_bytes);
                    if has_usage_fields(&u) {
                        usage_info = Some(extract_usage(&u));
                    }
                }
            }
        }

        let fallback_key = format!("{}:{}:{}", cascade_id, request_id, gen_idx);
        output.baseline_keys.push(fallback_key.clone());

        let Some((
            input_tokens,
            output_tokens,
            cached_tokens,
            cache_write_tokens,
            reasoning_tokens,
            response_id,
        )) = usage_info
        else {
            // In-flight or cancelled generation without usage metadata
            continue;
        };

        let record_key = match response_id.as_deref() {
            Some(rid) if !rid.trim().is_empty() => rid.trim().to_string(),
            _ => fallback_key,
        };
        output.baseline_keys.push(record_key.clone());

        // gen_metadata field 2 explicitly lists the step indices belonging to this generation.
        // If that mapping is absent, responseID may identify exactly one step. Never infer by
        // generation index, last_step_index, file time, or neighboring steps.
        let step_indices_result = get_generation_step_indices(&top_fields);
        let explicit_step_indices = match &step_indices_result {
            Ok(indices) if !indices.is_empty() => Some(indices.clone()),
            _ => None,
        };
        let response_matches = response_id.as_deref().and_then(|id| response_steps.get(id));
        let step_indices = match explicit_step_indices.as_ref() {
            Some(indices) => indices.clone(),
            None => match response_matches {
                Some(indices) if indices.len() == 1 => indices.clone(),
                Some(indices) if indices.len() > 1 => {
                    output.anomalies.push(format!("Ambiguous responseID step mapping in {} idx {}", session_id, gen_idx));
                    continue;
                }
                _ => {
                    let reason = step_indices_result.err().unwrap_or_else(|| "step mapping is empty".to_string());
                    output.anomalies.push(format!("Missing explicit step mapping in {} idx {} (key {}): {}", session_id, gen_idx, record_key, reason));
                    continue;
                }
            },
        };

        // A unique responseID match identifies the generation's response step more precisely
        // than the broader list of generation-associated work/tool steps.
        let step_indices = if let Some(matches) = response_matches {
            if matches.len() == 1 {
                if explicit_step_indices.as_ref().is_some_and(|indices| !indices.contains(&matches[0])) {
                    output.anomalies.push(format!("Conflicting responseID and step mapping in {} idx {} (key {})", session_id, gen_idx, record_key));
                    continue;
                }
                vec![matches[0]]
            } else if matches.len() > 1 {
                output.anomalies.push(format!("Ambiguous responseID step mapping in {} idx {}", session_id, gen_idx));
                continue;
            } else {
                step_indices
            }
        } else {
            step_indices
        };

        if response_matches.is_none() && step_indices.len() != 1 {
            output.anomalies.push(format!("Ambiguous multi-step generation mapping in {} idx {} (key {})", session_id, gen_idx, record_key));
            continue;
        }

        let mut associated_steps = Vec::new();
        let mut mapping_conflict = false;
        let mut missing_step_timestamp = false;
        for step_idx in step_indices {
            let Some((timestamp, step_response_id)) = step_metadata.get(&step_idx) else {
                missing_step_timestamp = true;
                break;
            };
            if response_id.as_deref().is_some_and(|rid| step_response_id.as_deref().is_some_and(|step_rid| step_rid != rid)) {
                mapping_conflict = true;
                break;
            }
            if let Some(timestamp) = timestamp {
                associated_steps.push(*timestamp);
            } else {
                missing_step_timestamp = true;
                break;
            }
        }
        if mapping_conflict {
            output.anomalies.push(format!("Conflicting responseID and step mapping in {} idx {} (key {})", session_id, gen_idx, record_key));
            continue;
        }
        if missing_step_timestamp {
            output.anomalies.push(format!("Missing step timestamp in {} idx {} (key {})", session_id, gen_idx, record_key));
            continue;
        }
        associated_steps.sort_unstable();
        associated_steps.dedup();
        let (Some(&gen_start_time), Some(&final_timestamp)) = (associated_steps.first(), associated_steps.last()) else {
            output.anomalies.push(format!("Missing associated step timestamp in {} idx {} (key {})", session_id, gen_idx, record_key));
            continue;
        };

        let raw_json = serde_json::json!({
            "model": raw_model,
            "input_tokens": input_tokens,
            "cached_tokens": cached_tokens,
            "cache_write_tokens": cache_write_tokens,
            "output_tokens": output_tokens,
            "reasoning_tokens": reasoning_tokens,
            "request_id": request_id,
            "cascade_id": cascade_id,
            "gen_idx": gen_idx,
            "gen_start_time": gen_start_time,
            "duration_ms": duration_ms,
        })
        .to_string();

        let parsed = ParsedNativeGeneration {
            record_key: record_key.clone(),
            session_id: session_id.clone(),
            gen_idx,
            response_id,
            model,
            model_raw: raw_model,
            gen_start_time,
            timestamp: final_timestamp,
            input_tokens,
            cached_tokens,
            cache_write_tokens,
            output_tokens,
            reasoning_tokens,
            raw_json,
            duration_ms,
        };

        if let Some(&existing_idx) = dedup_index.get(&record_key) {
            output.records[existing_idx] = parsed;
        } else {
            let idx = output.records.len();
            output.records.push(parsed);
            dedup_index.insert(record_key, idx);
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::process::ClassicProcessIdentity;

    #[test]
    fn background_scheduler_uses_app_runtime_without_caller_runtime() {
        let (tx, rx) = std::sync::mpsc::channel();
        spawn_on_app_runtime(async move {
            let _ = tx.send(());
        });
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .expect("Tauri async runtime should schedule from a non-runtime thread");
    }

    fn encode_varint(mut val: u64, buf: &mut Vec<u8>) {
        loop {
            let mut byte = (val & 0x7f) as u8;
            val >>= 7;
            if val != 0 {
                byte |= 0x80;
            }
            buf.push(byte);
            if val == 0 {
                break;
            }
        }
    }

    fn encode_tag(field_num: u32, wire_type: u8, buf: &mut Vec<u8>) {
        encode_varint(((field_num as u64) << 3) | (wire_type as u64), buf);
    }

    fn encode_varint_field(field_num: u32, val: u64, buf: &mut Vec<u8>) {
        encode_tag(field_num, 0, buf);
        encode_varint(val, buf);
    }

    fn encode_bytes_field(field_num: u32, bytes: &[u8], buf: &mut Vec<u8>) {
        encode_tag(field_num, 2, buf);
        encode_varint(bytes.len() as u64, buf);
        buf.extend_from_slice(bytes);
    }

    #[test]
    fn generation_step_decoder_rejects_overflowed_packed_varint() {
        let malformed = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02];
        let fields = vec![(2, ProtoValue::Bytes(&malformed))];
        assert!(get_generation_step_indices(&fields).is_err());
    }

    fn build_step_metadata(timestamp_sec: u64) -> Vec<u8> {
        let mut ts_msg = Vec::new();
        encode_varint_field(1, timestamp_sec, &mut ts_msg);
        let mut step_meta = Vec::new();
        encode_bytes_field(1, &ts_msg, &mut step_meta);
        step_meta
    }

    fn build_step_metadata_with_response(timestamp_sec: u64, response_id: &str) -> Vec<u8> {
        let mut timestamp = Vec::new();
        encode_varint_field(1, timestamp_sec, &mut timestamp);
        let mut response = Vec::new();
        encode_bytes_field(11, response_id.as_bytes(), &mut response);
        let mut metadata = Vec::new();
        encode_bytes_field(1, &timestamp, &mut metadata);
        encode_bytes_field(9, &response, &mut metadata);
        metadata
    }

    fn build_gen_metadata_blob(
        model: &str,
        last_step_idx: i64,
        uncached_input: u64,
        output: u64,
        cache_write: u64,
        cache_read: u64,
        provider_enum_tag6: u64,
        thinking_tag9: u64,
        visible_output_tag10: u64,
        response_id: Option<&str>,
    ) -> Vec<u8> {
        let mut usage = Vec::new();
        encode_varint_field(2, uncached_input, &mut usage);
        encode_varint_field(3, output, &mut usage);
        encode_varint_field(4, cache_write, &mut usage);
        encode_varint_field(5, cache_read, &mut usage);
        encode_varint_field(6, provider_enum_tag6, &mut usage);
        encode_varint_field(9, thinking_tag9, &mut usage);
        encode_varint_field(10, visible_output_tag10, &mut usage);
        if let Some(rid) = response_id {
            encode_bytes_field(11, rid.as_bytes(), &mut usage);
        }

        let mut kv_step = Vec::new();
        encode_bytes_field(1, b"last_step_index", &mut kv_step);
        encode_bytes_field(2, last_step_idx.to_string().as_bytes(), &mut kv_step);

        let mut f1 = Vec::new();
        encode_bytes_field(4, &usage, &mut f1);
        let mut first_phase = Vec::new();
        encode_varint_field(1, 2, &mut first_phase);
        encode_varint_field(2, 500_000_000, &mut first_phase);
        encode_bytes_field(11, &first_phase, &mut f1);
        let mut second_phase = Vec::new();
        encode_varint_field(1, 1, &mut second_phase);
        encode_bytes_field(12, &second_phase, &mut f1);
        encode_bytes_field(19, model.as_bytes(), &mut f1);
        encode_bytes_field(20, &kv_step, &mut f1);

        let mut top = Vec::new();
        encode_bytes_field(1, &f1, &mut top);
        encode_varint_field(2, last_step_idx.max(0) as u64, &mut top);
        top
    }

    #[test]
    fn test_proto_parser_and_exact_field_mapping_p1() {
        // Verify [P1] fix:
        // Tag 2 = uncached input (1000)
        // Tag 3 = output (500)
        // Tag 4 = cache write (300)
        // Tag 5 = cache read (2000)
        // Tag 6 = provider enum (24 -> MUST NOT be read as cache_write!)
        // Tag 9 = thinking/reasoning (120)
        // Tag 10 = visible output (380 -> MUST NOT be read as reasoning!)
        let tmp = std::env::temp_dir().join(format!("ag_test_p1_fields_{}.db", uuid::Uuid::new_v4()));
        {
            let conn = Connection::open(&tmp).unwrap();
            conn.execute("CREATE TABLE steps (idx INTEGER PRIMARY KEY, metadata BLOB)", [])
                .unwrap();
            conn.execute("CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB)", [])
                .unwrap();

            conn.execute(
                "INSERT INTO steps (idx, metadata) VALUES (1, ?1), (2, ?2)",
                params![build_step_metadata(1_710_000_100), build_step_metadata(1_710_000_105)],
            )
            .unwrap();

            let blob = build_gen_metadata_blob(
                "gemini-3.8-flash-n",
                1,
                1000,
                500,
                300,
                2000,
                24,
                120,
                380,
                Some("resp_exact_1"),
            );
            conn.execute("INSERT INTO gen_metadata (idx, data) VALUES (1, ?1)", params![blob])
                .unwrap();
        }

        let recs = extract_native_generations_from_db(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);

        assert_eq!(recs.len(), 1);
        let r = &recs[0];
        assert_eq!(r.model, "gemini-3.8-flash");
        assert_eq!(r.model_raw, "gemini-3.8-flash-n");
        assert_eq!(r.input_tokens, 1000);
        assert_eq!(r.output_tokens, 500);
        assert_eq!(r.cache_write_tokens, 300); // Tag 4, NOT 24 from Tag 6!
        assert_eq!(r.cached_tokens, 2000);     // Tag 5
        assert_eq!(r.reasoning_tokens, 120);   // Tag 9, NOT 380 from Tag 10!
        assert_eq!(r.duration_ms, Some(3500.0));
        assert_eq!(r.gen_start_time, 1_710_000_100);
        assert_eq!(r.timestamp, 1_710_000_100);
    }

    #[test]
    fn test_missing_timestamp_recorded_as_anomaly_never_file_mtime_p1() {
        // Verify [P1] fix: missing step timestamp must NOT fall back to file modification time,
        // and must be recorded as a collection anomaly.
        let tmp_dir = std::env::temp_dir().join(format!("ag_test_missing_ts_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let db_path = tmp_dir.join("sess_no_ts.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute("CREATE TABLE steps (idx INTEGER PRIMARY KEY, metadata BLOB)", [])
                .unwrap();
            conn.execute("CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB)", [])
                .unwrap();
            // Insert generation WITHOUT any step timestamps in `steps`
            let blob = build_gen_metadata_blob(
                "gemini-3.8-flash",
                10,
                100,
                50,
                0,
                0,
                24,
                10,
                40,
                Some("resp_no_ts"),
            );
            conn.execute("INSERT INTO gen_metadata (idx, data) VALUES (1, ?1)", params![blob])
                .unwrap();
        }

        let stats_conn = Connection::open_in_memory().unwrap();
        init_native_tables(&stats_conn).unwrap();
        set_enabled_with_conn(&stats_conn, true, 1_700_000_000, None).unwrap();

        let status = scan_conversations_in_dir(&stats_conn, &tmp_dir, 1_710_000_200).unwrap();
        let _ = std::fs::remove_dir_all(&tmp_dir);

        assert_eq!(status.total_native_records, 0, "Record without step timestamp must not be imported");
        assert!(
            status.last_error.as_deref().unwrap_or("").contains("Missing step timestamp"),
            "Missing timestamp must be recorded in last_error"
        );
    }

    #[test]
    fn test_post_enablement_time_filter_and_inflight_baseline_p1() {
        // Verify [P1] fix:
        // 1. In-flight generation present in gen_metadata at enablement time (even without usage/response_id yet)
        //    is baselined by slot key and excluded when it finishes after enablement.
        // 2. Historical DB that was unreadable at enablement time and becomes readable later is rejected
        //    because its generation timestamp < enabled_since.
        let tmp_dir = std::env::temp_dir().join(format!("ag_test_baseline_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let db_inflight = tmp_dir.join("sess_inflight.db");

        // At t = 1_710_000_000, generation idx 1 is in-flight (no usage fields yet)
        {
            let conn = Connection::open(&db_inflight).unwrap();
            conn.execute("CREATE TABLE steps (idx INTEGER PRIMARY KEY, metadata BLOB)", [])
                .unwrap();
            conn.execute("CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB)", [])
                .unwrap();
            let mut f1 = Vec::new();
            encode_bytes_field(19, b"gemini-3.8-flash", &mut f1);
            let mut top = Vec::new();
            encode_bytes_field(1, &f1, &mut top);
            conn.execute("INSERT INTO gen_metadata (idx, data) VALUES (1, ?1)", params![top])
                .unwrap();
        }

        let stats_conn = Connection::open_in_memory().unwrap();
        init_native_tables(&stats_conn).unwrap();
        let enable_time = 1_710_000_500;
        set_enabled_with_conn(&stats_conn, true, enable_time, Some(&tmp_dir)).unwrap();

        // Now (after enablement, at t = 1_710_000_510), the in-flight generation idx 1 completes!
        {
            let conn = Connection::open(&db_inflight).unwrap();
            conn.execute(
                "INSERT INTO steps (idx, metadata) VALUES (1, ?1), (2, ?2)",
                params![
                    build_step_metadata(1_710_000_490),
                    build_step_metadata(1_710_000_510)
                ],
            )
            .unwrap();
            let completed_blob = build_gen_metadata_blob(
                "gemini-3.8-flash",
                1,
                500,
                200,
                0,
                0,
                24,
                50,
                150,
                Some("resp_inflight_completed"),
            );
            conn.execute(
                "UPDATE gen_metadata SET data = ?1 WHERE idx = 1",
                params![completed_blob],
            )
            .unwrap();
        }

        // Also simulate a late-recovered historical DB that was NOT present during establish_baseline_in_dir,
        // whose generation occurred at t = 1_710_000_100 (< enable_time).
        let db_historical = tmp_dir.join("sess_historical_recovered.db");
        {
            let conn = Connection::open(&db_historical).unwrap();
            conn.execute("CREATE TABLE steps (idx INTEGER PRIMARY KEY, metadata BLOB)", [])
                .unwrap();
            conn.execute("CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB)", [])
                .unwrap();
            conn.execute(
                "INSERT INTO steps (idx, metadata) VALUES (1, ?1), (2, ?2)",
                params![
                    build_step_metadata(1_710_000_090),
                    build_step_metadata(1_710_000_100)
                ],
            )
            .unwrap();
            let hist_blob = build_gen_metadata_blob(
                "gemini-3.8-flash",
                1,
                800,
                300,
                0,
                0,
                24,
                0,
                300,
                Some("resp_historical_late"),
            );
            conn.execute("INSERT INTO gen_metadata (idx, data) VALUES (1, ?1)", params![hist_blob])
                .unwrap();
        }

        // Add one genuinely new generation in sess_inflight.db at idx 2 (started at 1_710_000_520 > enable_time)
        {
            let conn = Connection::open(&db_inflight).unwrap();
            conn.execute(
                "INSERT INTO steps (idx, metadata) VALUES (3, ?1), (4, ?2)",
                params![
                    build_step_metadata(1_710_000_520),
                    build_step_metadata(1_710_000_525)
                ],
            )
            .unwrap();
            let new_blob = build_gen_metadata_blob(
                "gemini-3.8-flash",
                3,
                400,
                100,
                50,
                150,
                24,
                20,
                80,
                Some("resp_genuinely_new"),
            );
            conn.execute("INSERT INTO gen_metadata (idx, data) VALUES (2, ?1)", params![new_blob])
                .unwrap();
        }

        let status = scan_conversations_in_dir(&stats_conn, &tmp_dir, 1_710_000_530).unwrap();
        let _ = std::fs::remove_dir_all(&tmp_dir);

        assert_eq!(
            status.total_native_records, 1,
            "Only the genuinely post-enablement generation (idx 2) should be counted"
        );
        let page = list_native_records_with_conn(&stats_conn, 20, 0, None).unwrap();
        assert_eq!(page.records.len(), 1);
        assert_eq!(page.records[0].record_key, "resp_genuinely_new");
        assert_eq!(page.records[0].uncached_input_tokens, 400);
        assert_eq!(page.records[0].cached_tokens, 150);
        assert_eq!(page.records[0].cache_write_tokens, 50);
        assert_eq!(page.records[0].input_tokens, 600); // 400 + 150 + 50
        assert_eq!(page.records[0].output_tokens, 100);
        assert_eq!(page.records[0].reasoning_tokens, 20);
        assert_eq!(page.records[0].total_tokens, 700); // 600 + 100 (thinking not double-counted)
    }

    #[test]
    fn response_step_after_enablement_counts_even_when_prior_associated_step_is_before() {
        let tmp_dir = std::env::temp_dir().join(format!("ag_test_response_step_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let db_path = tmp_dir.join("sess_response_step.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute("CREATE TABLE steps (idx INTEGER PRIMARY KEY, metadata BLOB)", []).unwrap();
            conn.execute("CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB)", []).unwrap();
            conn.execute(
                "INSERT INTO steps (idx, metadata) VALUES (2, ?1), (3, ?2)",
                params![build_step_metadata(1_710_000_490), build_step_metadata_with_response(1_710_000_510, "resp_after_enable")],
            ).unwrap();
            let mut blob = build_gen_metadata_blob("gemini-3.8-flash", 2, 400, 100, 0, 0, 24, 0, 100, Some("resp_after_enable"));
            encode_varint_field(2, 3, &mut blob);
            conn.execute("INSERT INTO gen_metadata (idx, data) VALUES (1, ?1)", params![blob]).unwrap();
        }

        let stats_conn = Connection::open_in_memory().unwrap();
        init_native_tables(&stats_conn).unwrap();
        set_enabled_with_conn(&stats_conn, true, 1_710_000_500, None).unwrap();
        open_verified_observation_window_with_conn(
            &stats_conn,
            "current@example.com",
            1_710_000_500,
            "switch_observation",
            &ClassicProcessIdentity { main_pid: 77, main_start_time: 1, ls_pid: None, ls_start_time: None },
        ).unwrap();
        verify_active_observation_window_with_hooks(
            &stats_conn,
            1_710_000_520,
            || Some(ClassicProcessIdentity { main_pid: 77, main_start_time: 1, ls_pid: None, ls_start_time: None }),
            |_| true,
        ).unwrap();
        let status = scan_conversations_in_dir(&stats_conn, &tmp_dir, 1_710_000_520).unwrap();
        let _ = std::fs::remove_dir_all(&tmp_dir);

        assert_eq!(status.total_native_records, 1);
        let page = list_native_records_with_conn(&stats_conn, 10, 0, None).unwrap();
        assert_eq!(page.records[0].timestamp, 1_710_000_510);
        assert_eq!(page.records[0].account_email, "current@example.com");
        assert_eq!(page.records[0].attribution_type, "auto");
    }

    #[test]
    fn test_observation_window_process_binding_offline_and_boundary_p1() {
        // Verify [P1] fix:
        // - Window requires process identity (main_pid, main_start_time, ls_pid, ls_start_time).
        // - Generation crossing switch boundary [gen_start_time < start_time <= timestamp] is unassigned.
        // - External credential/process change invalidates the window at last_verified_at.
        // - Manager restart invalidates unclosed windows at COALESCE(last_verified_at, start_time).
        let conn = Connection::open_in_memory().unwrap();
        init_native_tables(&conn).unwrap();

        let id_a = ClassicProcessIdentity {
            main_pid: 1001,
            main_start_time: 500,
            ls_pid: Some(1002),
            ls_start_time: Some(999),
        };
        open_verified_observation_window_with_conn(
            &conn,
            "userA@example.com",
            1000,
            "switch_observation",
            &id_a,
        )
        .unwrap();

        // Advance verification to t = 1100
        verify_active_observation_window_with_hooks(
            &conn,
            1100,
            || Some(id_a.clone()),
            |email| email == "userA@example.com",
        )
        .unwrap();

        // 1. Generation strictly inside [1000, 1100] -> attributed to userA
        assert_eq!(
            resolve_record_attribution(&conn, 1020, 1050),
            ("userA@example.com".to_string(), "auto".to_string())
        );

        // 2. Generation that started at 995 (before switch at 1000) and finished at 1010 -> unassigned!
        assert_eq!(
            resolve_record_attribution(&conn, 995, 1010),
            (String::new(), "unassigned".to_string())
        );

        // 3. Simulate external credential change or process exit before scan at t = 1200:
        //    Window is closed at last_verified_at (1100), so generation at [1150, 1160] is unassigned!
        verify_active_observation_window_with_hooks(
            &conn,
            1200,
            || Some(id_a.clone()),
            |_email| false, // External credential switch!
        )
        .unwrap();

        assert_eq!(
            resolve_record_attribution(&conn, 1150, 1160),
            (String::new(), "unassigned".to_string())
        );

        // 4. Open window for userB at t = 2000, verify at t = 2030, then simulate Manager restart
        let id_b = ClassicProcessIdentity {
            main_pid: 1001,
            main_start_time: 500,
            ls_pid: Some(2002),
            ls_start_time: Some(1999),
        };
        open_verified_observation_window_with_conn(
            &conn,
            "userB@example.com",
            2000,
            "switch_observation",
            &id_b,
        )
        .unwrap();
        verify_active_observation_window_with_hooks(
            &conn,
            2030,
            || Some(id_b.clone()),
            |email| email == "userB@example.com",
        )
        .unwrap();

        // Manager restarts after offline period: window closes at last_verified_at = 2030
        invalidate_unclosed_windows_on_startup_with_conn(&conn).unwrap();

        // Offline generation at [2100, 2110] is unassigned, while [2010, 2020] stays userB
        assert_eq!(
            resolve_record_attribution(&conn, 2010, 2020),
            ("userB@example.com".to_string(), "auto".to_string())
        );
        assert_eq!(
            resolve_record_attribution(&conn, 2100, 2110),
            (String::new(), "unassigned".to_string())
        );
    }

    #[test]
    fn test_unassign_persists_across_subsequent_scans_p2() {
        // Verify [P2] fix: manually unassigning an auto-attributed record sets user_modified = 1
        // and attribution_type = 'unassigned', and subsequent scans MUST NOT re-attribute it.
        let tmp_dir = std::env::temp_dir().join(format!("ag_test_unassign_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let db_path = tmp_dir.join("sess_unassign.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute("CREATE TABLE steps (idx INTEGER PRIMARY KEY, metadata BLOB)", [])
                .unwrap();
            conn.execute("CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB)", [])
                .unwrap();
            conn.execute(
                "INSERT INTO steps (idx, metadata) VALUES (1, ?1), (2, ?2)",
                params![
                    build_step_metadata(1_710_001_010),
                    build_step_metadata(1_710_001_020)
                ],
            )
            .unwrap();
            let blob = build_gen_metadata_blob(
                "gemini-3.8-flash",
                1,
                300,
                100,
                0,
                0,
                24,
                0,
                100,
                Some("resp_unassign_1"),
            );
            conn.execute("INSERT INTO gen_metadata (idx, data) VALUES (1, ?1)", params![blob])
                .unwrap();
        }

        let stats_conn = Connection::open_in_memory().unwrap();
        init_native_tables(&stats_conn).unwrap();
        set_enabled_with_conn(&stats_conn, true, 1_710_001_000, None).unwrap();

        let id = ClassicProcessIdentity {
            main_pid: 555,
            main_start_time: 100,
            ls_pid: Some(556),
            ls_start_time: Some(200),
        };
        open_verified_observation_window_with_conn(
            &stats_conn,
            "active@example.com",
            1_710_001_000,
            "switch_observation",
            &id,
        )
        .unwrap();
        verify_active_observation_window_with_hooks(
            &stats_conn,
            1_710_001_050,
            || Some(id.clone()),
            |_| true,
        )
        .unwrap();

        // First scan: record is auto-attributed to active@example.com
        scan_conversations_in_dir(&stats_conn, &tmp_dir, 1_710_001_050).unwrap();
        let page1 = list_native_records_with_conn(&stats_conn, 20, 0, None).unwrap();
        assert_eq!(page1.records[0].account_email, "active@example.com");
        assert_eq!(page1.records[0].attribution_type, "auto");

        // User unassigns the record
        unassign_record_account_with_conn(&stats_conn, "resp_unassign_1").unwrap();
        let page2 = list_native_records_with_conn(&stats_conn, 20, 0, None).unwrap();
        assert_eq!(page2.records[0].account_email, "");
        assert_eq!(page2.records[0].attribution_type, "unassigned");

        // Subsequent scan runs while observation window is still active: must stay unassigned!
        scan_conversations_in_dir(&stats_conn, &tmp_dir, 1_710_001_080).unwrap();
        let _ = std::fs::remove_dir_all(&tmp_dir);

        let page3 = list_native_records_with_conn(&stats_conn, 20, 0, None).unwrap();
        assert_eq!(page3.records[0].account_email, "");
        assert_eq!(page3.records[0].attribution_type, "unassigned");
        assert_eq!(page3.page, 1);
        assert_eq!(page3.total_pages, 1);
    }

    #[test]
    fn generation_timing_requires_both_valid_duration_fields() {
        let mut duration = Vec::new();
        encode_varint_field(1, 2, &mut duration);
        encode_varint_field(2, 500_000_000, &mut duration);
        let zero = [];
        assert_eq!(generation_duration_ms(&[(11, ProtoValue::Bytes(&duration)), (12, ProtoValue::Bytes(&zero))]), Some(2500.0));
        assert_eq!(generation_duration_ms(&[(11, ProtoValue::Bytes(&duration))]), None);
        assert_eq!(generation_duration_ms(&[(11, ProtoValue::Bytes(&zero)), (12, ProtoValue::Bytes(&zero))]), None);
        let mut invalid_nanos = Vec::new();
        encode_varint_field(2, 1_000_000_000, &mut invalid_nanos);
        assert_eq!(generation_duration_ms(&[(11, ProtoValue::Bytes(&duration)), (12, ProtoValue::Bytes(&invalid_nanos))]), None);
    }

    #[test]
    fn graceful_close_does_not_attribute_unverified_time_to_old_account() {
        let conn = Connection::open_in_memory().unwrap();
        init_native_tables(&conn).unwrap();
        let identity = ClassicProcessIdentity { main_pid: 1, main_start_time: 1, ls_pid: Some(2), ls_start_time: Some(2) };
        open_verified_observation_window_with_conn(&conn, "old@example.test", 1000, "switch_observation", &identity).unwrap();
        verify_active_observation_window_with_hooks(&conn, 1100, || Some(identity.clone()), |_| true).unwrap();
        // An external switch occurs before the next scan; shutdown must not certify that gap.
        close_active_observation_window_with_conn(&conn, 1120).unwrap();
        invalidate_unclosed_windows_on_startup_with_conn(&conn).unwrap();
        assert_eq!(resolve_record_attribution(&conn, 1110, 1110), (String::new(), "unassigned".into()));
        assert_eq!(resolve_record_attribution(&conn, 1090, 1090).0, "old@example.test");
    }

    #[test]
    fn test_real_conversations_read_only() {
        if let Some(home) = dirs::home_dir() {
            let conv_dir = home.join(".gemini").join("antigravity").join("conversations");
            if conv_dir.exists() {
                let mut total_records = 0;
                let mut timed_records = 0;
                if let Ok(entries) = std::fs::read_dir(&conv_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().and_then(|s| s.to_str()) == Some("db") {
                            if let Ok(output) = inspect_conversation_db(&path) {
                                for r in &output.records {
                                    if crate::modules::token_throughput::tokens_per_second(r.output_tokens, r.duration_ms).is_some() {
                                        timed_records += 1;
                                    }
                                    assert!(
                                        r.reasoning_tokens <= r.output_tokens,
                                        "Reasoning tokens ({}) must be <= output tokens ({}) in {}",
                                        r.reasoning_tokens,
                                        r.output_tokens,
                                        r.record_key
                                    );
                                    let total_input =
                                        r.input_tokens + r.cached_tokens + r.cache_write_tokens;
                                    let total_tokens = total_input + r.output_tokens;
                                    assert!(
                                        total_tokens > 0,
                                        "Generation should have non-zero token count"
                                    );
                                }
                                total_records += output.records.len();
                            }
                        }
                    }
                }
                println!(
                    "test_real_conversations_read_only successfully verified {} records from live databases ({} with valid generation timing)",
                    total_records, timed_records
                );
            }
        }
    }
}
