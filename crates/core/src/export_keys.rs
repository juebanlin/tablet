//! 导出配置的可编辑叶子：一个 key 空间，显示 / 写入 / 比对 / 还原四件事共用。
//!
//! **为什么要有它。** UI 上这些字段原来是「Slint 发裸字符串、Rust 用手写 match 接」，
//! 两边各写各的拼写，编译器管不着。已经漏了三个 —— `csdn_custom_regions` /
//! `csu_custom_regions` / `csg_custom_regions` 发出来没有任何 arm 接住，被
//! `_ => {}` 吞掉，于是三个 C#「自定义区域」勾选框点了自己弹回去。收成一个枚举之后
//! 拼写有了唯一真理源，`from_str` 认不出来就是肉眼可见的错。
//!
//! **比对与还原都落在 `Option` 上，不走字符串往返。** 这里有十几个叶子带非空默认
//! （`lombok` / `custom_regions` 是 `unwrap_or(true)`，五个枚举是 `unwrap_or(default)`），
//! `get() -> String` 抹掉了 `None` 与 `Some(默认值)` 的区别。若还原写成
//! `set(live, get(original))`，原值是 `None` 时会被写成 `Some(默认值)` —— 项目仍然脏，
//! 却没有哪个字段是「改过」的。所以 `modified` 先比「设没设」再比值，`revert` 直接复制
//! `Option`。见 @02.26 那套「脏是算出来的」。

use crate::config_leaf::{bool_str, opt_string, set_bool, set_enum};
use crate::model::ExportConfig;

/// 导出配置里一个可编辑的叶子。`as_str()` 就是 UI / `--set` / 测试共同认的那个 key。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExportKey {
    Encoding,
    LineEnding,
    JsonEmptyAs,
    XmlEmptyAs,
    CppJsonLib,
    ServerDataOutput,
    JavaPackage,
    JavaCodeOutput,
    JavaLombok,
    JavaCustomRegions,
    GoPackage,
    GoCodeOutput,
    GoCustomRegions,
    CppNamespace,
    CppCodeOutput,
    CppCustomRegions,
    CsharpDotnetNamespace,
    CsharpDotnetCodeOutput,
    CsharpDotnetCustomRegions,
    TypescriptServerCodeOutput,
    LuaOutput,
    GdscriptOutput,
    TypescriptClientOutput,
    CsharpUnityNamespace,
    CsharpUnityCodeOutput,
    CsharpUnityCustomRegions,
    CsharpGodotNamespace,
    CsharpGodotCodeOutput,
    CsharpGodotCustomRegions,
}

impl ExportKey {
    /// 全部 29 项。顺序 = UI 上从上到下的顺序（导出 tab 的表单顺序）。
    pub const ALL: [ExportKey; 29] = [
        ExportKey::Encoding,
        ExportKey::LineEnding,
        ExportKey::JsonEmptyAs,
        ExportKey::XmlEmptyAs,
        ExportKey::CppJsonLib,
        ExportKey::ServerDataOutput,
        ExportKey::JavaPackage,
        ExportKey::JavaCodeOutput,
        ExportKey::JavaLombok,
        ExportKey::JavaCustomRegions,
        ExportKey::GoPackage,
        ExportKey::GoCodeOutput,
        ExportKey::GoCustomRegions,
        ExportKey::CppNamespace,
        ExportKey::CppCodeOutput,
        ExportKey::CppCustomRegions,
        ExportKey::CsharpDotnetNamespace,
        ExportKey::CsharpDotnetCodeOutput,
        ExportKey::CsharpDotnetCustomRegions,
        ExportKey::TypescriptServerCodeOutput,
        ExportKey::LuaOutput,
        ExportKey::GdscriptOutput,
        ExportKey::TypescriptClientOutput,
        ExportKey::CsharpUnityNamespace,
        ExportKey::CsharpUnityCodeOutput,
        ExportKey::CsharpUnityCustomRegions,
        ExportKey::CsharpGodotNamespace,
        ExportKey::CsharpGodotCodeOutput,
        ExportKey::CsharpGodotCustomRegions,
    ];

    /// snake_case key。UI 的 `export-field-edited(key, …)` 与 `modified_field` 都用它。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Encoding => "encoding",
            Self::LineEnding => "line_ending",
            Self::JsonEmptyAs => "json_empty_as",
            Self::XmlEmptyAs => "xml_empty_as",
            Self::CppJsonLib => "cpp_json_lib",
            Self::ServerDataOutput => "server_data_output",
            Self::JavaPackage => "java_package",
            Self::JavaCodeOutput => "java_code_output",
            Self::JavaLombok => "java_lombok",
            Self::JavaCustomRegions => "java_custom_regions",
            Self::GoPackage => "go_package",
            Self::GoCodeOutput => "go_code_output",
            Self::GoCustomRegions => "go_custom_regions",
            Self::CppNamespace => "cpp_namespace",
            Self::CppCodeOutput => "cpp_code_output",
            Self::CppCustomRegions => "cpp_custom_regions",
            Self::CsharpDotnetNamespace => "csharp_dotnet_namespace",
            Self::CsharpDotnetCodeOutput => "csharp_dotnet_code_output",
            Self::CsharpDotnetCustomRegions => "csharp_dotnet_custom_regions",
            Self::TypescriptServerCodeOutput => "typescript_server_code_output",
            Self::LuaOutput => "lua_output",
            Self::GdscriptOutput => "gdscript_output",
            Self::TypescriptClientOutput => "typescript_client_output",
            Self::CsharpUnityNamespace => "csharp_unity_namespace",
            Self::CsharpUnityCodeOutput => "csharp_unity_code_output",
            Self::CsharpUnityCustomRegions => "csharp_unity_custom_regions",
            Self::CsharpGodotNamespace => "csharp_godot_namespace",
            Self::CsharpGodotCodeOutput => "csharp_godot_code_output",
            Self::CsharpGodotCustomRegions => "csharp_godot_custom_regions",
        }
    }

    /// 反查。未知 key = None —— **调用方必须把 None 当错误处理**，别再 `_ => {}` 吞掉。
    pub fn from_str(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.as_str() == s)
    }

    /// 显示值：**和 UI 上看到的那一格完全一致**（含各处 `unwrap_or(默认)` 兜底）。
    /// 只用于渲染，不用于比对 —— 比对见 [`modified`](Self::modified)。
    pub fn get(self, cfg: &ExportConfig) -> String {
        match self {
            Self::Encoding => cfg.encoding.unwrap_or_default().as_str().to_string(),
            Self::LineEnding => cfg.line_ending.unwrap_or_default().as_str().to_string(),
            Self::JsonEmptyAs => {
                cfg.json.as_ref().and_then(|j| j.empty_as).unwrap_or_default().as_str().to_string()
            }
            Self::XmlEmptyAs => {
                cfg.xml.as_ref().and_then(|x| x.empty_as).unwrap_or_default().as_str().to_string()
            }
            Self::CppJsonLib => cfg
                .server
                .as_ref()
                .and_then(|s| s.cpp.as_ref())
                .and_then(|c| c.json_lib)
                .unwrap_or_default()
                .as_str()
                .to_string(),
            Self::ServerDataOutput => {
                cfg.server.as_ref().and_then(|s| s.data_output.clone()).unwrap_or_default()
            }
            Self::JavaPackage => {
                cfg.server.as_ref().and_then(|s| s.java.as_ref()).and_then(|j| j.package.clone()).unwrap_or_default()
            }
            Self::JavaCodeOutput => {
                cfg.server.as_ref().and_then(|s| s.java.as_ref()).and_then(|j| j.code_output.clone()).unwrap_or_default()
            }
            // 缺省开：与 export 侧各处 `unwrap_or(true)` 同一口径（@11.3.4）
            Self::JavaLombok => {
                bool_str(cfg.server.as_ref().and_then(|s| s.java.as_ref()).and_then(|j| j.lombok).unwrap_or(true))
            }
            Self::JavaCustomRegions => bool_str(
                cfg.server.as_ref().and_then(|s| s.java.as_ref()).and_then(|j| j.custom_regions).unwrap_or(true),
            ),
            Self::GoPackage => {
                cfg.server.as_ref().and_then(|s| s.go.as_ref()).and_then(|g| g.package.clone()).unwrap_or_default()
            }
            Self::GoCodeOutput => {
                cfg.server.as_ref().and_then(|s| s.go.as_ref()).and_then(|g| g.code_output.clone()).unwrap_or_default()
            }
            Self::GoCustomRegions => bool_str(
                cfg.server.as_ref().and_then(|s| s.go.as_ref()).and_then(|g| g.custom_regions).unwrap_or(true),
            ),
            Self::CppNamespace => {
                cfg.server.as_ref().and_then(|s| s.cpp.as_ref()).and_then(|c| c.namespace.clone()).unwrap_or_default()
            }
            Self::CppCodeOutput => {
                cfg.server.as_ref().and_then(|s| s.cpp.as_ref()).and_then(|c| c.code_output.clone()).unwrap_or_default()
            }
            Self::CppCustomRegions => bool_str(
                cfg.server.as_ref().and_then(|s| s.cpp.as_ref()).and_then(|c| c.custom_regions).unwrap_or(true),
            ),
            Self::CsharpDotnetNamespace => cfg
                .server
                .as_ref()
                .and_then(|s| s.csharp_dotnet.as_ref())
                .and_then(|c| c.namespace.clone())
                .unwrap_or_default(),
            Self::CsharpDotnetCodeOutput => cfg
                .server
                .as_ref()
                .and_then(|s| s.csharp_dotnet.as_ref())
                .and_then(|c| c.code_output.clone())
                .unwrap_or_default(),
            Self::CsharpDotnetCustomRegions => bool_str(
                cfg.server
                    .as_ref()
                    .and_then(|s| s.csharp_dotnet.as_ref())
                    .and_then(|c| c.custom_regions)
                    .unwrap_or(true),
            ),
            Self::TypescriptServerCodeOutput => cfg
                .server
                .as_ref()
                .and_then(|s| s.typescript.as_ref())
                .and_then(|t| t.output.clone())
                .unwrap_or_default(),
            Self::LuaOutput => {
                cfg.client.as_ref().and_then(|c| c.lua.as_ref()).and_then(|l| l.output.clone()).unwrap_or_default()
            }
            Self::GdscriptOutput => {
                cfg.client.as_ref().and_then(|c| c.gdscript.as_ref()).and_then(|g| g.output.clone()).unwrap_or_default()
            }
            Self::TypescriptClientOutput => cfg
                .client
                .as_ref()
                .and_then(|c| c.typescript.as_ref())
                .and_then(|t| t.output.clone())
                .unwrap_or_default(),
            Self::CsharpUnityNamespace => cfg
                .client
                .as_ref()
                .and_then(|c| c.csharp_unity.as_ref())
                .and_then(|u| u.namespace.clone())
                .unwrap_or_default(),
            Self::CsharpUnityCodeOutput => cfg
                .client
                .as_ref()
                .and_then(|c| c.csharp_unity.as_ref())
                .and_then(|u| u.code_output.clone())
                .unwrap_or_default(),
            Self::CsharpUnityCustomRegions => bool_str(
                cfg.client
                    .as_ref()
                    .and_then(|c| c.csharp_unity.as_ref())
                    .and_then(|u| u.custom_regions)
                    .unwrap_or(true),
            ),
            Self::CsharpGodotNamespace => cfg
                .client
                .as_ref()
                .and_then(|c| c.csharp_godot.as_ref())
                .and_then(|g| g.namespace.clone())
                .unwrap_or_default(),
            Self::CsharpGodotCodeOutput => cfg
                .client
                .as_ref()
                .and_then(|c| c.csharp_godot.as_ref())
                .and_then(|g| g.code_output.clone())
                .unwrap_or_default(),
            Self::CsharpGodotCustomRegions => bool_str(
                cfg.client
                    .as_ref()
                    .and_then(|c| c.csharp_godot.as_ref())
                    .and_then(|g| g.custom_regions)
                    .unwrap_or(true),
            ),
        }
    }

    /// 这个叶子和 `orig` 一样吗。
    ///
    /// **先比「设没设」，再比值** —— 只比 `get()` 的字符串会把 `None` 与
    /// `Some(默认值)` 判成相等，于是「改过但看不出改了什么」就成立了。
    pub fn modified(self, live: &ExportConfig, orig: &ExportConfig) -> bool {
        if self.is_set(live) != self.is_set(orig) {
            return true;
        }
        self.get(live) != self.get(orig)
    }

    /// 这个叶子的 `Option` 是不是 `Some`（即项目有没有显式设它）。
    fn is_set(self, cfg: &ExportConfig) -> bool {
        match self {
            Self::Encoding => cfg.encoding.is_some(),
            Self::LineEnding => cfg.line_ending.is_some(),
            Self::JsonEmptyAs => cfg.json.as_ref().is_some_and(|j| j.empty_as.is_some()),
            Self::XmlEmptyAs => cfg.xml.as_ref().is_some_and(|x| x.empty_as.is_some()),
            Self::CppJsonLib => {
                cfg.server.as_ref().and_then(|s| s.cpp.as_ref()).is_some_and(|c| c.json_lib.is_some())
            }
            Self::ServerDataOutput => cfg.server.as_ref().is_some_and(|s| s.data_output.is_some()),
            Self::JavaPackage => java(cfg).is_some_and(|j| j.package.is_some()),
            Self::JavaCodeOutput => java(cfg).is_some_and(|j| j.code_output.is_some()),
            Self::JavaLombok => java(cfg).is_some_and(|j| j.lombok.is_some()),
            Self::JavaCustomRegions => java(cfg).is_some_and(|j| j.custom_regions.is_some()),
            Self::GoPackage => go(cfg).is_some_and(|g| g.package.is_some()),
            Self::GoCodeOutput => go(cfg).is_some_and(|g| g.code_output.is_some()),
            Self::GoCustomRegions => go(cfg).is_some_and(|g| g.custom_regions.is_some()),
            Self::CppNamespace => cpp(cfg).is_some_and(|c| c.namespace.is_some()),
            Self::CppCodeOutput => cpp(cfg).is_some_and(|c| c.code_output.is_some()),
            Self::CppCustomRegions => cpp(cfg).is_some_and(|c| c.custom_regions.is_some()),
            Self::CsharpDotnetNamespace => dotnet(cfg).is_some_and(|c| c.namespace.is_some()),
            Self::CsharpDotnetCodeOutput => dotnet(cfg).is_some_and(|c| c.code_output.is_some()),
            Self::CsharpDotnetCustomRegions => dotnet(cfg).is_some_and(|c| c.custom_regions.is_some()),
            Self::TypescriptServerCodeOutput => {
                cfg.server.as_ref().and_then(|s| s.typescript.as_ref()).is_some_and(|t| t.output.is_some())
            }
            Self::LuaOutput => cfg.client.as_ref().and_then(|c| c.lua.as_ref()).is_some_and(|l| l.output.is_some()),
            Self::GdscriptOutput => {
                cfg.client.as_ref().and_then(|c| c.gdscript.as_ref()).is_some_and(|g| g.output.is_some())
            }
            Self::TypescriptClientOutput => {
                cfg.client.as_ref().and_then(|c| c.typescript.as_ref()).is_some_and(|t| t.output.is_some())
            }
            Self::CsharpUnityNamespace => unity(cfg).is_some_and(|u| u.namespace.is_some()),
            Self::CsharpUnityCodeOutput => unity(cfg).is_some_and(|u| u.code_output.is_some()),
            Self::CsharpUnityCustomRegions => unity(cfg).is_some_and(|u| u.custom_regions.is_some()),
            Self::CsharpGodotNamespace => godot(cfg).is_some_and(|g| g.namespace.is_some()),
            Self::CsharpGodotCodeOutput => godot(cfg).is_some_and(|g| g.code_output.is_some()),
            Self::CsharpGodotCustomRegions => godot(cfg).is_some_and(|g| g.custom_regions.is_some()),
        }
    }

    /// 写入人给的值。返回是否接受 —— **非法值保持原样并返回 false**，
    /// 由调用方去吵（今天 `apply_export_field` 是静默不动，改了坏值看不出任何反应）。
    ///
    /// 字符串叶子：空串 → `None`（语义是「不覆盖，用全局的值」，见 @02.26.3）。
    pub fn set(self, cfg: &mut ExportConfig, v: &str) -> bool {
        match self {
            Self::Encoding => set_enum(&mut cfg.encoding, v),
            Self::LineEnding => set_enum(&mut cfg.line_ending, v),
            Self::JsonEmptyAs => set_enum(&mut cfg.json.get_or_insert_default().empty_as, v),
            Self::XmlEmptyAs => set_enum(&mut cfg.xml.get_or_insert_default().empty_as, v),
            Self::CppJsonLib => {
                set_enum(&mut cfg.server.get_or_insert_default().cpp.get_or_insert_default().json_lib, v)
            }
            Self::ServerDataOutput => {
                cfg.server.get_or_insert_default().data_output = opt_string(v);
                true
            }
            Self::JavaPackage => {
                cfg.server.get_or_insert_default().java.get_or_insert_default().package = opt_string(v);
                true
            }
            Self::JavaCodeOutput => {
                cfg.server.get_or_insert_default().java.get_or_insert_default().code_output = opt_string(v);
                true
            }
            Self::JavaLombok => {
                set_bool(&mut cfg.server.get_or_insert_default().java.get_or_insert_default().lombok, v)
            }
            Self::JavaCustomRegions => {
                set_bool(&mut cfg.server.get_or_insert_default().java.get_or_insert_default().custom_regions, v)
            }
            Self::GoPackage => {
                cfg.server.get_or_insert_default().go.get_or_insert_default().package = opt_string(v);
                true
            }
            Self::GoCodeOutput => {
                cfg.server.get_or_insert_default().go.get_or_insert_default().code_output = opt_string(v);
                true
            }
            Self::GoCustomRegions => {
                set_bool(&mut cfg.server.get_or_insert_default().go.get_or_insert_default().custom_regions, v)
            }
            Self::CppNamespace => {
                cfg.server.get_or_insert_default().cpp.get_or_insert_default().namespace = opt_string(v);
                true
            }
            Self::CppCodeOutput => {
                cfg.server.get_or_insert_default().cpp.get_or_insert_default().code_output = opt_string(v);
                true
            }
            Self::CppCustomRegions => {
                set_bool(&mut cfg.server.get_or_insert_default().cpp.get_or_insert_default().custom_regions, v)
            }
            Self::CsharpDotnetNamespace => {
                cfg.server.get_or_insert_default().csharp_dotnet.get_or_insert_default().namespace = opt_string(v);
                true
            }
            Self::CsharpDotnetCodeOutput => {
                cfg.server.get_or_insert_default().csharp_dotnet.get_or_insert_default().code_output = opt_string(v);
                true
            }
            Self::CsharpDotnetCustomRegions => set_bool(
                &mut cfg.server.get_or_insert_default().csharp_dotnet.get_or_insert_default().custom_regions,
                v,
            ),
            Self::TypescriptServerCodeOutput => {
                cfg.server.get_or_insert_default().typescript.get_or_insert_default().output = opt_string(v);
                true
            }
            Self::LuaOutput => {
                cfg.client.get_or_insert_default().lua.get_or_insert_default().output = opt_string(v);
                true
            }
            Self::GdscriptOutput => {
                cfg.client.get_or_insert_default().gdscript.get_or_insert_default().output = opt_string(v);
                true
            }
            Self::TypescriptClientOutput => {
                cfg.client.get_or_insert_default().typescript.get_or_insert_default().output = opt_string(v);
                true
            }
            Self::CsharpUnityNamespace => {
                cfg.client.get_or_insert_default().csharp_unity.get_or_insert_default().namespace = opt_string(v);
                true
            }
            Self::CsharpUnityCodeOutput => {
                cfg.client.get_or_insert_default().csharp_unity.get_or_insert_default().code_output = opt_string(v);
                true
            }
            Self::CsharpUnityCustomRegions => set_bool(
                &mut cfg.client.get_or_insert_default().csharp_unity.get_or_insert_default().custom_regions,
                v,
            ),
            Self::CsharpGodotNamespace => {
                cfg.client.get_or_insert_default().csharp_godot.get_or_insert_default().namespace = opt_string(v);
                true
            }
            Self::CsharpGodotCodeOutput => {
                cfg.client.get_or_insert_default().csharp_godot.get_or_insert_default().code_output = opt_string(v);
                true
            }
            Self::CsharpGodotCustomRegions => set_bool(
                &mut cfg.client.get_or_insert_default().csharp_godot.get_or_insert_default().custom_regions,
                v,
            ),
        }
    }

    /// 把这一个叶子还原成 `orig` 的样子。没改过就什么都不做。
    ///
    /// 「先把祖先链裁剪成 `orig` 的形状、再复制叶子」——`orig` 某个祖先为 `None`
    /// 蕴含它底下**每个**叶子都是 `None`，所以整条撤掉是安全的。不这么做的话，
    /// `set` 为了写叶子而造出来的祖先 `Some(默认值)` 会留着，项目仍然脏却没有哪个
    /// 字段是「改过」的。
    pub fn revert(self, live: &mut ExportConfig, orig: &ExportConfig) {
        if !self.modified(live, orig) {
            return;
        }
        match self {
            Self::Encoding => live.encoding = orig.encoding,
            Self::LineEnding => live.line_ending = orig.line_ending,
            // 这两个子树只有一个字段，整块换掉就是精确还原
            Self::JsonEmptyAs => live.json = orig.json.clone(),
            Self::XmlEmptyAs => live.xml = orig.xml.clone(),
            Self::CppJsonLib => {
                restore(&mut live.server, &orig.server, |l, o| {
                    restore(&mut l.cpp, &o.cpp, |l, o| l.json_lib = o.json_lib)
                });
            }
            Self::ServerDataOutput => {
                restore(&mut live.server, &orig.server, |l, o| l.data_output = o.data_output.clone());
            }
            Self::JavaPackage => {
                restore(&mut live.server, &orig.server, |l, o| {
                    restore(&mut l.java, &o.java, |l, o| l.package = o.package.clone())
                });
            }
            Self::JavaCodeOutput => {
                restore(&mut live.server, &orig.server, |l, o| {
                    restore(&mut l.java, &o.java, |l, o| l.code_output = o.code_output.clone())
                });
            }
            Self::JavaLombok => {
                restore(&mut live.server, &orig.server, |l, o| {
                    restore(&mut l.java, &o.java, |l, o| l.lombok = o.lombok)
                });
            }
            Self::JavaCustomRegions => {
                restore(&mut live.server, &orig.server, |l, o| {
                    restore(&mut l.java, &o.java, |l, o| l.custom_regions = o.custom_regions)
                });
            }
            Self::GoPackage => {
                restore(&mut live.server, &orig.server, |l, o| {
                    restore(&mut l.go, &o.go, |l, o| l.package = o.package.clone())
                });
            }
            Self::GoCodeOutput => {
                restore(&mut live.server, &orig.server, |l, o| {
                    restore(&mut l.go, &o.go, |l, o| l.code_output = o.code_output.clone())
                });
            }
            Self::GoCustomRegions => {
                restore(&mut live.server, &orig.server, |l, o| {
                    restore(&mut l.go, &o.go, |l, o| l.custom_regions = o.custom_regions)
                });
            }
            Self::CppNamespace => {
                restore(&mut live.server, &orig.server, |l, o| {
                    restore(&mut l.cpp, &o.cpp, |l, o| l.namespace = o.namespace.clone())
                });
            }
            Self::CppCodeOutput => {
                restore(&mut live.server, &orig.server, |l, o| {
                    restore(&mut l.cpp, &o.cpp, |l, o| l.code_output = o.code_output.clone())
                });
            }
            Self::CppCustomRegions => {
                restore(&mut live.server, &orig.server, |l, o| {
                    restore(&mut l.cpp, &o.cpp, |l, o| l.custom_regions = o.custom_regions)
                });
            }
            Self::CsharpDotnetNamespace => {
                restore(&mut live.server, &orig.server, |l, o| {
                    restore(&mut l.csharp_dotnet, &o.csharp_dotnet, |l, o| l.namespace = o.namespace.clone())
                });
            }
            Self::CsharpDotnetCodeOutput => {
                restore(&mut live.server, &orig.server, |l, o| {
                    restore(&mut l.csharp_dotnet, &o.csharp_dotnet, |l, o| l.code_output = o.code_output.clone())
                });
            }
            Self::CsharpDotnetCustomRegions => {
                restore(&mut live.server, &orig.server, |l, o| {
                    restore(&mut l.csharp_dotnet, &o.csharp_dotnet, |l, o| l.custom_regions = o.custom_regions)
                });
            }
            Self::TypescriptServerCodeOutput => {
                restore(&mut live.server, &orig.server, |l, o| {
                    restore(&mut l.typescript, &o.typescript, |l, o| l.output = o.output.clone())
                });
            }
            Self::LuaOutput => {
                restore(&mut live.client, &orig.client, |l, o| {
                    restore(&mut l.lua, &o.lua, |l, o| l.output = o.output.clone())
                });
            }
            Self::GdscriptOutput => {
                restore(&mut live.client, &orig.client, |l, o| {
                    restore(&mut l.gdscript, &o.gdscript, |l, o| l.output = o.output.clone())
                });
            }
            Self::TypescriptClientOutput => {
                restore(&mut live.client, &orig.client, |l, o| {
                    restore(&mut l.typescript, &o.typescript, |l, o| l.output = o.output.clone())
                });
            }
            Self::CsharpUnityNamespace => {
                restore(&mut live.client, &orig.client, |l, o| {
                    restore(&mut l.csharp_unity, &o.csharp_unity, |l, o| l.namespace = o.namespace.clone())
                });
            }
            Self::CsharpUnityCodeOutput => {
                restore(&mut live.client, &orig.client, |l, o| {
                    restore(&mut l.csharp_unity, &o.csharp_unity, |l, o| l.code_output = o.code_output.clone())
                });
            }
            Self::CsharpUnityCustomRegions => {
                restore(&mut live.client, &orig.client, |l, o| {
                    restore(&mut l.csharp_unity, &o.csharp_unity, |l, o| l.custom_regions = o.custom_regions)
                });
            }
            Self::CsharpGodotNamespace => {
                restore(&mut live.client, &orig.client, |l, o| {
                    restore(&mut l.csharp_godot, &o.csharp_godot, |l, o| l.namespace = o.namespace.clone())
                });
            }
            Self::CsharpGodotCodeOutput => {
                restore(&mut live.client, &orig.client, |l, o| {
                    restore(&mut l.csharp_godot, &o.csharp_godot, |l, o| l.code_output = o.code_output.clone())
                });
            }
            Self::CsharpGodotCustomRegions => {
                restore(&mut live.client, &orig.client, |l, o| {
                    restore(&mut l.csharp_godot, &o.csharp_godot, |l, o| l.custom_regions = o.custom_regions)
                });
            }
        }
    }
}

// ─── 取值用的小访问器（只是把上面那些 `and_then` 链缩短） ───────────────

fn java(c: &ExportConfig) -> Option<&crate::model::JavaExport> {
    c.server.as_ref().and_then(|s| s.java.as_ref())
}
fn go(c: &ExportConfig) -> Option<&crate::model::GoExport> {
    c.server.as_ref().and_then(|s| s.go.as_ref())
}
fn cpp(c: &ExportConfig) -> Option<&crate::model::CppExport> {
    c.server.as_ref().and_then(|s| s.cpp.as_ref())
}
fn dotnet(c: &ExportConfig) -> Option<&crate::model::DotNetExport> {
    c.server.as_ref().and_then(|s| s.csharp_dotnet.as_ref())
}
fn unity(c: &ExportConfig) -> Option<&crate::model::UnityCSharpExport> {
    c.client.as_ref().and_then(|c| c.csharp_unity.as_ref())
}
fn godot(c: &ExportConfig) -> Option<&crate::model::GodotCSharpExport> {
    c.client.as_ref().and_then(|c| c.csharp_godot.as_ref())
}

/// 把 `live` 这个 `Option` 裁剪成 `orig` 的形状，再对两者都有的那层动手。
///
/// `orig` 为 `None` ⇒ 整条链撤掉（安全：`orig` 的 `None` 蕴含叶子也是 `None`）。
fn restore<T: Clone>(live: &mut Option<T>, orig: &Option<T>, edit: impl FnOnce(&mut T, &T)) {
    match (live.as_mut(), orig.as_ref()) {
        (_, None) => *live = None,
        (Some(l), Some(o)) => edit(l, o),
        // live 连这一层都没有，说明整棵子树都不在：整块换成 orig 的即可
        (None, Some(o)) => *live = Some(o.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// 每个 key 一个**和默认值不同**的合法值。枚举叶子从 `all_str()` 里挑第一个不同的，
    /// 免得写死变体名（变体改了这里不会跟着漂）。
    fn sentinel(k: ExportKey) -> String {
        use crate::enums::{CppJsonLib, Encoding, JsonEmptyAs, LineEnding, XmlEmptyAs};

        fn pick(all: &[&'static str], cur: &str) -> String {
            all.iter().find(|s| **s != cur).expect("枚举至少要有两个选项").to_string()
        }

        match k {
            ExportKey::Encoding => pick(&Encoding::all_str(), Encoding::default().as_str()),
            ExportKey::LineEnding => pick(&LineEnding::all_str(), LineEnding::default().as_str()),
            ExportKey::JsonEmptyAs => pick(&JsonEmptyAs::all_str(), JsonEmptyAs::default().as_str()),
            ExportKey::XmlEmptyAs => pick(&XmlEmptyAs::all_str(), XmlEmptyAs::default().as_str()),
            ExportKey::CppJsonLib => pick(&CppJsonLib::all_str(), CppJsonLib::default().as_str()),
            // 布尔叶子默认是 true（各处 unwrap_or(true)），反着来
            ExportKey::JavaLombok
            | ExportKey::JavaCustomRegions
            | ExportKey::GoCustomRegions
            | ExportKey::CppCustomRegions
            | ExportKey::CsharpDotnetCustomRegions
            | ExportKey::CsharpUnityCustomRegions
            | ExportKey::CsharpGodotCustomRegions => "false".to_string(),
            // 其余全是字符串叶子，默认值都非空
            _ => "SENTINEL".to_string(),
        }
    }

    #[test]
    fn all_count_matches_the_settings_form() {
        assert_eq!(ExportKey::ALL.len(), 29);
    }

    #[test]
    fn keys_are_unique_and_round_trip() {
        let mut seen = BTreeSet::new();
        for k in ExportKey::ALL {
            assert!(seen.insert(k.as_str()), "重复的 key: {}", k.as_str());
            assert_eq!(ExportKey::from_str(k.as_str()), Some(k), "{} 反查不回来", k.as_str());
        }
        // 那两个曾经漏网的拼写现在必须**认不出来** —— 它们不是合法的 key
        assert_eq!(ExportKey::from_str("csdn_custom_regions"), None);
        assert_eq!(ExportKey::from_str("csu_custom_regions"), None);
        assert_eq!(ExportKey::from_str("csg_custom_regions"), None);
        assert_eq!(ExportKey::from_str("bogus"), None);
    }

    /// 每个 key 必须真的落到**自己那一格**上：写一个哨兵值，只有它自己算改过。
    /// 这条钉住 `get` / `set` / `modified` 三者认的是同一条路径。
    #[test]
    fn each_key_writes_only_its_own_leaf() {
        for k in ExportKey::ALL {
            let orig = ExportConfig::default();
            let mut live = orig.clone();
            assert!(!k.modified(&live, &orig), "{} 起点就该是干净的", k.as_str());

            let v = sentinel(k);
            assert!(k.set(&mut live, v.as_str()), "{} 拒绝了哨兵值 {:?}", k.as_str(), v);
            assert_eq!(k.get(&live), v, "{} 写进去又读不回来", k.as_str());
            assert!(k.modified(&live, &orig), "{} 改了却不算脏", k.as_str());

            for other in ExportKey::ALL {
                if other == k {
                    continue;
                }
                assert!(
                    !other.modified(&live, &orig),
                    "改 {} 却连带弄脏了 {}",
                    k.as_str(),
                    other.as_str()
                );
            }
        }
    }

    /// 三个 C# 的 key 曾经和 Rust 的 match 拼写不一致，被 `_ => {}` 吞掉，
    /// 勾选框点了自己弹回去。这条钉住它们真的落在那三格上。
    #[test]
    fn the_three_csharp_custom_region_keys_actually_land() {
        for k in [
            ExportKey::CsharpDotnetCustomRegions,
            ExportKey::CsharpUnityCustomRegions,
            ExportKey::CsharpGodotCustomRegions,
        ] {
            let orig = ExportConfig::default();
            let mut live = orig.clone();
            assert!(k.set(&mut live, "false"), "{} 被拒了", k.as_str());
            assert_eq!(k.get(&live), "false", "{} 没写进去", k.as_str());
            assert!(k.modified(&live, &orig), "{} 改了却不算脏", k.as_str());
        }
    }

    /// 原值是 `None`、现值是 `Some(默认值)` —— 这正是字符串往返会搞错的那种：
    /// `set(live, get(orig))` 会写回 `Some(默认值)`，于是项目仍然脏、却没有哪个字段是蓝的。
    #[test]
    fn revert_clears_a_leaf_that_was_unset_on_disk() {
        // 盘上没有 `[export.server]` 整段（手写的 tablet.toml 可能长这样）
        let orig = ExportConfig { server: None, ..ExportConfig::default() };
        let mut live = orig.clone();

        // 用户动了一下 go_package：set 为了写叶子会把 server 造出来
        assert!(ExportKey::GoPackage.set(&mut live, "x"));
        assert_ne!(live, orig, "改完确实和盘上不一样了");
        assert!(ExportKey::GoPackage.modified(&live, &orig));

        ExportKey::GoPackage.revert(&mut live, &orig);
        assert_eq!(live, orig, "还原后必须和盘上完全一致");
        assert!(!ExportKey::GoPackage.modified(&live, &orig));
    }

    /// 布尔叶子同理：默认是 true，盘上是 `None`，用户关掉再还原要回到 `None` 而不是 `Some(true)`。
    #[test]
    fn revert_puts_a_defaulted_bool_back_to_unset() {
        let orig = ExportConfig { server: None, ..ExportConfig::default() };
        let mut live = orig.clone();

        assert!(ExportKey::JavaLombok.set(&mut live, "false"));
        assert!(ExportKey::JavaLombok.modified(&live, &orig));

        ExportKey::JavaLombok.revert(&mut live, &orig);
        assert_eq!(live, orig, "还原后必须和盘上完全一致");
    }

    /// `modified` 必须能分辨「盘上没设」和「设成了默认值」。
    ///
    /// 场景：盘上 `[export.server.java]` 段在，但没有 `lombok` 这个键（`None`）；
    /// 用户把「缺省开」的开关关掉再打开 —— `Option` 从 `None` 变成 `Some(true)`，
    /// 而**显示值一直是 "true"、前后一模一样**。此时 `project.toml` 确实会多出一个键、
    /// 项目确实脏了；若 `modified` 只比渲染出来的字符串，就会报「没改过」，
    /// 于是出现「项目脏了、却没有任何字段是蓝的」。
    #[test]
    fn modified_sees_a_leaf_reset_to_its_default_value() {
        let mut orig = ExportConfig::default();
        orig.server.as_mut().unwrap().java.as_mut().unwrap().lombok = None;

        let mut live = orig.clone();
        assert!(!ExportKey::JavaLombok.modified(&live, &orig), "起点该是干净的");

        assert!(ExportKey::JavaLombok.set(&mut live, "false"));
        assert!(ExportKey::JavaLombok.set(&mut live, "true"));

        assert_eq!(
            ExportKey::JavaLombok.get(&live),
            ExportKey::JavaLombok.get(&orig),
            "显示值确实一模一样"
        );
        assert_ne!(live, orig, "盘上确实不一样了");
        assert!(ExportKey::JavaLombok.modified(&live, &orig), "明明不一样，却报没改过");
    }

    /// 没改过的叶子还原时不该碰任何东西 —— 否则「还原 A」会顺手抹掉 B 的编辑。
    #[test]
    fn revert_of_an_untouched_leaf_does_nothing() {
        let orig = ExportConfig::default();
        let mut live = orig.clone();
        assert!(ExportKey::GoPackage.set(&mut live, "x"));
        let before = live.clone();

        ExportKey::JavaPackage.revert(&mut live, &orig); // 它没改过

        assert_eq!(live, before, "没改过的叶子还原时碰到了别的格子");
    }

    #[test]
    fn set_rejects_a_value_it_cannot_parse() {
        let mut cfg = ExportConfig::default();

        assert!(!ExportKey::Encoding.set(&mut cfg, "utf-99"), "非法编码不该被接受");
        assert_eq!(ExportKey::Encoding.get(&cfg), "utf-8", "拒绝之后要保持原样，不是清空");

        assert!(!ExportKey::JavaLombok.set(&mut cfg, "yes"), "布尔只认 true/false");
        assert_eq!(ExportKey::JavaLombok.get(&cfg), "true", "拒绝之后要保持原样");

        // 字符串叶子的空串 = 不设（语义是「不覆盖，用全局的值」，@02.26.3）
        assert!(ExportKey::JavaPackage.set(&mut cfg, ""));
        assert_eq!(ExportKey::JavaPackage.get(&cfg), "");
        assert!(!ExportKey::JavaPackage.is_set(&cfg), "空串应当写成 None");
    }
}
