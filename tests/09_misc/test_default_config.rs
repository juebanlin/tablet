// 验证 DEFAULT_CONFIG 字符串模板与 GlobalConfig::default() 的一致性

use tablet_core::model::GlobalConfig;

#[test]
fn test_default_config_consistency() {
    // 从 DEFAULT_CONFIG 模板解析
    let config_from_str: GlobalConfig = toml::from_str(tablet_core::project::default_config_template())
        .expect("DEFAULT_CONFIG 模板解析失败");

    // 从 Default 实现获取
    let config_from_default = GlobalConfig::default();

    // 验证 UI 配置
    let ui_from_str = config_from_str.ui.expect("模板中应包含 [ui] 段");
    let ui_from_default = config_from_default.ui.expect("Default 应返回 Some(UiConfig)");

    assert_eq!(ui_from_str.auto_commit_on_blur, ui_from_default.auto_commit_on_blur, "auto_commit_on_blur 不一致");
    assert_eq!(ui_from_str.realtime_validate, ui_from_default.realtime_validate, "realtime_validate 不一致");
    assert_eq!(ui_from_str.log_level, ui_from_default.log_level, "log_level 不一致");
    assert_eq!(ui_from_str.picker_trigger_header, ui_from_default.picker_trigger_header, "picker_trigger_header 不一致");
    assert_eq!(ui_from_str.picker_trigger_data, ui_from_default.picker_trigger_data, "picker_trigger_data 不一致");
    assert_eq!(ui_from_str.show_meta_id, ui_from_default.show_meta_id, "show_meta_id 不一致");
    assert_eq!(ui_from_str.constant_ref_allowed, ui_from_default.constant_ref_allowed, "constant_ref_allowed 不一致");

    // 验证 Export 配置（顶层字段）
    let export_from_str = config_from_str.export.expect("模板中应包含 [export] 段");
    let export_from_default = config_from_default.export.expect("Default 应返回 Some(ExportConfig)");

    assert_eq!(export_from_str.encoding, export_from_default.encoding, "export.encoding 不一致");
    assert_eq!(export_from_str.line_ending, export_from_default.line_ending, "export.line_ending 不一致");

    // 验证各导出子段存在性
    assert!(export_from_str.json.is_some(), "模板中应包含 [export.json]");
    assert!(export_from_default.json.is_some(), "Default 应包含 json");

    assert!(export_from_str.xml.is_some(), "模板中应包含 [export.xml]");
    assert!(export_from_default.xml.is_some(), "Default 应包含 xml");

    assert!(export_from_str.server.is_some(), "模板中应包含 [export.server]");
    assert!(export_from_default.server.is_some(), "Default 应包含 server");

    assert!(export_from_str.client.is_some(), "模板中应包含 [export.client]");
    assert!(export_from_default.client.is_some(), "Default 应包含 client");

    // 验证 separators
    assert_eq!(config_from_str.separators, config_from_default.separators, "separators 不一致");

    println!("✓ DEFAULT_CONFIG 模板与 GlobalConfig::default() 一致");
}
