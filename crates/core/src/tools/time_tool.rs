//! **TimeTool** — provides the current date and time.

use crate::agent::tool::{Tool, ToolError, ToolSpec};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

/// A tool that returns the current system date and time.
///
/// No arguments required. Returns ISO 8601 format and Unix timestamp.
pub struct TimeTool;

#[async_trait::async_trait]
impl Tool for TimeTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "current_time".into(),
            description: "Returns the current system date and time (ISO 8601) and Unix timestamp. Useful for time-sensitive operations.".into(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
            category: crate::agent::tool::ToolCategory::Generic,
        }
    }

    async fn call(&self, _args: Value) -> Result<Value, ToolError> {
        let now =
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| ToolError::Execution {
                    tool: "current_time".into(),
                    message: format!("time error: {e}"),
                })?;

        let unix_secs = now.as_secs();
        let millis = now.subsec_millis();

        Ok(json!({
            "unix_timestamp_secs": unix_secs,
            "unix_timestamp_ms": unix_secs as u64 * 1000 + millis as u64,
            "date_iso": iso_date(unix_secs),
            "timezone": "UTC",
        }))
    }
}

/// Crude but dependency-free ISO date formatter.
fn iso_date(unix_secs: u64) -> String {
    // Days since epoch
    let days = unix_secs / 86400;
    let remaining = unix_secs % 86400;
    let hours = remaining / 3600;
    let minutes = (remaining % 3600) / 60;
    let seconds = remaining % 60;

    // Gregorian calendar from days since 1970-01-01
    let mut y = 1970i64;
    let mut d = days as i64;

    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if d < days_in_year {
            break;
        }
        d -= days_in_year;
        y += 1;
    }

    let month_days = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut m = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if d < md {
            m = i + 1;
            break;
        }
        d -= md;
    }
    let day = d + 1;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, day, hours, minutes, seconds
    )
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iso_date_epoch() {
        assert_eq!(iso_date(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn test_iso_date_known() {
        // 2024-01-15T06:30:00Z = 1705300200 (6h30m after midnight Jan 15)
        assert_eq!(iso_date(1705300200), "2024-01-15T06:30:00Z");
    }

    #[test]
    fn test_tool_call() {
        let tool = TimeTool;
        let args = json!({});
        let result = tool.call(args);
        let result = tokio::runtime::Runtime::new().unwrap().block_on(result);
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(
            val.get("unix_timestamp_secs")
                .and_then(Value::as_u64)
                .unwrap()
                > 1_700_000_000
        );
    }
}
