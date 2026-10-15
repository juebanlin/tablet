use std::fmt::Write;
use std::collections::HashSet;
use anyhow::Result;
use crate::model::*;
use crate::types::*;
use super::{to_camel_case, to_pascal_case, comment_block, LineEnding};

const TPL_ITPL: &str = include_str!("../../templates/java/ITpl.java");
const TPL_ICONST: &str = include_str!("../../templates/java/IConstTpl.java");
const TPL_IJSON_PARSER: &str = include_str!("../../templates/java/IJsonParser.java");
const TPL_IXML_PARSER: &str = include_str!("../../templates/java/IXmlParser.java");
const TPL_SIMPLE_JSON_PARSER: &str = include_str!("../../templates/java/SimpleJsonParser.java");
const TPL_SIMPLE_XML_PARSER: &str = include_str!("../../templates/java/SimpleXmlParser.java");
const TPL_UTIL: &str = include_str!("../../templates/java/TplUtil.java");
const TPL_HOLDER: &str = include_str!("../../templates/java/TplHolder.java");
const TPL_REGISTRY: &str = include_str!("../../templates/java/TplRegistry.java");
const TPL_TBL_META: &str = include_str!("../../templates/java/TblMeta.java");
const TPL_TBL_TYPE: &str = include_str!("../../templates/java/TblFieldType.java");
const TPL_TBL_SOURCE: &str = include_str!("../../templates/java/TblSource.java");
const TPL_TBL_DATA_SOURCE: &str = include_str!("../../templates/java/TblDataSource.java");
const TPL_TBL_FIELD_MISMATCH_HANDLER: &str = include_str!("../../templates/java/TblFieldMismatchHandler.java");
const TPL_TBL_LOAD_LOGGER: &str = include_str!("../../templates/java/TblLoadLogger.java");
const TPL_FILE_SYSTEM_TBL_DATA_SOURCE: &str = include_str!("../../templates/java/FileSystemTblDataSource.java");
const TPL_CLASSPATH_TBL_DATA_SOURCE: &str = include_str!("../../templates/java/ClasspathTblDataSource.java");
const TPL_PARADIGM: &str = include_str!("../../templates/java/Paradigm.java");
const TPL_SEP_CONFIG: &str = include_str!("../../templates/java/SepConfig.java");
const TPL_TUPLE2: &str = include_str!("../../templates/java/Tuple2.java");
const TPL_TUPLE3: &str = include_str!("../../templates/java/Tuple3.java");
const TPL_TUPLE4: &str = include_str!("../../templates/java/Tuple4.java");
const TPL_EXAMPLE: &str = include_str!("../../templates/java/Example.java");

fn is_server_export(export: &Export) -> bool {
    matches!(export, Export::ClientServer | Export::ServerOnly)
}

fn java_field_type(tbl_type_str: &str, enum_names: &HashSet<String>) -> String {
    if let Some(t) = TblType::parse(tbl_type_str) {
        if t.paradigm == Paradigm::Ref {
            if let Some(name) = &t.ref_name {
                if enum_names.contains(name) {
                    return format!("{}Enum", name);
                }
            }
            return "int".to_string();
        }
        return t.java_decl();
    }
    "String".to_string()
}

fn needs_tuple_import(tbl_type_str: &str) -> bool {
    matches!(TblType::parse(tbl_type_str).map(|t| t.paradigm),
        Some(Paradigm::Tuple2) | Some(Paradigm::Tuple3) | Some(Paradigm::Tuple4) |
        Some(Paradigm::ListTuple2) | Some(Paradigm::ListTuple3) | Some(Paradigm::ListTuple4) |
        Some(Paradigm::MapTuple2) | Some(Paradigm::MapTuple3) | Some(Paradigm::MapTuple4))
}

// PLACEHOLDER_JAVA_RS_PART2

/// 拼 `@TblFieldType(...)` 的实参。
///
/// 数据文件的 key 是 .tbl 源字段名（snake_case），反射加载器按 `field.getName()`（camelCase）
/// 取不到，所以源名与 Java 字段名不一致时要把源名带上；一致的（`id` / `name` 这种）省掉，
/// 让大多数行保持原样。规则见 @01.8.1。
fn field_type_ann(tbl_type: &str, src_name: &str, java_name: &str) -> String {
    if src_name == java_name {
        format!("\"{}\"", tbl_type)
    } else {
        format!("value = \"{}\", name = \"{}\"", tbl_type, src_name)
    }
}

fn gen_table_tpl(table: &Table, pkg: &str, group: &str, enum_names: &HashSet<String>, lombok: bool, custom_regions: bool) -> String {
    let mut s = String::new();
    writeln!(s, "package {}.tpl;", pkg).unwrap();
    writeln!(s).unwrap();
    writeln!(s, "import {}.ITpl;", pkg).unwrap();
    writeln!(s, "import {}.TblFieldType;", pkg).unwrap();
    writeln!(s, "import {}.TblSource;", pkg).unwrap();
    writeln!(s, "import java.util.*;").unwrap();

    let fields: Vec<&FieldDef> = table.schema.fields.iter()
        .filter(|f| is_server_export(&f.export))
        .collect();

    if fields.iter().any(|f| needs_tuple_import(&f.tbl_type)) {
        writeln!(s, "import {}.types.*;", pkg).unwrap();
    }
    if lombok {
        writeln!(s, "import lombok.Getter;").unwrap();
    }
    writeln!(s).unwrap();
    writeln!(s, "@TblSource(group = \"{}\", name = \"{}\", mode = \"table\")", group, table.name).unwrap();
    if lombok {
        writeln!(s, "@Getter").unwrap();
    }
    writeln!(s, "public class {}Tpl implements ITpl {{", table.name).unwrap();

    for f in &fields {
        let jtype = java_field_type(&f.tbl_type, enum_names);
        let name = to_camel_case(&f.name);
        if !f.desc.is_empty() {
            writeln!(s, "    /** {} */", comment_block(&f.desc)).unwrap();
        }
        writeln!(s, "    @TblFieldType({})", field_type_ann(&f.tbl_type, &f.name, &name)).unwrap();
        writeln!(s, "    private {} {};", jtype, name).unwrap();
    }
    writeln!(s).unwrap();

    // getId() 必须显式写：ITpl 接口声明了它，TplHolder 注册表也在调（@TplHolder）。
    // Lombok 见到同名方法会跳过，不会生成重复的。
    let index_field = to_camel_case("id");
    writeln!(s, "    @Override").unwrap();
    writeln!(s, "    public int getId() {{ return {}; }}", index_field).unwrap();

    // 其余 getter 交给 Lombok。注意 boolean 字段它生成的是 isXxx() 而不是 getXxx()。
    if !lombok {
        writeln!(s).unwrap();
        for f in &fields {
            if f.name == "id" { continue; }
            let jtype = java_field_type(&f.tbl_type, enum_names);
            let camel = to_camel_case(&f.name);
            let pascal = to_pascal_case(&f.name);
            writeln!(s, "    public {} get{}() {{ return {}; }}", jtype, pascal, camel).unwrap();
        }
    }
    if custom_regions {
        writeln!(s).unwrap();
        writeln!(s, "{}", super::custom_region("    ")).unwrap();
    }
    writeln!(s, "}}").unwrap();
    s
}

fn gen_constant_tpl(constant: &Constant, pkg: &str, group: &str, enum_names: &HashSet<String>, lombok: bool, custom_regions: bool) -> String {
    let mut s = String::new();
    writeln!(s, "package {}.tpl;", pkg).unwrap();
    writeln!(s).unwrap();
    writeln!(s, "import {}.IConstTpl;", pkg).unwrap();
    writeln!(s, "import {}.TblFieldType;", pkg).unwrap();
    writeln!(s, "import {}.TblSource;", pkg).unwrap();
    writeln!(s, "import java.util.*;").unwrap();

    let entries: Vec<&ConstEntry> = constant.entries.iter()
        .filter(|e| is_server_export(&e.export) && !e.name.is_empty())
        .collect();

    if entries.iter().any(|e| needs_tuple_import(&e.tbl_type)) {
        writeln!(s, "import {}.types.*;", pkg).unwrap();
    }
    if lombok {
        writeln!(s, "import lombok.Getter;").unwrap();
    }
    writeln!(s).unwrap();
    writeln!(s, "@TblSource(group = \"{}\", name = \"{}\", mode = \"constant\")", group, constant.name).unwrap();
    if lombok {
        writeln!(s, "@Getter").unwrap();
    }
    writeln!(s, "public class {}Tpl implements IConstTpl {{", constant.name).unwrap();

    for e in &entries {
        let jtype = java_field_type(&e.tbl_type, enum_names);
        let name = to_camel_case(&e.name);
        if !e.desc.is_empty() {
            writeln!(s, "    /** {} */", comment_block(&e.desc)).unwrap();
        }
        writeln!(s, "    @TblFieldType({})", field_type_ann(&e.tbl_type, &e.name, &to_camel_case(&e.name))).unwrap();
        writeln!(s, "    private {} {};", jtype, name).unwrap();
    }
    writeln!(s).unwrap();

    // IConstTpl 是空接口，没有强制；getter 纯粹给业务代码用，所以整块可以交给 Lombok
    if !lombok {
        for e in &entries {
            let jtype = java_field_type(&e.tbl_type, enum_names);
            let camel = to_camel_case(&e.name);
            let pascal = to_pascal_case(&e.name);
            writeln!(s, "    public {} get{}() {{ return {}; }}", jtype, pascal, camel).unwrap();
        }
    }
    if custom_regions {
        writeln!(s).unwrap();
        writeln!(s, "{}", super::custom_region("    ")).unwrap();
    }
    writeln!(s, "}}").unwrap();
    s
}

/// Java 字符串字面量转义：desc 会作为枚举构造参数写进源码，
/// 含 `"` `\` 或换行会直接破坏生成的 .java。
fn escape_java_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn gen_enum_class(enum_def: &EnumDef, pkg: &str, group: &str, custom_regions: bool) -> String {
    let mut s = String::new();
    writeln!(s, "package {}.tpl;", pkg).unwrap();
    writeln!(s).unwrap();
    writeln!(s, "import {}.TblSource;", pkg).unwrap();
    writeln!(s).unwrap();

    let valid: Vec<&EnumEntry> = enum_def.entries.iter()
        .filter(|e| !e.id.is_empty() && !e.name.is_empty())
        .collect();

    writeln!(s, "@TblSource(group = \"{}\", name = \"{}\", mode = \"enum\")", group, enum_def.name).unwrap();
    writeln!(s, "public enum {}Enum {{", enum_def.name).unwrap();

    for (i, e) in valid.iter().enumerate() {
        writeln!(s, "    /** {} */", comment_block(&e.desc)).unwrap();
        let sep = if i + 1 == valid.len() { ";" } else { "," };
        // desc 是作为构造参数写进源码的字符串字面量，含 " \ 换行 会破坏生成文件；
        // id 同理按整数写出（其他后端也都这么兜底）
        let id = e.id.parse::<i64>().unwrap_or(0);
        writeln!(s, "    {}({}, \"{}\"){}", e.name, id, escape_java_str(&e.desc), sep).unwrap();
    }
    writeln!(s).unwrap();

    writeln!(s, "    public final int id;").unwrap();
    writeln!(s, "    public final String desc;").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "    {}Enum(int id, String desc) {{", enum_def.name).unwrap();
    writeln!(s, "        this.id = id;").unwrap();
    writeln!(s, "        this.desc = desc;").unwrap();
    writeln!(s, "    }}").unwrap();
    writeln!(s).unwrap();
    // getter 与 Tpl 类保持同一风格；枚举不实现 ITpl，这里只是给业务提供统一取 id / desc 的入口
    writeln!(s, "    public int getId() {{ return id; }}").unwrap();
    writeln!(s, "    public String getDesc() {{ return desc; }}").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "    public static {}Enum fromId(int id) {{", enum_def.name).unwrap();
    writeln!(s, "        for ({}Enum e : values()) if (e.id == id) return e;", enum_def.name).unwrap();
    writeln!(s, "        return null;").unwrap();
    writeln!(s, "    }}").unwrap();
    // Java 的 enum 是完整类，业务判断函数（IsDefenseBuilding() 之类）直接放这里
    if custom_regions {
        writeln!(s).unwrap();
        writeln!(s, "{}", super::custom_region("    ")).unwrap();
    }
    writeln!(s, "}}").unwrap();
    s
}

pub fn export_all_java(project: &Project) -> Result<super::ExportResult> {
    // Java 字段名走 camelCase，先挡掉转换后撞名（见 check_key_collision）
    super::check_project_key_collisions(project)?;
    let export_cfg = &project.config.export;
    let server = export_cfg.server.as_ref();
    let java = server.and_then(|s| s.java.as_ref());

    let code_output = java
        .and_then(|j| j.code_output.as_deref())
        .unwrap_or("gen/server/java");
    let pkg = java
        .and_then(|j| j.package.as_deref())
        .unwrap_or("com.game.config");
    // 缺省开：字段 getter 交给 Lombok 的 @Getter
    let lombok = java.and_then(|j| j.lombok).unwrap_or(true);
    // 缺省关：不配就与没有本特性时逐字节一致
    let custom_regions = java.and_then(|j| j.custom_regions).unwrap_or(true);

    let line_ending = LineEnding::from_config(
        export_cfg.line_ending.map(|l| l.as_str())
            .unwrap_or("lf")
    );
    let encoding = export_cfg.encoding.map(|e| e.as_str())
        .unwrap_or("utf-8").to_string();
    let opts = super::ExportOptions { line_ending, encoding };

    let pkg_path = pkg.replace('.', "/");
    let output_dir = project.export_root().join(code_output).join(&pkg_path);
    let types_dir = output_dir.join("types");
    let tpl_dir = output_dir.join("tpl");

    // 收的是未编码的源码：保留区间要在 sync_export_dir 里拼接，到那儿再编码
    let mut collected: Vec<(std::path::PathBuf, String)> = Vec::new();

    let mut collect = |dir: &std::path::Path, name: &str, content: &str| {
        let path = dir.join(name);
        collected.push((path, content.to_string()));
    };

    let render = |tpl: &str| tpl.replace("{{PACKAGE}}", pkg);

    collect(&output_dir, "ITpl.java", &render(TPL_ITPL));
    collect(&output_dir, "IConstTpl.java", &render(TPL_ICONST));
    collect(&output_dir, "IJsonParser.java", &render(TPL_IJSON_PARSER));
    collect(&output_dir, "IXmlParser.java", &render(TPL_IXML_PARSER));
    collect(&output_dir, "SimpleJsonParser.java", &render(TPL_SIMPLE_JSON_PARSER));
    collect(&output_dir, "SimpleXmlParser.java", &render(TPL_SIMPLE_XML_PARSER));
    collect(&output_dir, "TplUtil.java", &render(TPL_UTIL));
    collect(&output_dir, "TblFieldType.java", &render(TPL_TBL_TYPE));
    collect(&output_dir, "TblSource.java", &render(TPL_TBL_SOURCE));
    collect(&output_dir, "TblMeta.java", &render(TPL_TBL_META));
    collect(&output_dir, "TblDataSource.java", &render(TPL_TBL_DATA_SOURCE));
    collect(&output_dir, "TblFieldMismatchHandler.java", &render(TPL_TBL_FIELD_MISMATCH_HANDLER));
    collect(&output_dir, "TblLoadLogger.java", &render(TPL_TBL_LOAD_LOGGER));
    collect(&output_dir, "FileSystemTblDataSource.java", &render(TPL_FILE_SYSTEM_TBL_DATA_SOURCE));
    collect(&output_dir, "ClasspathTblDataSource.java", &render(TPL_CLASSPATH_TBL_DATA_SOURCE));
    collect(&output_dir, "Paradigm.java", &render(TPL_PARADIGM));
    collect(&output_dir, "SepConfig.java", &render(TPL_SEP_CONFIG));
    collect(&output_dir, "Example.java", &render(TPL_EXAMPLE));
    collect(&types_dir, "Tuple2.java", &render(TPL_TUPLE2));
    collect(&types_dir, "Tuple3.java", &render(TPL_TUPLE3));
    collect(&types_dir, "Tuple4.java", &render(TPL_TUPLE4));

    let mut register_lines = String::new();
    let enum_names: HashSet<String> = project.groups.iter()
        .flat_map(|g| g.enums.iter())
        .filter(|e| !e.deleted)
        .map(|e| e.name.clone())
        .collect();

    for group in &project.groups {
        for table in &group.tables {
            if table.deleted { continue; }
            let content = gen_table_tpl(table, pkg, &group.name, &enum_names, lombok, custom_regions);
            let filename = format!("{}Tpl.java", &table.name);
            collect(&tpl_dir, &filename, &content);
            writeln!(register_lines, "        map.put(\"{}\", {}.tpl.{}Tpl.class);",
                table.name, pkg, table.name).unwrap();
        }

        for constant in &group.constants {
            if constant.deleted { continue; }
            let content = gen_constant_tpl(constant, pkg, &group.name, &enum_names, lombok, custom_regions);
            let filename = format!("{}Tpl.java", &constant.name);
            collect(&tpl_dir, &filename, &content);
            writeln!(register_lines, "        map.put(\"{}\", {}.tpl.{}Tpl.class);",
                constant.name, pkg, constant.name).unwrap();
        }

        for enum_def in &group.enums {
            if enum_def.deleted { continue; }
            let content = gen_enum_class(enum_def, pkg, &group.name, custom_regions);
            let filename = format!("{}Enum.java", &enum_def.name);
            collect(&tpl_dir, &filename, &content);
        }
    }

    let registry_content = render(TPL_REGISTRY).replace("{{REGISTER_LIST}}", register_lines.trim_end());
    collect(&output_dir, "TplRegistry.java", &registry_content);

    let holder_content = render(TPL_HOLDER);
    collect(&output_dir, "TplHolder.java", &holder_content);

    super::sync_export_dir(&output_dir, "java", collected, &opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_enums() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn nested_ref_maps_to_integer_list() {
        // S24：复杂类型内 ref 元素退化为 int（List<Integer> 等）
        assert_eq!(java_field_type("List<@Hero>", &no_enums()), "List<Integer>");
        assert_eq!(java_field_type("Set<@Hero>", &no_enums()), "Set<Integer>");
        assert_eq!(java_field_type("Map<int,@Hero>", &no_enums()), "Map<Integer,Integer>");
        assert_eq!(java_field_type("Tuple2<@Hero,int>", &no_enums()), "Tuple2<Integer,Integer>");
    }

    #[test]
    fn enum_has_get_id() {
        // 枚举不实现 ITpl，getId() 只为与 Tpl 类的取值风格一致；fromId 是反射加载要用，不能动
        let e = EnumDef {
            name: "HeroType".to_string(),
            path: std::path::PathBuf::new(),
            entries: vec![
                EnumEntry { id: "1".to_string(), name: "Warrior".to_string(), desc: "战士".to_string() },
            ],
            dirty: false, deleted: false, original_entries: Vec::new(), saved: true,
            display_field: None,
        };
        let out = gen_enum_class(&e, "com.game.config", "hero", false);
        assert!(out.contains("public int getId() { return id; }"), "got:\n{}", out);
        // desc 也要有 getter：只补 getId 会留下 id 双入口、desc 单入口的不对称
        assert!(out.contains("public String getDesc() { return desc; }"), "got:\n{}", out);
        assert!(out.contains("public static HeroTypeEnum fromId(int id)"), "got:\n{}", out);
    }

    #[test]
    fn enum_desc_is_escaped_in_literal_and_comment() {
        // desc 既进 Javadoc 又进构造参数；引号/反斜杠/换行/注释终止符都不能漏出去
        let e = EnumDef {
            name: "HeroType".to_string(),
            path: std::path::PathBuf::new(),
            entries: vec![
                EnumEntry {
                    id: "1".to_string(),
                    name: "Warrior".to_string(),
                    desc: "他说\"你好\"\\ 结束 */ 换行\n第二行".to_string(),
                },
            ],
            dirty: false, deleted: false, original_entries: Vec::new(), saved: true,
            display_field: None,
        };
        let out = gen_enum_class(&e, "com.game.config", "hero", false);
        // 注释里不能有裸换行，也不能出现会闭合 /** */ 的 */
        assert!(out.contains("/** 他说\"你好\"\\ 结束 * / 换行 第二行 */"), "got:\n{}", out);
        // 字面量里引号/反斜杠/换行都要转义
        assert!(out.contains(r#"Warrior(1, "他说\"你好\"\\ 结束 */ 换行\n第二行");"#), "got:\n{}", out);
    }

    #[test]
    fn table_field_carries_javadoc() {
        let t = super::super::test_fixtures::table("Item", vec![
            super::super::test_fixtures::field("stack_max", "堆叠上限", "int"),
        ]);
        let out = gen_table_tpl(&t, "com.game.config", "item", &no_enums(), false, false);
        assert!(out.contains("/** 堆叠上限 */"), "got:\n{}", out);
        // 数据文件 key 是源名 snake_case，Java 字段是 camelCase，反射加载器取不到，
        // 所以源名与字段名不一致时注解要带上 name（@01.8.1）
        assert!(out.contains(r#"@TblFieldType(value = "int", name = "stack_max")"#), "got:\n{}", out);
    }

    fn sample_table() -> Table {
        super::super::test_fixtures::table("Item", vec![
            super::super::test_fixtures::field("stack_max", "堆叠上限", "int"),
            super::super::test_fixtures::field("debug_mode", "调试开关", "bool"),
        ])
    }

    #[test]
    fn lombok_on_emits_annotation_and_no_explicit_getters() {
        let out = gen_table_tpl(&sample_table(), "com.game.config", "item", &no_enums(), true, false);
        assert!(out.contains("import lombok.Getter;"), "got:\n{}", out);
        assert!(out.contains("@Getter\npublic class ItemTpl"), "got:\n{}", out);
        // 显式 getter 全部让位给 Lombok
        assert!(!out.contains("public int getStackMax()"), "got:\n{}", out);
        assert!(!out.contains("public boolean getDebugMode()"), "got:\n{}", out);
    }

    #[test]
    fn lombok_on_keeps_get_id_override() {
        // ITpl 声明了 int getId()，TplHolder 注册表也在调；Lombok 见同名方法会跳过，
        // 所以这一行必须留着，且不能因为开了 Lombok 就消失
        let out = gen_table_tpl(&sample_table(), "com.game.config", "item", &no_enums(), true, false);
        assert!(out.contains("@Override\n    public int getId() { return id; }"), "got:\n{}", out);
        // 也不能出现第二份（Lombok 那边不会重复生成，但生成器自己别写两遍）
        assert_eq!(out.matches("public int getId()").count(), 1, "got:\n{}", out);
    }

    #[test]
    fn lombok_off_keeps_explicit_getters_and_no_import() {
        let out = gen_table_tpl(&sample_table(), "com.game.config", "item", &no_enums(), false, false);
        assert!(!out.contains("lombok"), "got:\n{}", out);
        assert!(out.contains("public int getStackMax() { return stackMax; }"), "got:\n{}", out);
        // 关闭时常量/布尔字段仍是 getXxx()，与开启 Lombok 后的 isXxx() 不同——这是有意的，
        // 差异写在 @11.3.4
        assert!(out.contains("public boolean getDebugMode() { return debugMode; }"), "got:\n{}", out);
    }

    #[test]
    fn constant_lombok_toggle_matches_table() {
        let c = super::super::test_fixtures::constant("GameConst", vec![
            super::super::test_fixtures::entry("version", "版本号", "str"),
        ]);
        let on = gen_constant_tpl(&c, "com.game.config", "global", &no_enums(), true, false);
        assert!(on.contains("@Getter\npublic class GameConstTpl"), "got:\n{}", on);
        assert!(!on.contains("public String getVersion()"), "got:\n{}", on);

        let off = gen_constant_tpl(&c, "com.game.config", "global", &no_enums(), false, false);
        assert!(!off.contains("lombok"), "got:\n{}", off);
        assert!(off.contains("public String getVersion() { return version; }"), "got:\n{}", off);
    }

    #[test]
    fn annotation_name_only_when_source_differs_from_field() {
        // `id` 两边同名 → 注解保持短形式，别把每行都写长
        assert_eq!(field_type_ann("int", "id", "id"), r#""int""#);
        // 源名 snake_case ≠ Java 字段 camelCase → 带上源名当查表 key
        assert_eq!(field_type_ann("int", "stack_max", "stackMax"),
            r#"value = "int", name = "stack_max""#);
    }

    #[test]
    fn top_level_ref_unchanged() {
        // 顶层 table ref → int；enum ref → XxxEnum（不受嵌套 ref 改动影响）
        assert_eq!(java_field_type("@Hero", &no_enums()), "int");
        let mut e = HashSet::new();
        e.insert("HeroType".to_string());
        assert_eq!(java_field_type("@HeroType", &e), "HeroTypeEnum");
    }
}
