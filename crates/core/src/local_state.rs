//! 工作区内的**用户习惯数据**（`.data/`）。
//!
//! 设计约束（和业务配置严格分开）：
//! - 位置：`<workdir>/.data/`，每个 checkout 一份；**不进 Git / SVN**；
//! - 根结构统一为 [`UserData`]，列宽等本机习惯、后续崩溃草稿 / 会话状态都挂在这一层；
//!   UI 做「清理用户数据」只面对一个入口；
//! - **连续撤销不在这里**：它是每个 Project 的内存 `EditHistory`，保存即清空（docs/12）；
//! - 读写不走 `save_all` / `persist_global_config` / dirty / 校验，由调用方在
//!   用户改变视图时独立调用 [`UserData::save`]；
//! - 读失败 / 坏文件 / 版本不匹配 → 空状态（视图偏好不是业务数据，不能卡启动）。
//!
//! 目录里同时放工作区锁（`.data/tablet.lock`）。`ensure_data_dir` 会顺手创建
//! `.data/.gitignore`（内容 `*`），防止用户误提交本机状态。

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DATA_DIR: &str = ".data";
pub const USER_DATA_FILE: &str = "user-data.json";
pub const LOCK_FILE: &str = "tablet.lock";
pub const USER_DATA_VERSION: u32 = 1;

/// 手动列宽的合法范围（自动列宽另有 60–480 的估算范围）。
pub const MIN_MANUAL_WIDTH: f32 = 40.0;
pub const MAX_MANUAL_WIDTH: f32 = 800.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Table,
    Constant,
    Enum,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::Table => "table",
            NodeKind::Constant => "constant",
            NodeKind::Enum => "enum",
        }
    }
}

/// 节点身份。**必须带 kind**：同一组里 Table 和 Enum 可以同名。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeKey {
    pub project: String,
    pub group: String,
    pub kind: NodeKind,
    pub node: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeViewState {
    #[serde(default = "default_true")]
    pub auto: bool,
    /// 列名 → 手动宽度（px）。自动模式下也保留，取消勾选时恢复。
    #[serde(default)]
    pub widths: BTreeMap<String, f32>,
}

impl Default for NodeViewState {
    fn default() -> Self {
        Self { auto: true, widths: BTreeMap::new() }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GridPreferences {
    /// 内存里用 map 方便按 `NodeKey` 查找；文件里序列化成结构化数组。
    #[serde(default, with = "nodes_serde")]
    pub nodes: BTreeMap<NodeKey, NodeViewState>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UiPreferences {
    #[serde(default)]
    pub grid: GridPreferences,
}

/// 预留：崩溃恢复草稿 / 会话历史（连续撤销是内存态，不写这里；见 docs/12）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HistoryState {}

/// 预留：启动 UI 锁 / 会话状态（当前锁仍在 `.data/tablet.lock`）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionState {}

/// 用户习惯数据的**唯一根结构**。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserData {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub ui: UiPreferences,
    #[serde(default)]
    pub history: HistoryState,
    #[serde(default)]
    pub session: SessionState,
}

impl Default for UserData {
    fn default() -> Self {
        Self {
            version: USER_DATA_VERSION,
            ui: UiPreferences::default(),
            history: HistoryState::default(),
            session: SessionState::default(),
        }
    }
}

impl UserData {
    pub fn load(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        let Ok(mut data) = serde_json::from_str::<Self>(&text) else {
            return Self::default();
        };
        // 视图偏好没有迁移价值；版本不对就当没有。
        if data.version != USER_DATA_VERSION {
            return Self::default();
        }
        data.sanitize();
        data
    }

    /// 独立保存通道：创建 `.data/`、`.gitignore`，临时文件 + rename。
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let dir = path.parent().unwrap_or(Path::new(DATA_DIR));
        ensure_dir(dir)?;
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let tmp_dir = dir.join("tmp");
        fs::create_dir_all(&tmp_dir)?;
        let tmp = tmp_dir.join(USER_DATA_FILE);
        fs::write(&tmp, text)?;
        match fs::rename(&tmp, path) {
            Ok(()) => Ok(()),
            Err(_) => {
                // Windows 上 rename 覆盖已存在文件可能失败；视图状态不强求严格原子。
                let _ = fs::remove_file(path);
                fs::rename(&tmp, path)
            }
        }
    }

    pub fn node(&self, key: &NodeKey) -> Option<&NodeViewState> {
        self.ui.grid.nodes.get(key)
    }

    pub fn node_mut(&mut self, key: NodeKey) -> &mut NodeViewState {
        self.ui.grid.nodes.entry(key).or_default()
    }

    pub fn clear_grid_node(&mut self, key: &NodeKey) {
        self.ui.grid.nodes.remove(key);
    }

    pub fn clear_all_ui(&mut self) {
        self.ui = UiPreferences::default();
    }

    fn sanitize(&mut self) {
        for state in self.ui.grid.nodes.values_mut() {
            state.widths.retain(|_, w| {
                w.is_finite()
                    && *w >= MIN_MANUAL_WIDTH
                    && *w <= MAX_MANUAL_WIDTH
            });
        }
    }
}

/// `<workdir>/.data`。
pub fn data_dir(workdir: &Path) -> PathBuf {
    workdir.join(DATA_DIR)
}

/// `<workdir>/.data/user-data.json`。
pub fn user_data_path(workdir: &Path) -> PathBuf {
    data_dir(workdir).join(USER_DATA_FILE)
}

/// `<workdir>/.data/tablet.lock`。
pub fn lock_path(workdir: &Path) -> PathBuf {
    data_dir(workdir).join(LOCK_FILE)
}

/// 创建 `.data/` 并保证有自忽略文件；返回目录。
pub fn ensure_data_dir(workdir: &Path) -> io::Result<PathBuf> {
    let dir = data_dir(workdir);
    ensure_dir(&dir)?;
    Ok(dir)
}

fn ensure_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let gitignore = dir.join(".gitignore");
    if !gitignore.exists() {
        // `*` 连自己也会被忽略；配合仓库根的 `.data/` 规则形成双保险。
        fs::write(gitignore, "*\n")?;
    }
    Ok(())
}

fn default_true() -> bool {
    true
}

fn default_version() -> u32 {
    USER_DATA_VERSION
}

/// 文件里的 `nodes` 是结构化数组（JSON 的 key 只能是字符串），内存里再还原成 map。
mod nodes_serde {
    use super::*;
    use serde::de::Error as _;
    use serde::{Deserializer, Serializer};

    #[derive(Serialize, Deserialize)]
    struct NodeEntry {
        project: String,
        group: String,
        kind: NodeKind,
        node: String,
        #[serde(flatten)]
        state: NodeViewState,
    }

    pub fn serialize<S>(nodes: &BTreeMap<NodeKey, NodeViewState>, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let entries: Vec<NodeEntry> = nodes
            .iter()
            .map(|(key, state)| NodeEntry {
                project: key.project.clone(),
                group: key.group.clone(),
                kind: key.kind,
                node: key.node.clone(),
                state: state.clone(),
            })
            .collect();
        entries.serialize(ser)
    }

    pub fn deserialize<'de, D>(
        de: D,
    ) -> Result<BTreeMap<NodeKey, NodeViewState>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let entries = Vec::<NodeEntry>::deserialize(de)?;
        let mut out = BTreeMap::new();
        for e in entries {
            if e.project.is_empty() || e.group.is_empty() || e.node.is_empty() {
                return Err(D::Error::custom("用户数据节点 key 不能为空"));
            }
            let key = NodeKey {
                project: e.project,
                group: e.group,
                kind: e.kind,
                node: e.node,
            };
            out.insert(key, e.state);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn tmp_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!(
            "tablet_user_data_{}_{}_{}", label, std::process::id(), n
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn key(project: &str, group: &str, kind: NodeKind, node: &str) -> NodeKey {
        NodeKey {
            project: project.into(),
            group: group.into(),
            kind,
            node: node.into(),
        }
    }

    fn sample() -> UserData {
        let mut data = UserData::default();
        let mut state = NodeViewState { auto: false, ..Default::default() };
        state.widths.insert("name".into(), 180.0);
        state.widths.insert("desc".into(), 320.0);
        data.ui.grid.nodes.insert(key("game", "hero", NodeKind::Table, "HeroBase"), state);
        let mut enum_state = NodeViewState::default();
        enum_state.widths.insert("name".into(), 240.0);
        data.ui.grid.nodes.insert(key("game", "hero", NodeKind::Enum, "HeroBase"), enum_state);
        data
    }

    #[test]
    fn round_trip_preserves_root_structure_and_kind() {
        let dir = tmp_dir("roundtrip");
        let path = user_data_path(&dir);
        let data = sample();
        data.save(&path).unwrap();

        let back = UserData::load(&path);
        assert_eq!(back.version, USER_DATA_VERSION);
        assert_eq!(back.ui.grid.nodes.len(), 2, "同名的 Table/Enum 必须分开");
        let table = back.node(&key("game", "hero", NodeKind::Table, "HeroBase")).unwrap();
        assert!(!table.auto);
        assert_eq!(table.widths.get("name"), Some(&180.0));
        let enum_state = back.node(&key("game", "hero", NodeKind::Enum, "HeroBase")).unwrap();
        assert!(enum_state.auto);
        assert_eq!(enum_state.widths.get("name"), Some(&240.0));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_uses_structured_array_and_creates_gitignore() {
        let dir = tmp_dir("shape");
        let path = user_data_path(&dir);
        sample().save(&path).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"nodes\""), "{}", text);
        assert!(text.contains("\"kind\": \"table\""), "{}", text);
        assert!(!text.contains('|'), "不要拼接分隔符 key: {}", text);
        let gi = fs::read_to_string(dir.join(DATA_DIR).join(".gitignore")).unwrap();
        assert!(gi.contains('*'));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_or_missing_file_yields_empty_state() {
        let dir = tmp_dir("corrupt");
        let path = user_data_path(&dir);
        assert_eq!(UserData::load(&path), UserData::default());

        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(UserData::load(&path), UserData::default());

        fs::write(&path, r#"{"version": 999, "ui": {"grid": {"nodes": []}}}"#).unwrap();
        assert_eq!(UserData::load(&path), UserData::default());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn widths_are_sanitized_on_load() {
        let dir = tmp_dir("sanitize");
        let path = user_data_path(&dir);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{
            "version": 1,
            "ui": {"grid": {"nodes": [
                {"project": "p", "group": "g", "kind": "table", "node": "N",
                 "auto": false, "widths": {"ok": 120, "neg": -5, "huge": 9000, "zero": 0}}
            ]}},
            "history": {},
            "session": {}
        }"#).unwrap();

        let data = UserData::load(&path);
        let n = data.node(&key("p", "g", NodeKind::Table, "N")).unwrap();
        assert_eq!(n.widths.get("ok"), Some(&120.0));
        assert!(!n.widths.contains_key("neg"));
        assert!(!n.widths.contains_key("huge"));
        assert!(!n.widths.contains_key("zero"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clear_helpers_work() {
        let mut data = sample();
        data.clear_grid_node(&key("game", "hero", NodeKind::Table, "HeroBase"));
        assert_eq!(data.ui.grid.nodes.len(), 1);
        data.clear_all_ui();
        assert!(data.ui.grid.nodes.is_empty());
    }

    #[test]
    fn lock_path_is_under_data_dir() {
        let dir = tmp_dir("lockpath");
        assert_eq!(lock_path(&dir), dir.join(DATA_DIR).join(LOCK_FILE));
        assert_eq!(user_data_path(&dir), dir.join(DATA_DIR).join(USER_DATA_FILE));
        let _ = fs::remove_dir_all(&dir);
    }
}
