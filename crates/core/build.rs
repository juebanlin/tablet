// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 juebanlin <juebanlin@gmail.com>

// 两件事：
//   1. 让 cargo 在 schemas/ 下任何文件改动时重编 tablet-core。
//   2. 注入构建期常量（崩溃日志用）。
//
// ── 1. schemas 重编跟踪 ──
// 原因：src/test_util.rs 用 `include_str!("../schemas/*.tblschema")` 把模板嵌进二进制，
// 但 cargo 默认只看 .rs 源文件 mtime，单独改 schema 文件不会触发 core 重编，
// 导致运行时仍是旧模板（如 HeroBase.type 改 kind 后用户依然看到 type 列）。
//
// 这里逐文件 emit rerun-if-changed；目录形式只追踪目录条目本身（增删文件），
// 不追踪文件内容修改，所以必须展开。
//
// 加新文件：放进 schemas/ 即被覆盖，无需改 build.rs。
//
// ── 2. 构建期常量（@docs/17 §5.4）──
// TARGET 是编译期信息，运行时拿不到；构建时间也不会随 exe 带出来。崩溃日志要靠它们
// 区分「同一版本号的不同构建」与目标平台。

use std::fs;
use std::path::Path;

fn main() {
    let schemas_dir = Path::new("schemas");
    println!("cargo:rerun-if-changed=schemas");
    if let Ok(entries) = fs::read_dir(schemas_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
    println!("cargo:rerun-if-changed=build.rs");

    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_TARGET={target}");

    // ISO8601 UTC，例如 2026-11-01T14:02:33Z。取不到系统时间时退化为 unknown，
    // 不用 unwrap —— 构建脚本 panic 会让整个工作区编不出来，代价远大于一个字段缺失。
    let built = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| time::OffsetDateTime::from_unix_timestamp(d.as_secs() as i64).ok())
        .and_then(|t| {
            t.format(time::macros::format_description!(
                "[year]-[month]-[day]T[hour]:[minute]:[second]Z"))
                .ok()
        })
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_TIME={built}");
}
