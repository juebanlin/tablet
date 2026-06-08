//! Excel 编辑桥接：UI 入口 → 调起外部 Excel → 监控关闭 → 回写到内存模型。
//!
//! 状态机（@docs/06-Excel桥接.md §2 / §3）：
//! ```
//!     [None]                                                      [Some(session)]
//!       │                                                              │
//!       │  launch_excel_edit() ────────────────────────────────────►   │
//!       │  导出 xlsx + open::that() + 启监控线程                       │
//!       │                                                              │
//!       │                  ◄──────────  on_excel_closed()              │
//!       │                  监控线程独占打开成功 → event loop 触发      │
//!       │                  → import_xlsx_into_group → apply_patch     │
//!       └──────────────────────────────────────────────────────────────┘
//! ```
//!
//! 监控线程通过 `slint::Weak::upgrade_in_event_loop` 把"关闭被检测到"的事件
//! 投回主线程；主线程从 `state.excel_session` 取 session 后调 core 解析。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use slint::ComponentHandle;

use crate::state::AppState;
use crate::AppWindow;

use tablet_core::excel::{
    export_group_book, import_xlsx_into_group, GroupPatch, NodePatch,
};
use tablet_core::model::Group;

/// 单个 Excel 编辑会话。同一时刻全局只允许一个（@plans §2.4）。
pub struct ExcelSession {
    pub project_id: String,
    pub group: String,
    pub xlsx_path: PathBuf,
    pub started_at: chrono::DateTime<chrono::Local>,
    /// 监控线程协作式停止信号。设为 true 后线程在下一次 polling 退出。
    /// 由 [`abort_session`] 在用户「强制放弃」/ 4h 超时时设置。
    pub stop_signal: Arc<AtomicBool>,
    /// 触发本次会话时的 include 列表（空 = 整组）。状态栏 / 强制解析等可能用到。
    #[allow(dead_code)]
    pub include: Vec<String>,
}

impl std::fmt::Debug for ExcelSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExcelSession")
            .field("project_id", &self.project_id)
            .field("group", &self.group)
            .field("xlsx_path", &self.xlsx_path)
            .field("started_at", &self.started_at)
            .field("include", &self.include)
            .finish()
    }
}

/// 把 GroupPatch 应用到指定 group：替换 records / entries，触发 dirty 更新。
///
/// dirty 标记由现有 `update_dirty()` 比对（`serialize(node) != original`）自然产生，
/// 桥接层不主动写 `dirty = true`——保证"xlsx 改回原状"也能正确回到 clean。
pub fn apply_patch_to_group(group: &mut Group, patch: &GroupPatch) {
    for np in &patch.patches {
        match np {
            NodePatch::Table { name, records } => {
                if let Some(t) = group.tables.iter_mut().find(|t| &t.name == name) {
                    t.records = records.clone();
                    t.update_dirty();
                }
            }
            NodePatch::Constant { name, entries } => {
                if let Some(c) = group.constants.iter_mut().find(|c| &c.name == name) {
                    c.entries = entries.clone();
                    c.update_dirty();
                }
            }
            NodePatch::Enum { name, entries } => {
                if let Some(e) = group.enums.iter_mut().find(|e| &e.name == name) {
                    e.entries = entries.clone();
                    e.update_dirty();
                }
            }
        }
    }
}

/// 给 xlsx 缓存文件生成带时间戳后缀的唯一文件名。
///
/// 设计动机（@plans §冲突处理）：用户点「强制放弃」时 Excel 仍可能持有
/// `<group>.xlsx` 的锁，删不掉；下次再点「Excel 编辑」如果用同名路径，
/// `fs::write` 在 Windows 上会拒（拒绝访问），macOS/Linux 上 advisory lock
/// 不一定准——为彻底绕开冲突，每次会话都用 `<group>-<millis>.xlsx` 作为
/// 独立文件，残留交给下次启动 / 退出清理。
///
/// 后缀用 SystemTime 毫秒，碰撞概率近零（人类按钮间隔 >> 1ms）。
fn unique_xlsx_name(group: &str) -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    format!("{}-{}.xlsx", group, millis)
}

/// GUI 入口公共流程：导出 xlsx → 写盘 → open::that() → 启动监控 → 写入 state.excel_session。
///
/// 失败时不修改 state（残留 xlsx 由 caller 负责清理；本函数尽量在失败前不写盘）。
///
/// 每次会话用 [`unique_xlsx_name`] 生成独立文件名，绕开"放弃后 Excel 仍持锁导致
/// 重新调起冲突"的问题——下次调起拿新路径，与上次的残留共存。残留由
/// [`cleanup_all_caches_on_exit`] / [`scan_residuals_on_startup`] 兜底清。
pub fn launch_excel_edit(
    state: &Rc<RefCell<AppState>>,
    weak: slint::Weak<AppWindow>,
    project_id: &str,
    group_name: &str,
    include: Vec<String>,
) -> anyhow::Result<()> {
    // 防御性：已有会话直接拒（UI 应已 disable 入口，这里兜底）
    if state.borrow().excel_session.is_some() {
        anyhow::bail!("已有 Excel 编辑会话进行中");
    }

    // 取数据 + 导出 + 算路径（独立文件名，不会和上次残留冲突）
    let (xlsx_path, bytes) = {
        let st = state.borrow();
        let project = st.engine.find_project(project_id)
            .ok_or_else(|| anyhow::anyhow!("找不到 Project: {}", project_id))?;
        let group = project.groups.iter().find(|g| g.name == group_name)
            .ok_or_else(|| anyhow::anyhow!("找不到分组 '{}'", group_name))?;

        let bytes = if include.is_empty() {
            export_group_book(group, None)?
        } else {
            let xs: Vec<&str> = include.iter().map(|s| s.as_str()).collect();
            export_group_book(group, Some(&xs))?
        };
        let xlsx_path = project.cache_dir().join(unique_xlsx_name(group_name));
        (xlsx_path, bytes)
    };

    // 写盘
    if let Some(parent) = xlsx_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&xlsx_path, &bytes)?;

    // 调起 Excel：系统默认优先 + 平台候选链兜底（@launch_xlsx_editor）
    let launched_with = launch_xlsx_editor(&xlsx_path)?;

    // 启动监控线程
    let stop = Arc::new(AtomicBool::new(false));
    spawn_watcher(weak, xlsx_path.clone(), stop.clone());

    // 写入 session
    let session = ExcelSession {
        project_id: project_id.to_string(),
        group: group_name.to_string(),
        xlsx_path: xlsx_path.clone(),
        started_at: chrono::Local::now(),
        stop_signal: stop,
        include,
    };
    {
        let mut st = state.borrow_mut();
        st.engine.log(format!(
            "[Excel] 已调起 {}::{} 编辑（{}: {}）",
            project_id, group_name, launched_with, xlsx_path.display()
        ));
        st.excel_session = Some(session);
    }
    Ok(())
}

/// 调起外部 xlsx 编辑器。
///
/// 优先用 [`open::that`] 走系统默认（尊重用户对 .xlsx 的文件关联设置——大多数用户已配好）；
/// 失败时按平台候选链兜底。返回实际调起的应用名（用于日志可观测）。
///
/// 候选链（仅在系统默认失败时启用）：
/// - **Windows**：Microsoft Excel → WPS Office → LibreOffice Calc
/// - **macOS**：Microsoft Excel → WPS Office → LibreOffice → Apple Numbers
/// - **Linux**：LibreOffice → WPS Office → OnlyOffice
///
/// 之所以不让用户配优先级：90% 用户的系统默认就是想用的；剩下 10% 走候选链已能覆盖。
/// 真有强诉求改顺序，可以后续加 `tablet.toml [excel] preferred_app = "..."` 配置项。
fn launch_xlsx_editor(path: &std::path::Path) -> anyhow::Result<String> {
    if open::that(path).is_ok() {
        return Ok("系统默认".into());
    }
    log::warn!("[Excel] 系统默认调起失败，尝试候选链");

    for (name, mut cmd) in detect_xlsx_apps() {
        match cmd.arg(path).spawn() {
            Ok(_) => return Ok(name.into()),
            Err(e) => log::debug!("[Excel] 候选 {} 调起失败: {}", name, e),
        }
    }

    anyhow::bail!("未检测到任何 xlsx 编辑器（系统默认 + Excel/WPS/LibreOffice 候选链均失败）")
}

#[cfg(target_os = "windows")]
fn detect_xlsx_apps() -> Vec<(&'static str, std::process::Command)> {
    let pf = std::env::var("ProgramFiles").unwrap_or_default();
    let pf86 = std::env::var("ProgramFiles(x86)").unwrap_or_default();
    let mut out = Vec::new();

    let first_existing = |candidates: &[String]| -> Option<String> {
        candidates.iter().find(|p| std::path::Path::new(p).exists()).cloned()
    };

    // Microsoft Excel：覆盖 Office 365 / 2019 / 2016 / 2013 / 2010 几个常见路径
    let excel = [
        format!(r"{}\Microsoft Office\root\Office16\EXCEL.EXE", pf),
        format!(r"{}\Microsoft Office\root\Office16\EXCEL.EXE", pf86),
        format!(r"{}\Microsoft Office\Office16\EXCEL.EXE", pf),
        format!(r"{}\Microsoft Office\Office15\EXCEL.EXE", pf),
        format!(r"{}\Microsoft Office\Office14\EXCEL.EXE", pf),
    ];
    if let Some(exe) = first_existing(&excel) {
        out.push(("Microsoft Excel", std::process::Command::new(exe)));
    }

    // WPS（金山）：et.exe 是 WPS 的电子表格组件
    let wps = [
        format!(r"{}\Kingsoft\WPS Office\office6\et.exe", pf),
        format!(r"{}\Kingsoft\WPS Office\office6\et.exe", pf86),
    ];
    if let Some(exe) = first_existing(&wps) {
        out.push(("WPS Office", std::process::Command::new(exe)));
    }

    // LibreOffice Calc：scalc.exe 是 LO 的电子表格组件
    let lo = [
        format!(r"{}\LibreOffice\program\scalc.exe", pf),
        format!(r"{}\LibreOffice\program\scalc.exe", pf86),
    ];
    if let Some(exe) = first_existing(&lo) {
        out.push(("LibreOffice Calc", std::process::Command::new(exe)));
    }

    out
}

#[cfg(target_os = "macos")]
fn detect_xlsx_apps() -> Vec<(&'static str, std::process::Command)> {
    let mut out = Vec::new();
    let bundles = [
        ("Microsoft Excel", "/Applications/Microsoft Excel.app"),
        ("WPS Office", "/Applications/wpsoffice.app"),
        ("LibreOffice", "/Applications/LibreOffice.app"),
        ("Apple Numbers", "/Applications/Numbers.app"),
    ];
    for (name, bundle) in bundles {
        if std::path::Path::new(bundle).exists() {
            // macOS 用 `open -a "<bundle>" <file>` 调起 .app
            let mut cmd = std::process::Command::new("open");
            cmd.args(["-a", bundle]);
            out.push((name, cmd));
        }
    }
    out
}

#[cfg(target_os = "linux")]
fn detect_xlsx_apps() -> Vec<(&'static str, std::process::Command)> {
    let mut out = Vec::new();
    let candidates = [
        ("LibreOffice Calc", "libreoffice"),
        ("LibreOffice (soffice)", "soffice"),
        ("WPS (et)", "et"),
        ("WPS Office", "wps"),
        ("OnlyOffice", "onlyoffice-desktopeditors"),
    ];
    for (name, bin) in candidates {
        if which_binary(bin).is_some() {
            out.push((name, std::process::Command::new(bin)));
        }
    }
    out
}

#[cfg(target_os = "linux")]
fn which_binary(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn detect_xlsx_apps() -> Vec<(&'static str, std::process::Command)> {
    Vec::new()
}

/// 启动后台监控线程。1 s polling 用 [`xlsx_is_released`] 多层探测 xlsx 是否已被外部编辑器释放，
/// 释放 = Excel/WPS/LibreOffice 等已关闭文件。
///
/// 启动后先 sleep 3 s——`open::that()` 返回到编辑器真正持锁有几百 ms 间隙，
/// 期间监控可能立即检测到"已关闭"假象（@plans §7 开放问题）。
fn spawn_watcher(weak: slint::Weak<AppWindow>, xlsx_path: PathBuf, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(3));

        loop {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            if xlsx_is_released(&xlsx_path) {
                let _ = weak.upgrade_in_event_loop(|ui_h: AppWindow| {
                    ui_h.invoke_excel_closed_detected();
                });
                return;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

/// xlsx 是否已被外部编辑器释放（可以读 / 删 / 重写）。
///
/// 多层探测降级（按"definite → best-effort"），任一层说"还在用"立即返 false；
/// 都说"释放"才真返 true：
///
/// 1. **Lock 文件检测**（@`has_lock_file`，跨平台）：同目录下若存在
///    `~$<filename>` (Microsoft Excel / WPS) 或 `.~lock.<filename>#` (LibreOffice / OnlyOffice)
///    → 编辑器仍在使用，definite。
/// 2. **`/proc/*/fd/` 扫描**（@`any_process_has_file_open`，仅 Linux）：遍历进程 fd
///    符号链接，命中 = 有进程持有，definite。catch 没有 lock 文件的编辑器（如 some 的 Vim 插件 / 命令行工具）。
/// 3. **OS 写探测**（@`try_exclusive_open`）：`OpenOptions::write` 能否打开。
///    Windows 上极准（Excel 持 share lock，写打开会被拒）；macOS / Linux 上
///    advisory lock 不影响 `open()`，但前两层已 cover Excel / LO，所以这层兜底就够。
///
/// **每平台实际激活的层**（cfg 控制 + 跨平台 helper 自然降级）：
/// - **Windows**：① + ③（① 抓 Excel/WPS 的 ~$file lock；③ 抓 share lock，双保险，互相校准）
/// - **macOS**：① + ③（① 抓 Excel for Mac 的 ~$file 和 LibreOffice 的 .~lock.X#；
///                    ③ advisory lock 不直接管 open() 但偶尔生效，作为 best-effort 兜底）
/// - **Linux**：① + ② + ③（① 抓 LibreOffice 的 .~lock.X#；② /proc fd 兜底
///                    catch 没 lock 文件的编辑器；③ best-effort）
fn xlsx_is_released(path: &std::path::Path) -> bool {
    if has_lock_file(path) {
        return false;
    }
    #[cfg(target_os = "linux")]
    if any_process_has_file_open(path) {
        return false;
    }
    try_exclusive_open(path)
}

/// 检查同目录下是否有编辑器创建的 lock 文件。
///
/// 覆盖：
/// - Microsoft Excel（Windows + macOS）：`~$<filename>`
/// - LibreOffice / OnlyOffice / WPS for Linux：`.~lock.<filename>#`
fn has_lock_file(path: &std::path::Path) -> bool {
    let parent = match path.parent() {
        Some(p) => p,
        None => return false,
    };
    let filename = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };
    parent.join(format!("~${}", filename)).exists()
        || parent.join(format!(".~lock.{}#", filename)).exists()
}

/// Linux 专用：扫 `/proc/*/fd/` 检查是否有进程持有该文件。
///
/// 比 [`has_lock_file`] 更通用（能 catch 没有 lock 文件的编辑器），用作 Linux 第二层
/// 探测。
///
/// 失败（/proc 不存在 / 权限不够读 fd）保守返 false（让 [`try_exclusive_open`] 第三层再判）；
/// 这种宽容性是为了不在权限受限的环境（容器 / sandboxed user）下假阳性。
#[cfg(target_os = "linux")]
fn any_process_has_file_open(path: &std::path::Path) -> bool {
    let canonical = match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let proc_dir = match std::fs::read_dir("/proc") {
        Ok(d) => d,
        Err(_) => return false,
    };
    for entry in proc_dir.flatten() {
        let pid_path = entry.path();
        // 只关心数字 PID 目录（其它如 /proc/sys, /proc/meminfo 跳过）
        let is_numeric_pid = pid_path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        if !is_numeric_pid {
            continue;
        }
        let fd_dir = match std::fs::read_dir(pid_path.join("fd")) {
            Ok(d) => d,
            Err(_) => continue, // 没权限读其它进程 fd，跳过
        };
        for fd_entry in fd_dir.flatten() {
            if let Ok(target) = std::fs::read_link(fd_entry.path()) {
                if target == canonical {
                    return true;
                }
            }
        }
    }
    false
}

/// OS write 锁探测：尝试以读写模式打开。Windows 上 Excel 持 share lock 会拒；
/// macOS / Linux 上 advisory lock 不影响 `open()`，所以这层只在 Windows 上独立可靠，
/// 其它平台必须搭配 [`has_lock_file`] / [`any_process_has_file_open`] 使用。
fn try_exclusive_open(path: &std::path::Path) -> bool {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .is_ok()
}

/// xlsx 关闭事件的主线程回调：解析 → apply patch → 验证 → 清理临时文件。
///
/// 失败路径不修改任何内存模型（结构错误整次拒绝，@plans §2.3）。
pub fn on_excel_closed(state: &Rc<RefCell<AppState>>) {
    let session = match state.borrow_mut().excel_session.take() {
        Some(s) => s,
        None => return, // 已被「放弃」/ 4h 强制等清掉
    };

    // 第一步：parse（不可变借）
    let import_result = {
        let st = state.borrow();
        let project = match st.engine.find_project(&session.project_id) {
            Some(p) => p,
            None => {
                drop(st);
                state.borrow_mut().engine.log(format!(
                    "[Excel] {} 回写失败: project '{}' 已关闭",
                    session.group, session.project_id,
                ));
                let _ = std::fs::remove_file(&session.xlsx_path);
                return;
            }
        };
        let group = match project.groups.iter().find(|g| g.name == session.group) {
            Some(g) => g,
            None => {
                drop(st);
                state.borrow_mut().engine.log(format!(
                    "[Excel] {} 回写失败: 分组已被删除",
                    session.group,
                ));
                let _ = std::fs::remove_file(&session.xlsx_path);
                return;
            }
        };
        import_xlsx_into_group(&session.xlsx_path, group)
    };

    // 第二步：apply（可变借）
    match import_result {
        Ok(patch) => {
            let count = patch.patches.len();
            {
                let mut st = state.borrow_mut();
                if let Some(project) = st.engine.find_project_mut(&session.project_id) {
                    if let Some(group) = project.groups.iter_mut().find(|g| g.name == session.group) {
                        apply_patch_to_group(group, &patch);
                    }
                }
                st.engine.revalidate_all_projects();
                st.engine.log(format!(
                    "[Excel] {} 回写完成（影响 {} 个节点）",
                    session.group, count,
                ));
            }
        }
        Err(e) => {
            state.borrow_mut().engine.log(format!(
                "[Excel] {} 回写失败: {}",
                session.group, e
            ));
        }
    }

    let _ = std::fs::remove_file(&session.xlsx_path);
}

/// 同步 ExcelSession 到 UI（按钮 enabled / 状态栏文案 / disable 标记）。
pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let active = st.excel_session.is_some();
    let label = match &st.excel_session {
        Some(s) => format!(
            "正在 Excel 中编辑 {}::{}，关闭 Excel 后自动回填",
            s.project_id, s.group
        ),
        None => String::new(),
    };
    ui_h.set_excel_session_active(active);
    ui_h.set_excel_session_status(label.into());
}

/// 扫描所有已加载 Project 的 `.tbl-cache/*.xlsx`，返回 (project_id, group_name, xlsx_path) 列表。
///
/// xlsx 文件名约定 `<group>-<millis>.xlsx`（@see [`unique_xlsx_name`]）；
/// group_name 通过 rsplit `-` 提取，suffix 若全是数字才认作 millis（兼容老格式 `<group>.xlsx`
/// 与含 `-` 的 group 名如 `hero-skill`）。
fn scan_all_residuals(state: &Rc<RefCell<AppState>>) -> Vec<(String, String, PathBuf)> {
    let st = state.borrow();
    let mut out = Vec::new();
    for project in &st.engine.projects {
        let cache = project.cache_dir();
        let entries = match std::fs::read_dir(&cache) {
            Ok(e) => e,
            Err(_) => continue, // 目录不存在 = 没残留
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "xlsx") {
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?");
                let group_name = match stem.rsplit_once('-') {
                    Some((g, suffix)) if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) => g,
                    _ => stem,
                }.to_string();
                out.push((project.schema.meta.id.clone(), group_name, path));
            }
        }
    }
    out
}

/// 启动后扫描所有 Project 的 `.tbl-cache/*.xlsx`，**静默删除**残留文件（无弹窗）。
///
/// 残留必然来自上次崩溃 / kill / 强制退出——正常流程会被 [`cleanup_all_caches_on_exit`]
/// 兜底清掉。这里直接删 + 日志通报即可，不打扰用户。
///
/// 删除失败（极少：上次 tablet 崩了但 Excel 还在跑且仍持有 xlsx）时只 log 警告，
/// 不报错；下次调起会被 [`launch_excel_edit`] 的冲突检测拦下。
///
/// @plans §5.6（简化版：恢复编辑选项已弃，残留直接丢）
pub fn scan_residuals_on_startup(state: &Rc<RefCell<AppState>>) {
    let residuals = scan_all_residuals(state);
    if residuals.is_empty() {
        return;
    }

    let mut st = state.borrow_mut();
    for (project_id, group, path) in &residuals {
        match std::fs::remove_file(path) {
            Ok(_) => st.engine.log(format!(
                "[Excel] 发现残留 {}::{} 已自动丢弃",
                project_id, group
            )),
            Err(_) => st.engine.log(format!(
                "[Excel] 发现残留 {}::{}，但删除失败（可能 Excel 仍在打开），跳过",
                project_id, group
            )),
        }
    }
}

/// 退出清理：删除所有已加载 Project 的 `.tbl-cache/*.xlsx`。
///
/// 异常退出（崩溃 / kill）由 [`scan_residuals_on_startup`] 兜底，本路径不处理。
///
/// @plans §5.7
pub fn cleanup_all_caches_on_exit(state: &Rc<RefCell<AppState>>) {
    // 若仍有 active session，先停掉监控线程（即使程序快要退出，避免线程访问已释放资源）
    if let Some(session) = state.borrow_mut().excel_session.take() {
        session.stop_signal.store(true, Ordering::Relaxed);
    }
    let residuals = scan_all_residuals(state);
    for (_, _, path) in residuals {
        let _ = std::fs::remove_file(&path);
    }
}

/// 强制放弃当前 Excel 编辑会话（不合并任何改动）。
///
/// 触发场景：
/// - 用户在 modal 弹窗里点「强制放弃（不合并）」按钮
/// - 4h 超时（Timer 自动调）
///
/// **xlsx 删除可能失败**：Windows 下 Excel 仍持锁时 fs::remove_file 会被拒；此时
/// 在日志里说明，让用户手动关闭 Excel；下次再点「Excel 编辑」会被
/// [`launch_excel_edit`] 的冲突检测拦下，给出"先关闭 Excel 再重试"提示。
///
/// @plans §5.5（简化版：超时直接放弃，不弹三选项）
pub fn abort_session(state: &Rc<RefCell<AppState>>) {
    let session = match state.borrow_mut().excel_session.take() {
        Some(s) => s,
        None => return,
    };
    session.stop_signal.store(true, Ordering::Relaxed);

    let log = match std::fs::remove_file(&session.xlsx_path) {
        Ok(_) => format!(
            "[Excel] {} 编辑已放弃（不合并任何改动）",
            session.group,
        ),
        Err(_) => format!(
            "[Excel] {} 编辑已放弃；xlsx 仍在 Excel 中打开，请手动关闭以释放缓存（下次调起会被拦截直至关闭）",
            session.group,
        ),
    };
    state.borrow_mut().engine.log(log);
}

/// 注册 `excel-closed-detected` callback——监控线程的 event-loop 投递目标。
/// 同时注册「强制放弃」cancel callback 以及 4h 超时定时器（@plans §5.5 简化版）。
pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    // 1. xlsx 关闭被检测到（监控线程 → event loop → 此处）
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_excel_closed_detected(move || {
            on_excel_closed(&s);
            if let Some(ui_h) = weak.upgrade() {
                push(&ui_h, &s);
                crate::refresh::after_grid_edit(&ui_h, &s);
            }
        });
    }

    // 2. 模态弹窗「强制放弃」按钮
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_excel_modal_cancel_clicked(move || {
            abort_session(&s);
            if let Some(ui_h) = weak.upgrade() {
                push(&ui_h, &s);
                crate::refresh::after_grid_edit(&ui_h, &s);
            }
        });
    }

    // 3. 4h 超时定时器：每 1 min tick 一次，session 时长 ≥ 4h 自动 abort。
    //    Timer 必须 leak 才能活到进程退出（slint::Timer 不 Send，只能在主线程持有）。
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(60),
            move || {
                let should_abort = {
                    let st = s.borrow();
                    match &st.excel_session {
                        Some(session) => {
                            chrono::Local::now()
                                .signed_duration_since(session.started_at)
                                .num_hours() >= 4
                        }
                        None => false,
                    }
                };
                if should_abort {
                    s.borrow_mut().engine.log(
                        "[Excel] 编辑超时（>4h），自动强制放弃".to_string(),
                    );
                    abort_session(&s);
                    if let Some(ui_h) = weak.upgrade() {
                        push(&ui_h, &s);
                        crate::refresh::after_grid_edit(&ui_h, &s);
                    }
                }
            },
        );
        // 让 Timer 活到进程退出。slint::Timer 不 Send，所以只能在主线程的事件循环上绑定。
        Box::leak(Box::new(timer));
    }
}
