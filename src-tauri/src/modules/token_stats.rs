use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Aggregated token statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenStatsAggregated {
    pub period: String, // e.g., "2024-01-15 14:00" for hourly, "2024-01-15" for daily
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cached_tokens: u64,
    #[serde(default)]
    pub total_reasoning_tokens: u64,
    pub total_tokens: u64,
    pub request_count: u64,
    #[serde(default)]
    pub native_generation_count: u64,
}

/// Per-account token statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountTokenStats {
    pub account_email: String,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cached_tokens: u64,
    #[serde(default)]
    pub total_reasoning_tokens: u64,
    pub total_tokens: u64,
    pub request_count: u64,
    #[serde(default)]
    pub native_generation_count: u64,
    #[serde(default)]
    pub proxy_input_tokens: u64,
    #[serde(default)]
    pub proxy_output_tokens: u64,
    #[serde(default)]
    pub proxy_cached_tokens: u64,
    #[serde(default)]
    pub proxy_total_tokens: u64,
}

/// Summary statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenStatsSummary {
    #[serde(default)]
    pub throughput: Option<crate::modules::token_throughput::TokenThroughputSummary>,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cached_tokens: u64,
    #[serde(default)]
    pub total_reasoning_tokens: u64,
    pub total_tokens: u64,
    pub total_requests: u64,
    #[serde(default)]
    pub total_native_generations: u64,
    pub unique_accounts: u64,
    #[serde(default)]
    pub unassigned_tokens: u64,
    #[serde(default)]
    pub proxy_input_tokens: u64,
    #[serde(default)]
    pub proxy_output_tokens: u64,
    #[serde(default)]
    pub proxy_cached_tokens: u64,
    #[serde(default)]
    pub proxy_total_tokens: u64,
}

/// Per-model token statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelTokenStats {
    pub model: String,
    #[serde(default)]
    pub throughput: Option<crate::modules::token_throughput::TokenThroughputSummary>,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cached_tokens: u64,
    #[serde(default)]
    pub total_reasoning_tokens: u64,
    pub total_tokens: u64,
    pub request_count: u64,
    #[serde(default)]
    pub native_generation_count: u64,
    #[serde(default)]
    pub proxy_input_tokens: u64,
    #[serde(default)]
    pub proxy_output_tokens: u64,
    #[serde(default)]
    pub proxy_cached_tokens: u64,
    #[serde(default)]
    pub proxy_total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelTrendPoint {
    pub period: String,
    pub model_data: std::collections::HashMap<String, u64>,
}

/// One read snapshot for the dashboard's selected [start, end) interval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenStatsDashboard {
    pub summary: TokenStatsSummary,
    pub models: Vec<ModelTokenStats>,
    pub timeline: Vec<TokenStatsAggregated>,
    pub model_trend: Vec<ModelTrendPoint>,
}

/// Account trend data point (for stacked area chart)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountTrendPoint {
    pub period: String,
    pub account_data: std::collections::HashMap<String, u64>,
}

pub(crate) fn get_db_path() -> Result<PathBuf, String> {
    let data_dir = crate::modules::account::get_data_dir()?;
    Ok(data_dir.join("token_stats.db"))
}

pub fn connect_db() -> Result<Connection, String> {
    let db_path = get_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    // Enable WAL mode for better concurrency
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "busy_timeout", 5000)
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| e.to_string())?;

    Ok(conn)
}

fn add_column_if_missing(conn: &Connection, table: &str, column_def: &str) -> Result<(), String> {
    let sql = format!("ALTER TABLE {} ADD COLUMN {}", table, column_def);
    match conn.execute(&sql, []) {
        Ok(_) => Ok(()),
        Err(e) if e.to_string().contains("duplicate column name") => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Initialize the token stats database
pub fn init_db() -> Result<(), String> {
    let conn = connect_db()?;

    // Create main usage table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS token_usage (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER NOT NULL,
            account_email TEXT NOT NULL,
            model TEXT NOT NULL,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            cached_tokens INTEGER NOT NULL DEFAULT 0,
            total_tokens INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    // Create indexes for efficient queries
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_token_timestamp ON token_usage (timestamp DESC)",
        [],
    )
    .map_err(|e| e.to_string())?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_token_account ON token_usage (account_email)",
        [],
    )
    .map_err(|e| e.to_string())?;

    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_token_account_timestamp ON token_usage (account_email, timestamp DESC)",
        [],
    );

    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_token_model ON token_usage (model)",
        [],
    );

    // Create hourly aggregation table for fast queries
    conn.execute(
        "CREATE TABLE IF NOT EXISTS token_stats_hourly (
            hour_bucket TEXT NOT NULL,
            account_email TEXT NOT NULL,
            total_input_tokens INTEGER NOT NULL DEFAULT 0,
            total_output_tokens INTEGER NOT NULL DEFAULT 0,
            total_cached_tokens INTEGER NOT NULL DEFAULT 0,
            total_tokens INTEGER NOT NULL DEFAULT 0,
            request_count INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (hour_bucket, account_email)
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_hourly_account ON token_stats_hourly (account_email, hour_bucket)",
        [],
    );

    add_column_if_missing(
        &conn,
        "token_usage",
        "cached_tokens INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(
        &conn,
        "token_usage",
        "reasoning_tokens INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(
        &conn,
        "token_stats_hourly",
        "total_cached_tokens INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(
        &conn,
        "token_stats_hourly",
        "total_reasoning_tokens INTEGER NOT NULL DEFAULT 0",
    )?;

    crate::modules::native_token::init_native_tables(&conn)?;
    add_column_if_missing(&conn, "token_usage", "duration_ms REAL")?;

    Ok(())
}

/// Record token usage from a request
pub fn record_usage(
    account_email: &str,
    model: &str,
    input_tokens: u32,
    output_tokens: u32,
    cached_tokens: u32,
    reasoning_tokens: u32,
) -> Result<(), String> {
    record_usage_with_duration(account_email, model, input_tokens, output_tokens, cached_tokens, reasoning_tokens, None)
}

pub fn record_usage_with_duration(
    account_email: &str,
    model: &str,
    input_tokens: u32,
    output_tokens: u32,
    cached_tokens: u32,
    reasoning_tokens: u32,
    duration_ms: Option<u64>,
) -> Result<(), String> {
    let conn = connect_db()?;
    let timestamp = chrono::Local::now().timestamp();
    let total_tokens = input_tokens + output_tokens;

    // Insert into raw usage table
    conn.execute(
        "INSERT INTO token_usage (timestamp, account_email, model, input_tokens, output_tokens, cached_tokens, reasoning_tokens, total_tokens, duration_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![timestamp, account_email, model, input_tokens, output_tokens, cached_tokens, reasoning_tokens, total_tokens, duration_ms.filter(|value| *value > 0).map(|value| value as f64)],
    ).map_err(|e| e.to_string())?;

    let hour_bucket = chrono::Local::now().format("%Y-%m-%d %H:00").to_string();
    conn.execute(
        "INSERT INTO token_stats_hourly (hour_bucket, account_email, total_input_tokens, total_output_tokens, total_cached_tokens, total_reasoning_tokens, total_tokens, request_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)
         ON CONFLICT(hour_bucket, account_email) DO UPDATE SET
            total_input_tokens = total_input_tokens + ?3,
            total_output_tokens = total_output_tokens + ?4,
            total_cached_tokens = total_cached_tokens + ?5,
            total_reasoning_tokens = total_reasoning_tokens + ?6,
            total_tokens = total_tokens + ?7,
            request_count = request_count + 1",
        params![hour_bucket, account_email, input_tokens, output_tokens, cached_tokens, reasoning_tokens, total_tokens],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

/// Attach current weekly usage without loading raw history or persisting derived totals.
pub(crate) fn populate_weekly_usage(accounts: &mut [crate::models::Account]) -> Result<(), String> {
    if !accounts.iter().any(|account| {
        account
            .quota
            .as_ref()
            .is_some_and(|q| q.quota_groups.is_some())
    }) {
        return Ok(());
    }
    let conn = connect_db()?;
    populate_weekly_usage_with_conn(&conn, accounts, chrono::Utc::now().timestamp())
}

fn populate_weekly_usage_with_conn(
    conn: &Connection,
    accounts: &mut [crate::models::Account],
    now: i64,
) -> Result<(), String> {
    let mut stmt = conn
        .prepare(
            "SELECT COALESCE(SUM(input_tokens + output_tokens), 0) FROM token_usage
         WHERE account_email = ?1 AND timestamp >= ?2 AND timestamp < ?3 AND timestamp <= ?4
         AND ((?5 = 0 AND model LIKE 'gemini%')
           OR (?5 = 1 AND (model LIKE 'claude%' OR model LIKE 'gpt%')))",
        )
        .map_err(|e| e.to_string())?;
    for account in accounts {
        let Some(groups) = account
            .quota
            .as_mut()
            .and_then(|quota| quota.quota_groups.as_mut())
        else {
            continue;
        };
        for group in groups {
            let name = group.display_name.to_lowercase();
            for bucket in &mut group.buckets {
                bucket.cycle_tokens = None;
                let Some((start, end)) = bucket.weekly_cycle_bounds(now) else {
                    continue;
                };
                let id = bucket.bucket_id.to_lowercase();
                let third_party =
                    name.contains("claude") || name.contains("gpt") || id.contains("3p");
                if !third_party && !name.contains("gemini") && !id.contains("gemini") {
                    continue;
                }
                bucket.cycle_tokens = Some(
                    stmt.query_row(
                        params![account.email, start, end, now, third_party],
                        |row| row.get(0),
                    )
                    .map_err(|e| e.to_string())?,
                );
            }
        }
    }
    Ok(())
}

fn make_usage_subquery(source: Option<&str>) -> Result<&'static str, String> {
    match source {
        None | Some("proxy") => Ok("(SELECT timestamp, account_email, model, input_tokens, output_tokens, cached_tokens, COALESCE(reasoning_tokens, 0) as reasoning_tokens, total_tokens, 1 as req_count, 0 as gen_count, input_tokens as proxy_input_tokens, output_tokens as proxy_output_tokens, cached_tokens as proxy_cached_tokens, total_tokens as proxy_total_tokens FROM token_usage)"),
        Some("antigravity") => Ok("(SELECT timestamp, CASE WHEN account_email = '' OR account_email IS NULL OR attribution_type = 'unassigned' THEN 'unassigned' ELSE account_email END as account_email, model, (input_tokens + cached_tokens + cache_write_tokens) as input_tokens, output_tokens, cached_tokens, COALESCE(reasoning_tokens, 0) as reasoning_tokens, total_tokens, 0 as req_count, 1 as gen_count, 0 as proxy_input_tokens, 0 as proxy_output_tokens, 0 as proxy_cached_tokens, 0 as proxy_total_tokens FROM native_token_usage)"),
        Some("all") => Ok("(SELECT timestamp, account_email, model, input_tokens, output_tokens, cached_tokens, COALESCE(reasoning_tokens, 0) as reasoning_tokens, total_tokens, 1 as req_count, 0 as gen_count, input_tokens as proxy_input_tokens, output_tokens as proxy_output_tokens, cached_tokens as proxy_cached_tokens, total_tokens as proxy_total_tokens FROM token_usage UNION ALL SELECT timestamp, CASE WHEN account_email = '' OR account_email IS NULL OR attribution_type = 'unassigned' THEN 'unassigned' ELSE account_email END as account_email, model, (input_tokens + cached_tokens + cache_write_tokens) as input_tokens, output_tokens, cached_tokens, COALESCE(reasoning_tokens, 0) as reasoning_tokens, total_tokens, 0 as req_count, 1 as gen_count, 0 as proxy_input_tokens, 0 as proxy_output_tokens, 0 as proxy_cached_tokens, 0 as proxy_total_tokens FROM native_token_usage)"),
        Some(other) => Err(format!("Invalid token stats source: {other}")),
    }
}

/// Get minute-level aggregated stats for a time range
pub fn get_minute_stats(
    minutes: i64,
    step_minutes: i64,
    source: Option<&str>,
) -> Result<Vec<TokenStatsAggregated>, String> {
    let conn = connect_db()?;
    let step_secs = step_minutes.max(1) * 60;
    let cutoff = if minutes <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (minutes * 60)
    };

    let subquery = make_usage_subquery(source)?;
    let sql = format!(
        "SELECT strftime('%Y-%m-%d %H:%M', datetime(timestamp - (timestamp % ?2), 'unixepoch', 'localtime')) as minute_bucket,
            COALESCE(SUM(input_tokens), 0) as input,
            COALESCE(SUM(output_tokens), 0) as output,
            COALESCE(SUM(cached_tokens), 0) as cached,
            COALESCE(SUM(reasoning_tokens), 0) as reasoning,
            COALESCE(SUM(total_tokens), 0) as total,
            COALESCE(SUM(req_count), 0) as count,
            COALESCE(SUM(gen_count), 0) as gen_count
         FROM {}
         WHERE timestamp >= ?1
         GROUP BY minute_bucket
         ORDER BY minute_bucket ASC",
        subquery
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![cutoff, step_secs], |row| {
            Ok(TokenStatsAggregated {
                period: row.get(0)?,
                total_input_tokens: row.get(1)?,
                total_output_tokens: row.get(2)?,
                total_cached_tokens: row.get(3)?,
                total_reasoning_tokens: row.get(4)?,
                total_tokens: row.get(5)?,
                request_count: row.get(6)?,
                native_generation_count: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

/// Get hourly aggregated stats for a time range
pub fn get_hourly_stats(
    hours: i64,
    source: Option<&str>,
) -> Result<Vec<TokenStatsAggregated>, String> {
    let conn = connect_db()?;
    let cutoff = if hours <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (hours * 3600)
    };

    let subquery = make_usage_subquery(source)?;
    let sql = format!(
        "SELECT strftime('%Y-%m-%d %H:00', datetime(timestamp, 'unixepoch', 'localtime')) as hour_bucket,
            COALESCE(SUM(input_tokens), 0) as input,
            COALESCE(SUM(output_tokens), 0) as output,
            COALESCE(SUM(cached_tokens), 0) as cached,
            COALESCE(SUM(reasoning_tokens), 0) as reasoning,
            COALESCE(SUM(total_tokens), 0) as total,
            COALESCE(SUM(req_count), 0) as count,
            COALESCE(SUM(gen_count), 0) as gen_count
         FROM {}
         WHERE timestamp >= ?1
         GROUP BY hour_bucket
         ORDER BY hour_bucket ASC",
        subquery
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([cutoff], |row| {
            Ok(TokenStatsAggregated {
                period: row.get(0)?,
                total_input_tokens: row.get(1)?,
                total_output_tokens: row.get(2)?,
                total_cached_tokens: row.get(3)?,
                total_reasoning_tokens: row.get(4)?,
                total_tokens: row.get(5)?,
                request_count: row.get(6)?,
                native_generation_count: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

/// Get daily aggregated stats for a time range
pub fn get_daily_stats(
    days: i64,
    source: Option<&str>,
) -> Result<Vec<TokenStatsAggregated>, String> {
    let conn = connect_db()?;
    let cutoff = if days <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (days * 86400)
    };

    let subquery = make_usage_subquery(source)?;
    let sql = format!(
        "SELECT strftime('%Y-%m-%d', datetime(timestamp, 'unixepoch', 'localtime')) as day_bucket,
            COALESCE(SUM(input_tokens), 0) as input,
            COALESCE(SUM(output_tokens), 0) as output,
            COALESCE(SUM(cached_tokens), 0) as cached,
            COALESCE(SUM(reasoning_tokens), 0) as reasoning,
            COALESCE(SUM(total_tokens), 0) as total,
            COALESCE(SUM(req_count), 0) as count,
            COALESCE(SUM(gen_count), 0) as gen_count
         FROM {}
         WHERE timestamp >= ?1
         GROUP BY day_bucket
         ORDER BY day_bucket ASC",
        subquery
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([cutoff], |row| {
            Ok(TokenStatsAggregated {
                period: row.get(0)?,
                total_input_tokens: row.get(1)?,
                total_output_tokens: row.get(2)?,
                total_cached_tokens: row.get(3)?,
                total_reasoning_tokens: row.get(4)?,
                total_tokens: row.get(5)?,
                request_count: row.get(6)?,
                native_generation_count: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

/// Get weekly aggregated stats
pub fn get_weekly_stats(
    weeks: i64,
    source: Option<&str>,
) -> Result<Vec<TokenStatsAggregated>, String> {
    let conn = connect_db()?;
    let cutoff = if weeks <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (weeks * 7 * 86400)
    };

    let subquery = make_usage_subquery(source)?;
    let sql = format!(
        "SELECT strftime('%Y-W%W', datetime(timestamp, 'unixepoch', 'localtime')) as week_bucket,
            COALESCE(SUM(input_tokens), 0) as input,
            COALESCE(SUM(output_tokens), 0) as output,
            COALESCE(SUM(cached_tokens), 0) as cached,
            COALESCE(SUM(reasoning_tokens), 0) as reasoning,
            COALESCE(SUM(total_tokens), 0) as total,
            COALESCE(SUM(req_count), 0) as count,
            COALESCE(SUM(gen_count), 0) as gen_count
         FROM {}
         WHERE timestamp >= ?1
         GROUP BY week_bucket
         ORDER BY week_bucket ASC",
        subquery
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([cutoff], |row| {
            Ok(TokenStatsAggregated {
                period: row.get(0)?,
                total_input_tokens: row.get(1)?,
                total_output_tokens: row.get(2)?,
                total_cached_tokens: row.get(3)?,
                total_reasoning_tokens: row.get(4)?,
                total_tokens: row.get(5)?,
                request_count: row.get(6)?,
                native_generation_count: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

/// Get per-account statistics for a time range
pub fn get_account_stats(
    hours: i64,
    source: Option<&str>,
) -> Result<Vec<AccountTokenStats>, String> {
    let conn = connect_db()?;
    let cutoff = if hours <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (hours * 3600)
    };
    get_account_stats_with_conn(&conn, cutoff, source)
}

pub fn get_account_stats_with_conn(
    conn: &Connection,
    cutoff: i64,
    source: Option<&str>,
) -> Result<Vec<AccountTokenStats>, String> {
    let subquery = make_usage_subquery(source)?;
    let sql = format!(
        "SELECT account_email,
            COALESCE(SUM(input_tokens), 0) as input,
            COALESCE(SUM(output_tokens), 0) as output,
            COALESCE(SUM(cached_tokens), 0) as cached,
            COALESCE(SUM(reasoning_tokens), 0) as reasoning,
            COALESCE(SUM(total_tokens), 0) as total,
            COALESCE(SUM(req_count), 0) as count,
            COALESCE(SUM(gen_count), 0) as gen_count,
            COALESCE(SUM(proxy_input_tokens), 0) as proxy_input,
            COALESCE(SUM(proxy_output_tokens), 0) as proxy_output,
            COALESCE(SUM(proxy_cached_tokens), 0) as proxy_cached,
            COALESCE(SUM(proxy_total_tokens), 0) as proxy_total
         FROM {}
         WHERE timestamp >= ?1
         GROUP BY account_email
         ORDER BY total DESC",
        subquery
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([cutoff], |row| {
            Ok(AccountTokenStats {
                account_email: row.get(0)?,
                total_input_tokens: row.get(1)?,
                total_output_tokens: row.get(2)?,
                total_cached_tokens: row.get(3)?,
                total_reasoning_tokens: row.get(4)?,
                total_tokens: row.get(5)?,
                request_count: row.get(6)?,
                native_generation_count: row.get(7)?,
                proxy_input_tokens: row.get(8)?,
                proxy_output_tokens: row.get(9)?,
                proxy_cached_tokens: row.get(10)?,
                proxy_total_tokens: row.get(11)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

/// Get summary statistics for a time range
pub fn get_summary_stats(
    hours: i64,
    source: Option<&str>,
) -> Result<TokenStatsSummary, String> {
    let conn = connect_db()?;
    let cutoff = if hours <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (hours * 3600)
    };
    get_summary_stats_with_conn(&conn, cutoff, source)
}

pub fn get_summary_stats_with_conn(
    conn: &Connection,
    cutoff: i64,
    source: Option<&str>,
) -> Result<TokenStatsSummary, String> {
    get_summary_stats_in_range(conn, cutoff, i64::MAX, source)
}

fn get_summary_stats_in_range(
    conn: &Connection,
    cutoff: i64,
    end: i64,
    source: Option<&str>,
) -> Result<TokenStatsSummary, String> {
    let subquery = make_usage_subquery(source)?;
    let sql = format!(
        "SELECT COALESCE(SUM(input_tokens), 0),
            COALESCE(SUM(output_tokens), 0),
            COALESCE(SUM(cached_tokens), 0),
            COALESCE(SUM(reasoning_tokens), 0),
            COALESCE(SUM(total_tokens), 0),
            COALESCE(SUM(req_count), 0),
            COALESCE(SUM(gen_count), 0),
            COUNT(DISTINCT CASE WHEN account_email != '' AND account_email != 'unassigned' THEN account_email END),
            COALESCE(SUM(CASE WHEN account_email = '' OR account_email = 'unassigned' THEN total_tokens ELSE 0 END), 0),
            COALESCE(SUM(proxy_input_tokens), 0),
            COALESCE(SUM(proxy_output_tokens), 0),
            COALESCE(SUM(proxy_cached_tokens), 0),
            COALESCE(SUM(proxy_total_tokens), 0)
         FROM {}
         WHERE timestamp >= ?1 AND timestamp < ?2",
        subquery
    );

    let (
        total_input,
        total_output,
        total_cached,
        total_reasoning,
        total,
        requests,
        native_generations,
        unique_accounts,
        unassigned_tokens,
        proxy_input,
        proxy_output,
        proxy_cached,
        proxy_total,
    ): (
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
    ) = conn
        .query_row(&sql, [cutoff, end], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                row.get(11)?,
                row.get(12)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    Ok(TokenStatsSummary {
        throughput: Some(crate::modules::token_throughput::summarize_range(
            conn, cutoff, end, source,
        )?),
        total_input_tokens: total_input,
        total_output_tokens: total_output,
        total_cached_tokens: total_cached,
        total_reasoning_tokens: total_reasoning,
        total_tokens: total,
        total_requests: requests,
        total_native_generations: native_generations,
        unique_accounts,
        unassigned_tokens,
        proxy_input_tokens: proxy_input,
        proxy_output_tokens: proxy_output,
        proxy_cached_tokens: proxy_cached,
        proxy_total_tokens: proxy_total,
    })
}

pub fn get_model_stats(
    hours: i64,
    source: Option<&str>,
) -> Result<Vec<ModelTokenStats>, String> {
    let conn = connect_db()?;
    let cutoff = if hours <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (hours * 3600)
    };
    get_model_stats_with_conn(&conn, cutoff, source)
}

pub fn get_model_stats_with_conn(
    conn: &Connection,
    cutoff: i64,
    source: Option<&str>,
) -> Result<Vec<ModelTokenStats>, String> {
    get_model_stats_in_range(conn, cutoff, i64::MAX, source)
}

fn get_model_stats_in_range(
    conn: &Connection,
    cutoff: i64,
    end: i64,
    source: Option<&str>,
) -> Result<Vec<ModelTokenStats>, String> {
    let subquery = make_usage_subquery(source)?;
    let rates = crate::modules::token_throughput::summarize_models(conn, cutoff, end, source)?;
    let sql = format!(
        "SELECT model,
            COALESCE(SUM(input_tokens), 0) as input,
            COALESCE(SUM(output_tokens), 0) as output,
            COALESCE(SUM(cached_tokens), 0) as cached,
            COALESCE(SUM(reasoning_tokens), 0) as reasoning,
            COALESCE(SUM(total_tokens), 0) as total,
            COALESCE(SUM(req_count), 0) as count,
            COALESCE(SUM(gen_count), 0) as gen_count,
            COALESCE(SUM(proxy_input_tokens), 0) as proxy_input,
            COALESCE(SUM(proxy_output_tokens), 0) as proxy_output,
            COALESCE(SUM(proxy_cached_tokens), 0) as proxy_cached,
            COALESCE(SUM(proxy_total_tokens), 0) as proxy_total
         FROM {}
         WHERE timestamp >= ?1 AND timestamp < ?2
         GROUP BY model
         ORDER BY total DESC",
        subquery
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([cutoff, end], |row| {
            let model: String = row.get(0)?;
            Ok(ModelTokenStats {
                throughput: rates.get(&model).cloned(),
                model,
                total_input_tokens: row.get(1)?,
                total_output_tokens: row.get(2)?,
                total_cached_tokens: row.get(3)?,
                total_reasoning_tokens: row.get(4)?,
                total_tokens: row.get(5)?,
                request_count: row.get(6)?,
                native_generation_count: row.get(7)?,
                proxy_input_tokens: row.get(8)?,
                proxy_output_tokens: row.get(9)?,
                proxy_cached_tokens: row.get(10)?,
                proxy_total_tokens: row.get(11)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

pub fn get_dashboard_stats(
    start: i64,
    end: i64,
    bucket: &str,
) -> Result<TokenStatsDashboard, String> {
    let conn = connect_db()?;
    get_dashboard_stats_with_conn(&conn, start, end, bucket)
}

pub fn get_dashboard_stats_with_conn(
    conn: &Connection,
    start: i64,
    end: i64,
    bucket: &str,
) -> Result<TokenStatsDashboard, String> {
    if start < 0 || end <= start {
        return Err("Invalid statistics date range".into());
    }
    let format = match bucket {
        "hourly" => "%Y-%m-%d %H:00",
        "daily" => "%Y-%m-%d",
        _ => return Err("Invalid statistics bucket".into()),
    };
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let summary = get_summary_stats_in_range(&tx, start, end, Some("all"))?;
    let models = get_model_stats_in_range(&tx, start, end, Some("all"))?;
    let (timeline, model_trend) = {
        let sql = format!(
            "SELECT strftime('{format}', timestamp, 'unixepoch', 'localtime'), model,
                SUM(input_tokens), SUM(output_tokens), SUM(cached_tokens), SUM(reasoning_tokens),
                SUM(total_tokens), SUM(req_count), SUM(gen_count)
             FROM {} WHERE timestamp >= ?1 AND timestamp < ?2 GROUP BY 1, model ORDER BY 1",
            make_usage_subquery(Some("all"))?
        );
        let mut stmt = tx.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([start, end], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    TokenStatsAggregated {
                        period: row.get(0)?,
                        total_input_tokens: row.get(2)?,
                        total_output_tokens: row.get(3)?,
                        total_cached_tokens: row.get(4)?,
                        total_reasoning_tokens: row.get(5)?,
                        total_tokens: row.get(6)?,
                        request_count: row.get(7)?,
                        native_generation_count: row.get(8)?,
                    },
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut timeline = std::collections::BTreeMap::<String, TokenStatsAggregated>::new();
        let mut trends =
            std::collections::BTreeMap::<String, std::collections::HashMap<String, u64>>::new();
        for row in rows {
            let (model, point) = row.map_err(|e| e.to_string())?;
            trends
                .entry(point.period.clone())
                .or_default()
                .insert(model, point.total_tokens);
            let total =
                timeline
                    .entry(point.period.clone())
                    .or_insert_with(|| TokenStatsAggregated {
                        period: point.period.clone(),
                        ..Default::default()
                    });
            total.total_input_tokens += point.total_input_tokens;
            total.total_output_tokens += point.total_output_tokens;
            total.total_cached_tokens += point.total_cached_tokens;
            total.total_reasoning_tokens += point.total_reasoning_tokens;
            total.total_tokens += point.total_tokens;
            total.request_count += point.request_count;
            total.native_generation_count += point.native_generation_count;
        }
        (
            timeline.into_values().collect(),
            trends
                .into_iter()
                .map(|(period, model_data)| ModelTrendPoint { period, model_data })
                .collect(),
        )
    };
    tx.commit().map_err(|e| e.to_string())?;
    Ok(TokenStatsDashboard {
        summary,
        models,
        timeline,
        model_trend,
    })
}

pub fn get_model_trend_minute(
    minutes: i64,
    step_minutes: i64,
    source: Option<&str>,
) -> Result<Vec<ModelTrendPoint>, String> {
    let conn = connect_db()?;
    let step_secs = step_minutes.max(1) * 60;
    let cutoff = if minutes <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (minutes * 60)
    };

    let subquery = make_usage_subquery(source)?;
    let sql = format!(
        "SELECT strftime('%Y-%m-%d %H:%M', datetime(timestamp - (timestamp % ?2), 'unixepoch', 'localtime')) as minute_bucket,
            model,
            COALESCE(SUM(total_tokens), 0) as total
         FROM {}
         WHERE timestamp >= ?1
         GROUP BY minute_bucket, model
         ORDER BY minute_bucket ASC",
        subquery
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let mut trend_map: std::collections::BTreeMap<String, std::collections::HashMap<String, u64>> =
        std::collections::BTreeMap::new();

    let rows = stmt
        .query_map(params![cutoff, step_secs], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    for row in rows {
        let (period, model, total) = row.map_err(|e| e.to_string())?;
        trend_map.entry(period).or_default().insert(model, total);
    }

    Ok(trend_map
        .into_iter()
        .map(|(period, model_data)| ModelTrendPoint { period, model_data })
        .collect())
}

pub fn get_model_trend_hourly(
    hours: i64,
    source: Option<&str>,
) -> Result<Vec<ModelTrendPoint>, String> {
    let conn = connect_db()?;
    let cutoff = if hours <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (hours * 3600)
    };

    let subquery = make_usage_subquery(source)?;
    let sql = format!(
        "SELECT strftime('%Y-%m-%d %H:00', datetime(timestamp, 'unixepoch', 'localtime')) as hour_bucket,
            model,
            COALESCE(SUM(total_tokens), 0) as total
         FROM {}
         WHERE timestamp >= ?1
         GROUP BY hour_bucket, model
         ORDER BY hour_bucket ASC",
        subquery
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let mut trend_map: std::collections::BTreeMap<String, std::collections::HashMap<String, u64>> =
        std::collections::BTreeMap::new();

    let rows = stmt
        .query_map([cutoff], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    for row in rows {
        let (period, model, total) = row.map_err(|e| e.to_string())?;
        trend_map.entry(period).or_default().insert(model, total);
    }

    Ok(trend_map
        .into_iter()
        .map(|(period, model_data)| ModelTrendPoint { period, model_data })
        .collect())
}

pub fn get_model_trend_daily(
    days: i64,
    source: Option<&str>,
) -> Result<Vec<ModelTrendPoint>, String> {
    let conn = connect_db()?;
    let cutoff = if days <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (days * 86400)
    };

    let subquery = make_usage_subquery(source)?;
    let sql = format!(
        "SELECT strftime('%Y-%m-%d', datetime(timestamp, 'unixepoch', 'localtime')) as day_bucket,
            model,
            COALESCE(SUM(total_tokens), 0) as total
         FROM {}
         WHERE timestamp >= ?1
         GROUP BY day_bucket, model
         ORDER BY day_bucket ASC",
        subquery
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let mut trend_map: std::collections::BTreeMap<String, std::collections::HashMap<String, u64>> =
        std::collections::BTreeMap::new();

    let rows = stmt
        .query_map([cutoff], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    for row in rows {
        let (period, model, total) = row.map_err(|e| e.to_string())?;
        trend_map.entry(period).or_default().insert(model, total);
    }

    Ok(trend_map
        .into_iter()
        .map(|(period, model_data)| ModelTrendPoint { period, model_data })
        .collect())
}

pub fn get_account_trend_minute(
    minutes: i64,
    step_minutes: i64,
    source: Option<&str>,
) -> Result<Vec<AccountTrendPoint>, String> {
    let conn = connect_db()?;
    let step_secs = step_minutes.max(1) * 60;
    let cutoff = if minutes <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (minutes * 60)
    };

    let subquery = make_usage_subquery(source)?;
    let sql = format!(
        "SELECT strftime('%Y-%m-%d %H:%M', datetime(timestamp - (timestamp % ?2), 'unixepoch', 'localtime')) as minute_bucket,
            account_email,
            COALESCE(SUM(total_tokens), 0) as total
         FROM {}
         WHERE timestamp >= ?1
         GROUP BY minute_bucket, account_email
         ORDER BY minute_bucket ASC",
        subquery
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let mut trend_map: std::collections::BTreeMap<String, std::collections::HashMap<String, u64>> =
        std::collections::BTreeMap::new();

    let rows = stmt
        .query_map(params![cutoff, step_secs], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    for row in rows {
        let (period, account, total) = row.map_err(|e| e.to_string())?;
        trend_map.entry(period).or_default().insert(account, total);
    }

    Ok(trend_map
        .into_iter()
        .map(|(period, account_data)| AccountTrendPoint {
            period,
            account_data,
        })
        .collect())
}

pub fn get_account_trend_hourly(
    hours: i64,
    source: Option<&str>,
) -> Result<Vec<AccountTrendPoint>, String> {
    let conn = connect_db()?;
    let cutoff = if hours <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (hours * 3600)
    };

    let subquery = make_usage_subquery(source)?;
    let sql = format!(
        "SELECT strftime('%Y-%m-%d %H:00', datetime(timestamp, 'unixepoch', 'localtime')) as hour_bucket,
            account_email,
            COALESCE(SUM(total_tokens), 0) as total
         FROM {}
         WHERE timestamp >= ?1
         GROUP BY hour_bucket, account_email
         ORDER BY hour_bucket ASC",
        subquery
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let mut trend_map: std::collections::BTreeMap<String, std::collections::HashMap<String, u64>> =
        std::collections::BTreeMap::new();

    let rows = stmt
        .query_map([cutoff], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    for row in rows {
        let (period, account, total) = row.map_err(|e| e.to_string())?;
        trend_map.entry(period).or_default().insert(account, total);
    }

    Ok(trend_map
        .into_iter()
        .map(|(period, account_data)| AccountTrendPoint {
            period,
            account_data,
        })
        .collect())
}

pub fn get_account_trend_daily(
    days: i64,
    source: Option<&str>,
) -> Result<Vec<AccountTrendPoint>, String> {
    let conn = connect_db()?;
    let cutoff = if days <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (days * 86400)
    };

    let subquery = make_usage_subquery(source)?;
    let sql = format!(
        "SELECT strftime('%Y-%m-%d', datetime(timestamp, 'unixepoch', 'localtime')) as day_bucket,
            account_email,
            COALESCE(SUM(total_tokens), 0) as total
         FROM {}
         WHERE timestamp >= ?1
         GROUP BY day_bucket, account_email
         ORDER BY day_bucket ASC",
        subquery
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let mut trend_map: std::collections::BTreeMap<String, std::collections::HashMap<String, u64>> =
        std::collections::BTreeMap::new();

    let rows = stmt
        .query_map([cutoff], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    for row in rows {
        let (period, account, total) = row.map_err(|e| e.to_string())?;
        trend_map.entry(period).or_default().insert(account, total);
    }

    Ok(trend_map
        .into_iter()
        .map(|(period, account_data)| AccountTrendPoint {
            period,
            account_data,
        })
        .collect())
}

/// Clear token usage statistics atomically within a single transaction
pub fn clear_stats(source: Option<&str>) -> Result<(), String> {
    let mut conn = connect_db()?;
    clear_stats_with_conn(&mut conn, source)
}

fn clear_stats_with_conn(conn: &mut Connection, source: Option<&str>) -> Result<(), String> {
    clear_stats_with_conn_and_dir(
        conn,
        source,
        crate::modules::native_token::get_conversations_dir().as_deref(),
    )
}

pub(crate) fn clear_stats_with_conn_and_dir(
    conn: &mut Connection,
    source: Option<&str>,
    baseline_dir: Option<&std::path::Path>,
) -> Result<(), String> {
    let s = source.unwrap_or("proxy");
    if !matches!(s, "proxy" | "antigravity" | "all") {
        return Err(format!("Invalid token stats source: {s}"));
    }
    let tx = conn.transaction().map_err(|e| e.to_string())?;

    if s == "proxy" || s == "all" {
        tx.execute("DELETE FROM token_usage", []).map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM token_stats_hourly", []).map_err(|e| e.to_string())?;
    }

    if s == "antigravity" || s == "all" {
        tx.execute("DELETE FROM native_token_usage", []).map_err(|e| e.to_string())?;
        let now = chrono::Local::now().timestamp();
        crate::modules::native_token::establish_baseline_with_conn_and_dir(&tx, now, baseline_dir)?;
    }

    tx.commit().map_err(|e| e.to_string())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_uses_one_bounded_all_source_range_for_every_aggregate() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE token_usage (timestamp INTEGER, account_email TEXT, model TEXT,
                    input_tokens INTEGER, output_tokens INTEGER, cached_tokens INTEGER,
                    total_tokens INTEGER, reasoning_tokens INTEGER, duration_ms REAL);
                 CREATE TABLE native_token_usage (timestamp INTEGER, account_email TEXT, model TEXT,
                    input_tokens INTEGER, cached_tokens INTEGER, cache_write_tokens INTEGER,
                    output_tokens INTEGER, total_tokens INTEGER, reasoning_tokens INTEGER,
                    attribution_type TEXT, duration_ms REAL);
                 INSERT INTO token_usage VALUES
                    (100, 'account-a', 'alpha', 1000, 100, 200, 1100, 20, 1000),
                    (199, 'account-a', 'beta', 10, 50, 0, 60, 0, 1000),
                    (99, 'outside', 'outside', 90000, 90000, 0, 180000, 0, 1);
                 INSERT INTO native_token_usage VALUES
                    (150, 'account-b', 'alpha', 400, 100, 50, 900, 1450, 40, 'auto', 3000),
                    (160, '', 'untimed', 20, 30, 0, 0, 50, 0, 'unassigned', NULL),
                    (200, 'outside', 'outside', 90000, 0, 0, 90000, 180000, 0, 'auto', 1);",
        )
        .unwrap();

        for bucket in ["hourly", "daily"] {
            let dashboard = get_dashboard_stats_with_conn(&conn, 100, 200, bucket).unwrap();
            let summary = &dashboard.summary;
            assert_eq!(summary.total_tokens, 2660);
            assert_eq!(summary.total_input_tokens, 1610);
            assert_eq!(summary.total_output_tokens, 1050);
            assert_eq!(summary.total_cached_tokens, 330);
            assert_eq!(summary.total_reasoning_tokens, 60);
            assert_eq!(summary.total_requests, 2);
            assert_eq!(summary.total_native_generations, 2);
            assert_eq!(
                summary.unique_accounts, 2,
                "account aggregation remains available"
            );
            assert_eq!(summary.unassigned_tokens, 50);
            assert_eq!(
                dashboard.models.iter().map(|m| m.total_tokens).sum::<u64>(),
                summary.total_tokens
            );
            assert_eq!(
                dashboard
                    .timeline
                    .iter()
                    .map(|p| p.total_tokens)
                    .sum::<u64>(),
                summary.total_tokens
            );
            assert_eq!(
                dashboard
                    .timeline
                    .iter()
                    .map(|p| p.request_count + p.native_generation_count)
                    .sum::<u64>(),
                4
            );
            assert_eq!(
                dashboard
                    .model_trend
                    .iter()
                    .flat_map(|p| p.model_data.values())
                    .sum::<u64>(),
                summary.total_tokens
            );
            assert!(dashboard.models.iter().all(|m| m.model != "outside"));
            let alpha = dashboard
                .models
                .iter()
                .find(|m| m.model == "alpha")
                .unwrap();
            assert_eq!(alpha.total_tokens, 2550);
            assert_eq!(
                alpha.throughput.as_ref().unwrap().tokens_per_second,
                Some(250.0)
            );
            assert_eq!(alpha.throughput.as_ref().unwrap().timed_generations, 2);
            let beta = dashboard.models.iter().find(|m| m.model == "beta").unwrap();
            assert_eq!(
                beta.throughput.as_ref().unwrap().tokens_per_second,
                Some(50.0)
            );
            let untimed = dashboard
                .models
                .iter()
                .find(|m| m.model == "untimed")
                .unwrap();
            assert!(untimed.throughput.is_none());
        }
        let empty = get_dashboard_stats_with_conn(&conn, 201, 300, "daily").unwrap();
        assert_eq!(empty.summary.total_tokens, 0);
        assert!(empty.models.is_empty() && empty.timeline.is_empty() && empty.model_trend.is_empty());
    }

    #[test]
    fn dashboard_rejects_invalid_ranges_and_buckets_before_querying() {
        let conn = Connection::open_in_memory().unwrap();
        for (start, end, bucket) in [
            (-1, 10, "daily"),
            (10, 10, "daily"),
            (11, 10, "daily"),
            (0, 10, "invalid"),
        ] {
            let error = get_dashboard_stats_with_conn(&conn, start, end, bucket).unwrap_err();
            assert!(error.starts_with("Invalid statistics"));
        }
    }

    #[test]
    fn usage_subquery_defaults_to_proxy_and_only_all_unions_sources() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE token_usage (timestamp INTEGER, account_email TEXT, model TEXT, input_tokens INTEGER, output_tokens INTEGER, cached_tokens INTEGER, total_tokens INTEGER, reasoning_tokens INTEGER);
             CREATE TABLE native_token_usage (timestamp INTEGER, account_email TEXT, model TEXT, input_tokens INTEGER, cached_tokens INTEGER, cache_write_tokens INTEGER, output_tokens INTEGER, total_tokens INTEGER, reasoning_tokens INTEGER, attribution_type TEXT);
             INSERT INTO token_usage VALUES (100, 'proxy', 'gpt-test', 10, 2, 1, 12, 0);
             INSERT INTO native_token_usage VALUES (100, 'native', 'gemini-test', 20, 3, 4, 5, 32, 1, 'auto');",
        ).unwrap();
        let sum_for = |source| {
            let sql = format!("SELECT COALESCE(SUM(total_tokens), 0) FROM {}", make_usage_subquery(source).unwrap());
            conn.query_row(&sql, [], |row| row.get::<_, i64>(0)).unwrap()
        };
        assert_eq!(sum_for(None), 12);
        assert_eq!(sum_for(Some("proxy")), 12);
        assert_eq!(sum_for(Some("antigravity")), 32);
        assert_eq!(sum_for(Some("all")), 44);
        assert!(make_usage_subquery(Some("unexpected")).is_err());
    }

    #[test]
    fn clear_stats_defaults_to_proxy_and_rejects_invalid_source_before_deleting() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::modules::native_token::init_native_tables(&conn).unwrap();
        conn.execute_batch(
            "CREATE TABLE token_usage (id INTEGER);
             CREATE TABLE token_stats_hourly (id INTEGER);
             INSERT INTO token_usage VALUES (1);
             INSERT INTO token_stats_hourly VALUES (1);
             INSERT INTO native_token_usage (record_key, session_id, model, timestamp, created_at) VALUES ('native-1', 's', 'gemini', 1, 1);",
        ).unwrap();
        clear_stats_with_conn(&mut conn, None).unwrap();
        let proxy_rows: i64 = conn.query_row("SELECT COUNT(*) FROM token_usage", [], |row| row.get(0)).unwrap();
        let native_rows: i64 = conn.query_row("SELECT COUNT(*) FROM native_token_usage", [], |row| row.get(0)).unwrap();
        assert_eq!(proxy_rows, 0);
        assert_eq!(native_rows, 1);

        assert!(clear_stats_with_conn(&mut conn, Some("unexpected")).is_err());
        let native_rows: i64 = conn.query_row("SELECT COUNT(*) FROM native_token_usage", [], |row| row.get(0)).unwrap();
        assert_eq!(native_rows, 1);

        conn.execute("INSERT INTO token_usage VALUES (2)", []).unwrap();
        conn.execute("INSERT INTO token_stats_hourly VALUES (2)", []).unwrap();
        clear_stats_with_conn(&mut conn, Some("antigravity")).unwrap();
        let proxy_rows: i64 = conn.query_row("SELECT COUNT(*) FROM token_usage", [], |row| row.get(0)).unwrap();
        let native_rows: i64 = conn.query_row("SELECT COUNT(*) FROM native_token_usage", [], |row| row.get(0)).unwrap();
        assert_eq!(proxy_rows, 1, "clearing Antigravity must preserve proxy usage");
        assert_eq!(native_rows, 0);

        conn.execute("INSERT INTO native_token_usage (record_key, session_id, model, timestamp, created_at) VALUES ('native-2', 's', 'gemini', 2, 2)", []).unwrap();
        clear_stats_with_conn(&mut conn, Some("all")).unwrap();
        let proxy_rows: i64 = conn.query_row("SELECT COUNT(*) FROM token_usage", [], |row| row.get(0)).unwrap();
        let native_rows: i64 = conn.query_row("SELECT COUNT(*) FROM native_token_usage", [], |row| row.get(0)).unwrap();
        assert_eq!(proxy_rows, 0, "explicit all must clear proxy usage");
        assert_eq!(native_rows, 0, "explicit all must clear native usage");
    }

    fn weekly_snapshot(observed: i64, reset: i64, fraction: f64) -> crate::models::QuotaData {
        let iso = |seconds| {
            chrono::DateTime::from_timestamp(seconds, 0)
                .unwrap()
                .to_rfc3339()
        };
        serde_json::from_value(serde_json::json!({
            "models": [], "last_updated": observed,
            "quota_groups": [
                {"display_name": "Gemini Models", "buckets": [
                    {"bucket_id": "gemini-weekly", "window": "weekly", "remaining_fraction": fraction,
                     "reset_time": iso(reset), "observed_at": observed * 1000},
                    {"bucket_id": "gemini-5h", "window": "5h", "remaining_fraction": 1,
                     "reset_time": iso(reset), "observed_at": observed * 1000}
                ]},
                {"display_name": "Claude and GPT models", "buckets": [
                    {"bucket_id": "3p-weekly", "window": "weekly", "remaining_fraction": 0.5,
                     "reset_time": iso(reset + 100), "observed_at": observed * 1000}
                ]}
            ]
        })).unwrap()
    }

    #[test]
    fn weekly_usage_tracks_accounts_groups_and_official_reset_cycles() {
        let now = 1_700_000_000;
        let reset = now + 100;
        let start = reset - 7 * 86400;
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE token_usage (timestamp INTEGER, account_email TEXT,
            model TEXT, input_tokens INTEGER, output_tokens INTEGER, cached_tokens INTEGER);",
        )
        .unwrap();
        for (email, model, timestamp, input, output) in [
            ("a", "gemini-pro", start - 1, 900, 0),
            ("a", "gemini-pro", start, 11, 2),
            ("a", "claude-sonnet", start, 900, 0),
            ("a", "claude-sonnet", start + 100, 20, 0),
            ("a", "gpt-oss", now, 30, 0),
            ("a", "custom-alias", now, 900, 0),
            ("b", "gemini-pro", start - 51, 900, 0),
            ("b", "gemini-pro", start - 50, 7, 3),
            ("b", "gemini-pro", now, 900, 0),
            ("a", "gemini-pro", reset, 40, 0),
        ] {
            conn.execute(
                "INSERT INTO token_usage VALUES (?1, ?2, ?3, ?4, ?5, 99)",
                params![timestamp, email, model, input, output],
            )
            .unwrap();
        }
        let mut accounts: Vec<_> = [("a", reset), ("b", reset - 50)]
            .into_iter()
            .map(|(email, account_reset)| {
                let token = crate::models::TokenData::new(
                    String::new(),
                    String::new(),
                    0,
                    None,
                    None,
                    None,
                    false,
                    None,
                );
                let mut account = crate::models::Account::new(email.into(), email.into(), token);
                account.update_quota(weekly_snapshot(now, account_reset, 0.0));
                account
            })
            .collect();
        let buckets = |account: &crate::models::Account| {
            account
                .quota
                .as_ref()
                .unwrap()
                .quota_groups
                .as_ref()
                .unwrap()
                .iter()
                .flat_map(|group| group.buckets.iter().map(|bucket| bucket.cycle_tokens))
                .collect::<Vec<_>>()
        };
        populate_weekly_usage_with_conn(&conn, &mut accounts, now).unwrap();
        assert_eq!(buckets(&accounts[0]), vec![Some(13), None, Some(50)]);
        assert_eq!(buckets(&accounts[1]), vec![Some(910), None, Some(0)]);

        // Legacy persisted boundaries are ignored and omitted on the next serialization.
        let mut legacy = serde_json::to_value(&accounts[0]).unwrap();
        legacy["quota"]["quota_groups"][0]["buckets"][0]["cycle_start"] = now.into();
        accounts[0] = serde_json::from_value(legacy).unwrap();
        assert!(
            serde_json::to_value(&accounts[0]).unwrap()["quota"]["quota_groups"][0]["buckets"][0]
                .get("cycle_start")
                .is_none()
        );
        populate_weekly_usage_with_conn(&conn, &mut accounts, now).unwrap();
        assert_eq!(buckets(&accounts[0]), vec![Some(13), None, Some(50)]);

        // Small and large replenishments, refreshes and reloads keep the official interval.
        for (offset, fraction) in [(1, 0.000001), (2, 1.0), (3, 0.9)] {
            accounts[0].update_quota(weekly_snapshot(now + offset, reset, fraction));
            accounts[0] =
                serde_json::from_value(serde_json::to_value(&accounts[0]).unwrap()).unwrap();
            populate_weekly_usage_with_conn(&conn, &mut accounts, now + offset).unwrap();
            assert_eq!(buckets(&accounts[0]), vec![Some(13), None, Some(50)]);
            assert_eq!(buckets(&accounts[1]), vec![Some(910), None, Some(0)]);
        }
        // An older positive observation cannot move the boundary.
        accounts[0].update_quota(weekly_snapshot(now, reset + 10, 1.0));
        populate_weekly_usage_with_conn(&conn, &mut accounts, now + 3).unwrap();
        assert_eq!(buckets(&accounts[0]), vec![Some(13), None, Some(50)]);

        // A new official reset switches cycles and includes its exact new start.
        accounts[0].update_quota(weekly_snapshot(reset, reset + 7 * 86400, 1.0));
        populate_weekly_usage_with_conn(&conn, &mut accounts, reset).unwrap();
        assert_eq!(buckets(&accounts[0]), vec![Some(40), None, None]);
        let first = &accounts[0]
            .quota
            .as_ref()
            .unwrap()
            .quota_groups
            .as_ref()
            .unwrap()[0]
            .buckets[0];
        assert_eq!(
            first.weekly_cycle_bounds(reset),
            Some((reset, reset + 7 * 86400))
        );

        // Invalid and expired periods are unavailable, never fabricated zero totals.
        accounts[0]
            .quota
            .as_mut()
            .unwrap()
            .quota_groups
            .as_mut()
            .unwrap()[0]
            .buckets[0]
            .reset_time = "invalid".into();
        populate_weekly_usage_with_conn(&conn, &mut accounts, reset + 101).unwrap();
        assert_eq!(buckets(&accounts[0]), vec![None, None, Some(0)]);
        assert_eq!(buckets(&accounts[1]), vec![None, None, None]);
    }

    #[test]
    fn test_unified_sources_and_manual_attribution_invariants() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE token_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                account_email TEXT NOT NULL,
                model TEXT NOT NULL,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cached_tokens INTEGER NOT NULL DEFAULT 0,
                reasoning_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                duration_ms REAL
            );",
        )
        .unwrap();
        crate::modules::native_token::init_native_tables(&conn).unwrap();

        // Insert 1 proxy record: input=1000, output=500, cached=200, reasoning=100, total=1500
        conn.execute(
            "INSERT INTO token_usage (timestamp, account_email, model, input_tokens, output_tokens, cached_tokens, reasoning_tokens, total_tokens)
             VALUES (1710000100, 'userA@example.com', 'gemini-3.8-flash', 1000, 500, 200, 100, 1500)",
            [],
        )
        .unwrap();

        // Insert 1 native record (unassigned): uncached_input=400, cached=300, cache_write=100 (unified input=800), output=200, reasoning=50, total=1000
        conn.execute(
            "INSERT INTO native_token_usage (
                record_key, session_id, response_id, model, timestamp,
                input_tokens, cached_tokens, cache_write_tokens, output_tokens, reasoning_tokens,
                total_tokens, account_email, attribution_type, user_modified, raw_json, created_at
             ) VALUES ('resp_1', 'sess_1', 'resp_1', 'gemini-3.8-flash', 1710000150, 400, 300, 100, 200, 50, 1000, '', 'unassigned', 0, '{}', 1710000150)",
            [],
        )
        .unwrap();

        let sum_proxy = get_summary_stats_with_conn(&conn, 0, Some("proxy")).unwrap();
        let sum_native = get_summary_stats_with_conn(&conn, 0, Some("antigravity")).unwrap();
        let sum_all = get_summary_stats_with_conn(&conn, 0, Some("all")).unwrap();

        // 1. Source 'all' equals 'proxy' + 'antigravity'
        assert_eq!(sum_proxy.total_tokens, 1500);
        assert_eq!(sum_native.total_tokens, 1000);
        assert_eq!(sum_all.total_tokens, sum_proxy.total_tokens + sum_native.total_tokens);
        assert_eq!(sum_all.total_input_tokens, sum_proxy.total_input_tokens + sum_native.total_input_tokens);
        assert_eq!(sum_all.total_output_tokens, sum_proxy.total_output_tokens + sum_native.total_output_tokens);
        assert_eq!(sum_all.total_requests, 1);
        assert_eq!(sum_all.total_native_generations, 1);

        // 2. Proxy-only cost fields never mix in native tokens
        assert_eq!(sum_all.proxy_total_tokens, 1500);
        assert_eq!(sum_native.proxy_total_tokens, 0);

        // 3. Manual attribution changes account grouping without changing totals
        crate::modules::native_token::assign_record_account_with_conn(&conn, "resp_1", "userA@example.com").unwrap();
        let sum_all_after = get_summary_stats_with_conn(&conn, 0, Some("all")).unwrap();
        assert_eq!(sum_all_after.total_tokens, sum_all.total_tokens);
        assert_eq!(sum_all_after.unassigned_tokens, 0);

        let accounts_after = get_account_stats_with_conn(&conn, 0, Some("all")).unwrap();
        assert_eq!(accounts_after.len(), 1);
        assert_eq!(accounts_after[0].account_email, "userA@example.com");
        assert_eq!(accounts_after[0].total_tokens, 2500);
        assert_eq!(accounts_after[0].proxy_total_tokens, 1500);
    }
}
