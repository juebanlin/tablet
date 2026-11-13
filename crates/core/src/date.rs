// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 juebanlin <juebanlin@gmail.com>

//! `date` 类型的展示与 Excel 互转。
//!
//! 存储层已经确定是 Unix 毫秒时间戳（`long`），这里只负责「人看的时间」：
//! - UI / 日志：按本机时区展示为 `YYYY-MM-DD HH:MM:SS`；
//! - Excel：1900 日期系统里的序列值（serial），显示格式 `yyyy-mm-dd hh:mm:ss`。
//!
//! Excel 日期不带时区，因此序列值按本机时区的墙上时间解释；同一台机器上导出再导入
//! 是无损的。跨时区共享 xlsx 时，墙上时间保持不变，绝对时刻可能变化——这与 Excel 自身
//! 的语义一致。

use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};

use crate::types::{BaseType, Paradigm, TblType, TypeParam};

/// 日期展示格式：秒级，够策划阅读，也不会把毫秒抖动带进界面。
pub const DISPLAY_FORMAT: &str = "%Y-%m-%d %H:%M:%S";
/// Excel 1900 日期系统：1899-12-30 到 1970-01-01 的天数。
const EXCEL_EPOCH_OFFSET_DAYS: f64 = 25_569.0;
const MS_PER_DAY: f64 = 86_400_000.0;

/// 类型串是否就是顶层 `date`（复合类型里的 date 在解析阶段已被拒绝）。
pub fn is_date_type(tbl_type: &str) -> bool {
    matches!(
        TblType::parse(tbl_type),
        Some(TblType {
            paradigm: Paradigm::Base,
            params,
            ..
        }) if matches!(params.first(), Some(TypeParam::Base(BaseType::Date)))
    )
}

/// 原始存储值 → 展示文本；非法值原样返回，交给校验层报错。
pub fn display_value(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    match trimmed.parse::<i64>() {
        Ok(ms) => format_epoch_ms(ms).unwrap_or_else(|| raw.to_string()),
        Err(_) => raw.to_string(),
    }
}

/// Unix 毫秒 → 本机时区的 `YYYY-MM-DD HH:MM:SS`。
pub fn format_epoch_ms(ms: i64) -> Option<String> {
    let dt = Local.timestamp_millis_opt(ms).single()?;
    Some(dt.format(DISPLAY_FORMAT).to_string())
}

/// Unix 毫秒 → Excel 1900 日期系统序列值（按本机墙上时间）。
pub fn epoch_ms_to_excel_serial(ms: i64) -> Option<f64> {
    let dt = Local.timestamp_millis_opt(ms).single()?;
    let naive = dt.naive_local();
    let wall_ms = Utc.from_utc_datetime(&naive).timestamp_millis();
    Some(wall_ms as f64 / MS_PER_DAY + EXCEL_EPOCH_OFFSET_DAYS)
}

/// Excel 单元格里的普通数字 → Unix 毫秒。
///
/// 导出时 `date` 字段写的是 Excel 序列值（现代日期约 4.5 万）；策划也可能直接在
/// 日期格里填毫秒时间戳（约 1.7 万亿）。两者量级相差极大，用 1000 万做分界：
/// 超过它按毫秒时间戳，否则按 Excel 1900 序列值。
pub fn excel_number_to_epoch_ms(n: f64) -> Option<i64> {
    if !n.is_finite() {
        return None;
    }
    if n.abs() > 10_000_000.0 {
        Some(n.round() as i64)
    } else {
        excel_serial_to_epoch_ms(n)
    }
}

/// Excel 1900 日期系统序列值 → Unix 毫秒（按本机墙上时间）。
pub fn excel_serial_to_epoch_ms(serial: f64) -> Option<i64> {
    if !serial.is_finite() {
        return None;
    }
    let wall_ms = ((serial - EXCEL_EPOCH_OFFSET_DAYS) * MS_PER_DAY).round() as i64;
    let naive = DateTime::<Utc>::from_timestamp_millis(wall_ms)?.naive_utc();
    local_naive_to_epoch_ms(naive)
}

/// Excel 单元格里的本地日期分量 → Unix 毫秒。
///
/// calamine 会把带日期格式的单元格解析成 `ExcelDateTime`，并能正确处理 1904 日期系统，
/// 所以导入路径优先走这里，而不是自己拿 serial 反推。
pub fn local_ymd_hms_milli_to_epoch_ms(
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    milli: u16,
) -> Option<i64> {
    let naive = chrono::NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32)?
        .and_hms_milli_opt(hour as u32, minute as u32, second as u32, milli as u32)?;
    local_naive_to_epoch_ms(naive)
}

fn local_naive_to_epoch_ms(naive: NaiveDateTime) -> Option<i64> {
    // DST 回拨造成的歧义取第一次出现；跳变缺失的本地时间在真实数据里几乎不会出现。
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|dt| dt.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_top_level_date_only() {
        assert!(is_date_type("date"));
        assert!(is_date_type(" date "));
        assert!(!is_date_type("long"));
        assert!(!is_date_type("List<date>"));
        assert!(!is_date_type("Tuple2<date,int>"));
    }

    #[test]
    fn display_rejects_bad_input_without_hiding_it() {
        assert_eq!(display_value(""), "");
        assert_eq!(display_value("abc"), "abc");
        assert!(display_value("1704067200000").contains("2024-01-01"));
    }

    #[test]
    fn excel_serial_round_trips_through_local_wall_clock() {
        let ms = 1_705_322_096_000i64; // 2024-01-15 12:34:56 UTC；具体本机时区不影响往返
        let serial = epoch_ms_to_excel_serial(ms).expect("serial");
        let back = excel_serial_to_epoch_ms(serial).expect("epoch");
        assert_eq!(back, ms);
    }
}
