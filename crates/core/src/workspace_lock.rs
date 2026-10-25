//! 工作区进程锁：**GUI 与 CLI 互斥**（@03.7.5）。
//!
//! 为什么需要第二把锁：锁文件原来只是 GUI 的单实例开关，CLI 不参与，
//! 于是「GUI 开着 + 跑 CLI」「两个 CLI 写同一个组」都没有任何互斥。组不在进程目录里，
//! 所有进程共享同一个 `workdir/projects/<id>/config/<group>/`，同时写就是丢更新，
//! 甚至（临时文件名相同时）写坏文件。
//!
//! 写法抄 git 的 `index.lock`：
//! - `create_new` 独占创建，谁先谁得；
//! - 文件里写 `<pid> <kind>`，持有者崩了也能靠 PID 存活检测认出残留；
//! - `WorkspaceLock` drop 时只删**自己的**那把（PID 对得上才删）。
//!
//! 这不是文件系统锁（不阻止别的编辑器写 xlsx），只是**本工具自己的进程之间**的约定。
//! Excel 占用另外由 `~$` 检测，外部改动由基线核对兜底（@02.26.8）。

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// 锁文件里记的进程类型，只用于报错时把话说清楚。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcKind {
    Gui,
    Cli,
    /// 旧版锁文件只有 PID，没有类型
    Unknown,
}

impl ProcKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ProcKind::Gui => "gui",
            ProcKind::Cli => "cli",
            ProcKind::Unknown => "?",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ProcKind::Gui => "GUI",
            ProcKind::Cli => "CLI",
            ProcKind::Unknown => "进程",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "gui" => ProcKind::Gui,
            "cli" => ProcKind::Cli,
            _ => ProcKind::Unknown,
        }
    }
}

/// 持有期间有效；drop 时释放。
#[derive(Debug)]
pub struct WorkspaceLock {
    path: PathBuf,
    pid: u32,
}

impl Drop for WorkspaceLock {
    fn drop(&mut self) {
        // 只删自己的锁：万一残留锁被别的进程接管，PID 对不上就不能动。
        if read_lock(&self.path).map(|(pid, _)| pid) == Some(self.pid) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn read_lock(path: &Path) -> Option<(u32, ProcKind)> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut it = text.split_whitespace();
    let pid = it.next()?.parse().ok()?;
    let kind = it.next().map(ProcKind::parse).unwrap_or(ProcKind::Unknown);
    Some((pid, kind))
}

/// 尝试拿到工作区锁。被别的活进程占着就返回错误（GUI / CLI 互斥）。
pub fn acquire(workdir: &Path, kind: ProcKind) -> Result<WorkspaceLock> {
    crate::local_state::ensure_data_dir(workdir).with_context(|| {
        format!("无法创建用户数据目录 {}", crate::local_state::data_dir(workdir).display())
    })?;
    let path = crate::local_state::lock_path(workdir);
    // 残留锁：最多抢两轮（第一次遇到残留 → 删掉 → 再 create_new）。
    for _ in 0..2 {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                let pid = std::process::id();
                // 写失败不致命：锁文件已经在，互斥仍然成立。
                let _ = writeln!(f, "{} {}", pid, kind.as_str());
                return Ok(WorkspaceLock { path, pid });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let (pid, holder) = read_lock(&path).unwrap_or((0, ProcKind::Unknown));
                if pid != 0 && is_process_alive(pid) {
                    anyhow::bail!(
                        "工作区正被另一个 {} 使用（PID {}）：GUI 与 CLI 不能同时操作 {}",
                        holder.label(), pid, workdir.display()
                    );
                }
                // 持有者已经不在了：残留锁，清掉重试。
                let _ = std::fs::remove_file(&path);
            }
            Err(e) => {
                return Err(e).with_context(|| format!("无法获取工作区锁 {}", path.display()));
            }
        }
    }
    anyhow::bail!("无法获取工作区锁 {}", path.display())
}

/// 这个 PID 现在是否还活着。跨平台各用一条轻量命令。
pub fn is_process_alive(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Unix-like：kill 0 不发送信号，仅检查目标进程是否存在
        use std::process::Command;
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(label: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!(
            "tablet_lock_{}_{}_{}", label, std::process::id(), n));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn second_acquire_is_refused_while_the_first_is_alive() {
        let dir = tmp_dir("alive");
        let first = acquire(&dir, ProcKind::Gui).expect("第一把锁");
        let err = acquire(&dir, ProcKind::Cli).expect_err("第二把必须被拒绝");
        let msg = format!("{:#}", err);
        assert!(msg.contains("GUI"), "错误里要说清是谁占着: {}", msg);
        assert!(msg.contains(&std::process::id().to_string()), "要带 PID: {}", msg);
        drop(first);
        // 释放之后可以再拿
        acquire(&dir, ProcKind::Cli).expect("释放后应能再拿");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stale_lock_is_taken_over() {
        let dir = tmp_dir("stale");
        let path = crate::local_state::lock_path(&dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // 一个几乎不可能存在的 PID（Windows 上 PID 是 4 的倍数且 << 4M）
        std::fs::write(&path, "4294967290 gui\n").unwrap();
        let lock = acquire(&dir, ProcKind::Cli).expect("残留锁应被接管");
        drop(lock);
        assert!(!path.exists(), "drop 后锁文件应删除");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_lock_file_without_kind_still_parses() {
        let dir = tmp_dir("legacy");
        let path = crate::local_state::lock_path(&dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("{}\n", std::process::id())).unwrap();
        let err = acquire(&dir, ProcKind::Cli).expect_err("活进程的旧格式锁也要拦住");
        assert!(format!("{:#}", err).contains("进程"), "{}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn drop_does_not_delete_a_lock_that_changed_hands() {
        let dir = tmp_dir("handover");
        let lock = acquire(&dir, ProcKind::Cli).unwrap();
        let path = crate::local_state::lock_path(&dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // 模拟锁被别的进程接管（PID 变了）
        std::fs::write(&path, "1 gui\n").unwrap();
        drop(lock);
        assert!(path.exists(), "不该删掉别人的锁");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
