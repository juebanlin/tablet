//! 生成测试配置 + 可选 TestMain 入口。
//!
//! 把原 main.rs `Command::GenerateTest` 分支的全部编排逻辑搬过来，
//! 错误改成 `anyhow::bail`，不再 process::exit。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tbl_core::ops::ProjectEngine;

#[derive(Debug, Clone)]
pub struct GenerateTestOptions {
    pub include_empty: bool,
    pub schema: Option<PathBuf>,
    pub rows: usize,
    pub seed: u64,
    /// "json" / "xml"：影响 TestMain 的初始化方式。
    pub format: String,
    /// "java" / "go" / "none"。
    pub lang: String,
}

#[derive(Debug, Default)]
pub struct GenerateTestSummary {
    pub used_schema: Option<PathBuf>,
    pub data_format: String,
    pub lang: String,
}

pub fn run_generate_test(
    engine: &mut ProjectEngine,
    workdir: &Path,
    opts: GenerateTestOptions,
) -> Result<GenerateTestSummary> {
    let config_dir = engine.project().data_dir();
    let core_opts = tbl_core::test_util::TestGenOptions {
        include_empty: opts.include_empty,
        rows: opts.rows,
        seed: opts.seed,
    };

    let server = engine.project().config.export.as_ref().and_then(|e| e.server.as_ref());
    let java_pkg = server.and_then(|s| s.java.as_ref())
        .and_then(|j| j.package.as_deref())
        .unwrap_or("com.game.config")
        .to_string();
    let go_pkg = server.and_then(|s| s.go.as_ref())
        .and_then(|g| g.package.as_deref())
        .unwrap_or("config")
        .to_string();
    let go_code_output = server.and_then(|s| s.go.as_ref())
        .and_then(|g| g.code_output.as_deref())
        .unwrap_or("gen/server/go")
        .to_string();

    if let Some(schema_path) = &opts.schema {
        let content = std::fs::read_to_string(schema_path)
            .with_context(|| format!("读取 schema 失败: {}", schema_path.display()))?;
        let parsed = tbl_core::tblschema::parse_tblschema(&content)
            .map_err(|e| anyhow::anyhow!("解析 schema 失败: {}", e))?;
        tbl_core::test_util::generate_from_schema(&config_dir, &parsed, &core_opts);

        match opts.lang.as_str() {
            "java" => tbl_core::test_util::generate_test_main_from_schema(workdir, &parsed, &java_pkg, &opts.format),
            "go" => tbl_core::test_util::generate_test_main_go_from_schema(workdir, &parsed, &go_pkg, &go_code_output, &opts.format),
            "none" => {}
            other => anyhow::bail!("未知 --lang: {}", other),
        }
    } else {
        tbl_core::test_util::generate_test_config(&config_dir, &core_opts);

        match opts.lang.as_str() {
            "java" => tbl_core::test_util::generate_test_main(workdir, &core_opts, &java_pkg, &opts.format),
            "go" => tbl_core::test_util::generate_test_main_go(workdir, &core_opts, &go_pkg, &go_code_output, &opts.format),
            "none" => {}
            other => anyhow::bail!("未知 --lang: {}", other),
        }
    }

    Ok(GenerateTestSummary {
        used_schema: opts.schema,
        data_format: opts.format,
        lang: opts.lang,
    })
}
