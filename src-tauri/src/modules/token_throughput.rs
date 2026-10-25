use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// Output throughput includes waiting for the first token. Untimed records never enter the rate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenThroughputSummary {
    pub tokens_per_second: Option<f64>,
    pub timed_generations: u64,
}

pub fn tokens_per_second(output_tokens: u64, duration_ms: Option<f64>) -> Option<f64> {
    let duration = duration_ms.filter(|value| value.is_finite() && *value > 0.0)?;
    if output_tokens == 0 {
        return None;
    }
    let rate = output_tokens as f64 / duration * 1000.0;
    rate.is_finite().then_some(rate)
}

pub fn summarize(
    conn: &Connection,
    cutoff: i64,
    source: Option<&str>,
) -> Result<TokenThroughputSummary, String> {
    summarize_range(conn, cutoff, i64::MAX, source)
}

pub fn summarize_range(
    conn: &Connection,
    start: i64,
    end: i64,
    source: Option<&str>,
) -> Result<TokenThroughputSummary, String> {
    let tables: &[&str] = match source {
        None | Some("proxy") => &["token_usage"],
        Some("antigravity") => &["native_token_usage"],
        Some("all") => &["token_usage", "native_token_usage"],
        _ => return Err("Invalid token statistics source".into()),
    };
    let mut output_tokens = 0u64;
    let mut duration_ms = 0.0;
    let mut count = 0u64;
    for table in tables {
        let sql = format!(
            "SELECT COALESCE(SUM(output_tokens), 0), COALESCE(SUM(duration_ms), 0.0), COUNT(*)
             FROM {table} WHERE timestamp >= ?1 AND timestamp < ?2 AND duration_ms > 0 AND output_tokens > 0"
        );
        let (tokens, duration, samples): (u64, f64, u64) = conn
            .query_row(&sql, [start, end], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|error| error.to_string())?;
        output_tokens = output_tokens.saturating_add(tokens);
        duration_ms += duration;
        count = count.saturating_add(samples);
    }
    Ok(TokenThroughputSummary {
        tokens_per_second: tokens_per_second(output_tokens, Some(duration_ms)),
        timed_generations: count,
    })
}

pub fn summarize_models(
    conn: &Connection,
    start: i64,
    end: i64,
    source: Option<&str>,
) -> Result<std::collections::HashMap<String, TokenThroughputSummary>, String> {
    let tables: &[&str] = match source {
        None | Some("proxy") => &["token_usage"],
        Some("antigravity") => &["native_token_usage"],
        Some("all") => &["token_usage", "native_token_usage"],
        _ => return Err("Invalid token statistics source".into()),
    };
    let mut totals = std::collections::HashMap::<String, (u64, f64, u64)>::new();
    for table in tables {
        let mut stmt = conn.prepare(&format!(
            "SELECT model, SUM(output_tokens), SUM(duration_ms), COUNT(*) FROM {table}
             WHERE timestamp >= ?1 AND timestamp < ?2 AND duration_ms > 0 AND output_tokens > 0 GROUP BY model"
        )).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([start, end], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, u64>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (model, output, duration, count) = row.map_err(|e| e.to_string())?;
            let total = totals.entry(model).or_default();
            total.0 = total.0.saturating_add(output);
            total.1 += duration;
            total.2 = total.2.saturating_add(count);
        }
    }
    Ok(totals
        .into_iter()
        .map(|(model, (output, duration, count))| {
            (
                model,
                TokenThroughputSummary {
                    tokens_per_second: tokens_per_second(output, Some(duration)),
                    timed_generations: count,
                },
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighted_throughput_respects_sources_time_and_untimed_history() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE token_usage (timestamp INTEGER, output_tokens INTEGER, duration_ms REAL);
             CREATE TABLE native_token_usage (timestamp INTEGER, output_tokens INTEGER, duration_ms REAL);
             INSERT INTO token_usage VALUES (100, 100, 1000), (100, 99999, NULL), (100, 500, 0), (1, 10000, 1);
             INSERT INTO native_token_usage VALUES (100, 900, 3000), (100, 10000, NULL), (100, 0, 100000);",
        ).unwrap();
        let proxy = summarize(&conn, 50, Some("proxy")).unwrap();
        let native = summarize(&conn, 50, Some("antigravity")).unwrap();
        let all = summarize(&conn, 50, Some("all")).unwrap();
        assert_eq!(proxy.tokens_per_second, Some(100.0));
        assert_eq!(native.tokens_per_second, Some(300.0));
        assert_eq!(all.tokens_per_second, Some(250.0));
        assert_eq!(all.timed_generations, 2);
        assert_eq!(summarize(&conn, 200, None).unwrap().tokens_per_second, None);
        assert!(summarize(&conn, 0, Some("invalid")).is_err());
    }

    #[test]
    fn missing_zero_or_invalid_timing_never_fabricates_speed() {
        for duration in [None, Some(0.0), Some(-1.0), Some(f64::NAN), Some(f64::INFINITY)] {
            assert_eq!(tokens_per_second(100, duration), None);
        }
        assert_eq!(tokens_per_second(0, Some(1000.0)), None);
        assert_eq!(tokens_per_second(25, Some(500.0)), Some(50.0));
    }

    #[test]
    fn model_rates_combine_sources_by_duration_and_honor_both_bounds() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
                "CREATE TABLE token_usage (model TEXT, timestamp INTEGER, output_tokens INTEGER, duration_ms REAL);
                 CREATE TABLE native_token_usage (model TEXT, timestamp INTEGER, output_tokens INTEGER, duration_ms REAL);
                 INSERT INTO token_usage VALUES ('a', 100, 100, 1000), ('a', 99, 90000, 1), ('a', 200, 90000, 1), ('b', 100, 500, 1000), ('untimed', 100, 200, NULL);
                 INSERT INTO native_token_usage VALUES ('a', 150, 900, 3000), ('b', 150, 900, 0);"
            ).unwrap();
        let rates = summarize_models(&conn, 100, 200, Some("all")).unwrap();
        assert_eq!(rates["a"].tokens_per_second, Some(250.0));
        assert_eq!(rates["a"].timed_generations, 2);
        assert_eq!(rates["b"].tokens_per_second, Some(500.0));
        assert!(!rates.contains_key("untimed"));
        assert_eq!(
            summarize_models(&conn, 100, 200, Some("proxy")).unwrap()["a"].tokens_per_second,
            Some(100.0)
        );
    }
}
