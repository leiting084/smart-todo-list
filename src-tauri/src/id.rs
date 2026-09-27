//! 18 位时间戳风格 ID（SPEC §5）：`yyMMddHHmmssSSS`（15 位本地时间）+ 3 位随机数字。
//!
//! - 全数字、定宽 18 位，字典序即时间序（与 External 风格兼容，导入可保留原 id）；
//! - 同一毫秒内用全局单调计数器兜底，保证进程内生成的 ID 严格递增；
//! - 3 位"随机"取系统纳秒，避免同毫秒多来源碰撞，且不引入 rand 依赖。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

static LAST: OnceLock<AtomicU64> = OnceLock::new();

fn last() -> &'static AtomicU64 {
    LAST.get_or_init(|| AtomicU64::new(0))
}

/// 生成一个 18 位数字 ID 字符串。
pub fn new_id() -> String {
    let now = chrono::Local::now();
    // yyMMddHHmmss = 12 位；毫秒 3 位；合计 15 位时间前缀
    let prefix = now.format("%y%m%d%H%M%S").to_string();
    let millis = now.timestamp_subsec_millis();
    // 纳秒取模作 3 位随机后缀
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let suffix = nanos % 1000;
    let candidate: u64 = format!("{prefix}{millis:03}{suffix:03}")
        .parse()
        .expect("18 位数字 ID 必可解析为 u64");

    // CAS 保证严格大于上一个 ID（同毫秒碰撞时 +1，必要时向毫秒位进位）。
    let atomic = last();
    let final_num = loop {
        let prev = atomic.load(Ordering::Acquire);
        let target = if candidate > prev { candidate } else { prev + 1 };
        if atomic
            .compare_exchange(prev, target, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            break target;
        }
    };
    format!("{final_num:018}")
}

/// 任意本地时间 → YYYYMMDD（纯函数，可注入时钟测跨天）。
pub fn ymd_of(dt: chrono::NaiveDateTime) -> i64 {
    dt.format("%Y%m%d")
        .to_string()
        .parse()
        .expect("YYYYMMDD 必为数字")
}

/// 今天（本地时区）YYYYMMDD。
pub fn today_ymd() -> i64 {
    ymd_of(chrono::Local::now().naive_local())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime};

    #[test]
    fn id_is_18_digits() {
        for _ in 0..100 {
            let id = new_id();
            assert_eq!(id.len(), 18, "ID 必须 18 位：{id}");
            assert!(id.bytes().all(|b| b.is_ascii_digit()), "ID 必须全数字：{id}");
        }
    }

    #[test]
    fn id_prefix_is_today() {
        let id = new_id();
        let today = chrono::Local::now().format("%y%m%d").to_string();
        assert!(id.starts_with(&today), "ID 前缀应为今天 {today}，实际 {id}");
    }

    #[test]
    fn ids_are_strictly_monotonic_under_burst() {
        // 紧凑生成大量 ID，强制同毫秒碰撞，验证严格单调
        let n = 5000;
        let mut prev = 0u64;
        for _ in 0..n {
            let id = new_id();
            let num: u64 = id.parse().unwrap();
            assert!(num > prev, "ID 必须严格递增：{num} <= {prev}");
            prev = num;
        }
    }

    #[test]
    fn ymd_across_midnight_month_year() {
        let at = |d: NaiveDate, hms: &str| {
            ymd_of(d.and_time(NaiveTime::parse_from_str(hms, "%H:%M:%S").unwrap()))
        };
        // 跨天 0 点
        let day = NaiveDate::from_ymd_opt(2026, 9, 16).unwrap();
        assert_eq!(at(day, "23:59:59"), 20260916);
        assert_eq!(at(day, "00:00:00"), 20260916);
        let next = NaiveDate::from_ymd_opt(2026, 9, 17).unwrap();
        assert_eq!(at(next, "00:00:01"), 20260917);
        // 跨月/跨年/闰年
        assert_eq!(at(NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(), "00:30:00"), 20261001);
        assert_eq!(at(NaiveDate::from_ymd_opt(2027, 1, 1).unwrap(), "00:30:00"), 20270101);
        assert_eq!(at(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap(), "12:00:00"), 20240229);
    }
}
