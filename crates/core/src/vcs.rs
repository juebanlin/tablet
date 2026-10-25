//! 读取工作区所在版本库的**分支 / 修订**（只读，不依赖 `git` 命令）。
//!
//! 用途：GUI 标题栏显示当前配置仓库的分支，提醒不要改错分支；CLI `workspace info`
//! 把同样的信息交给 Unity 插件 / CI。
//!
//! 为什么自己解析 `.git` 而不是起 `git` 进程：
//! - 不要求 PATH 里有 git，Windows 上也不会闪控制台；
//! - 毫秒级，GUI 标题可以随加载刷新。
//!
//! 支持的 Git 形态：普通 checkout、`gitdir:` 文件（submodule / worktree）、
//! worktree 的 `commondir`、loose ref、`packed-refs`、detached HEAD、unborn branch。
//! 不支持 reftable（会明确写进 `error`，不瞎猜）。SVN 依赖 `svn info --show-item`
//! （SVN 1.9+），没有命令行工具时只报 `kind = svn`。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VcsKind {
    Git,
    Svn,
}

impl VcsKind {
    pub fn as_str(self) -> &'static str {
        match self {
            VcsKind::Git => "git",
            VcsKind::Svn => "svn",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            VcsKind::Git => "Git",
            VcsKind::Svn => "SVN",
        }
    }
}

/// 一次探测结果。所有字段都允许为空：没有版本库、detached、缺 svn 工具都是正常情况。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VcsInfo {
    pub kind: Option<VcsKind>,
    /// Git 分支名 / SVN 相对 URL（`trunk`、`branches/foo`）。
    pub branch: Option<String>,
    /// Git 短 hash / SVN 修订号。
    pub revision: Option<String>,
    /// Git detached HEAD（此时 `branch` 为空）。
    pub detached: bool,
    /// 解析失败 / 缺工具时的说明；GUI 可以忽略或显示在日志里。
    pub error: Option<String>,
}

impl VcsInfo {
    /// 一行摘要：`main @ 8c47a4f`、`trunk@12345`、`HEAD@abc1234`。
    /// 没有任何可用信息时返回 None。
    pub fn display(&self) -> Option<String> {
        match self.kind? {
            VcsKind::Git => match (&self.branch, &self.revision) {
                (Some(b), Some(r)) => Some(format!("{} @ {}", b, r)),
                (Some(b), None) => Some(b.clone()),
                (None, Some(r)) => Some(format!("HEAD@{}", r)),
                (None, None) => None,
            },
            VcsKind::Svn => match (&self.branch, &self.revision) {
                (Some(b), Some(r)) => Some(format!("{}@{}", b, r)),
                (Some(b), None) => Some(b.clone()),
                (None, Some(r)) => Some(format!("r{}", r)),
                (None, None) => None,
            },
        }
    }
}

/// 从 `start` 向上找 Git / SVN 工作副本；Git 优先。
pub fn probe(start: &Path) -> VcsInfo {
    if let Some(git_dir) = find_git_dir(start) {
        return probe_git(&git_dir);
    }
    if let Some(root) = find_dir_with(start, ".svn") {
        return probe_svn(&root);
    }
    VcsInfo::default()
}

/// 向上找包含 `.git`（目录或 gitdir 文件）的目录，返回真正的 gitdir。
fn find_git_dir(start: &Path) -> Option<PathBuf> {
    for dir in start.ancestors() {
        let dot = dir.join(".git");
        if dot.is_dir() {
            return Some(dot);
        }
        if dot.is_file() {
            let Ok(text) = fs::read_to_string(&dot) else { continue };
            let Some(rest) = text.trim().strip_prefix("gitdir:") else { continue };
            let rest = rest.trim();
            if rest.is_empty() {
                continue;
            }
            let p = Path::new(rest);
            return Some(if p.is_absolute() { p.to_path_buf() } else { dir.join(p) });
        }
    }
    None
}

/// 向上找包含指定子目录（如 `.svn`）的工作副本根。
fn find_dir_with(start: &Path, sub: &str) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(sub).is_dir())
        .map(|dir| dir.to_path_buf())
}

fn probe_git(git_dir: &Path) -> VcsInfo {
    let mut info = VcsInfo {
        kind: Some(VcsKind::Git),
        ..Default::default()
    };
    let head = match fs::read_to_string(git_dir.join("HEAD")) {
        Ok(s) => s,
        Err(e) => {
            info.error = Some(format!("无法读取 .git/HEAD: {}", e));
            return info;
        }
    };
    let head = head.trim();
    let common = common_dir(git_dir);

    if let Some(rest) = head.strip_prefix("ref:") {
        let refname = rest.trim();
        if refname.is_empty() {
            info.error = Some("HEAD 为空引用".to_string());
            return info;
        }
        info.branch = refname
            .strip_prefix("refs/heads/")
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());
        info.revision = resolve_ref(git_dir, &common, refname).map(|sha| short_hash(&sha));
        // unborn branch：ref 文件还没生成，这是合法状态，不算错误。
        if info.revision.is_none() && has_reftable(git_dir, &common) {
            info.error = Some("仓库使用 reftable，暂不支持读取提交号".to_string());
        }
        return info;
    }

    if is_hex_hash(head) {
        info.detached = true;
        info.revision = Some(short_hash(head));
        return info;
    }

    info.error = Some(format!("无法解析 .git/HEAD: {}", head));
    info
}

/// worktree 的 refs / packed-refs 在 common dir；普通仓库没有 commondir 文件。
fn common_dir(git_dir: &Path) -> PathBuf {
    let Ok(text) = fs::read_to_string(git_dir.join("commondir")) else {
        return git_dir.to_path_buf();
    };
    let rest = text.trim();
    if rest.is_empty() {
        return git_dir.to_path_buf();
    }
    let p = Path::new(rest);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        git_dir.join(p)
    }
}

fn resolve_ref(git_dir: &Path, common: &Path, refname: &str) -> Option<String> {
    for base in [git_dir, common] {
        if let Ok(text) = fs::read_to_string(base.join(refname)) {
            let text = text.trim();
            if is_hex_hash(text) {
                return Some(text.to_string());
            }
        }
    }
    for base in [common, git_dir] {
        let Ok(text) = fs::read_to_string(base.join("packed-refs")) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
                continue;
            }
            let mut it = line.split_whitespace();
            if let (Some(sha), Some(name)) = (it.next(), it.next()) {
                if name == refname && is_hex_hash(sha) {
                    return Some(sha.to_string());
                }
            }
        }
    }
    None
}

fn has_reftable(git_dir: &Path, common: &Path) -> bool {
    git_dir.join("reftable").is_dir() || common.join("reftable").is_dir()
}

fn is_hex_hash(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn short_hash(s: &str) -> String {
    s.chars().take(7).collect()
}

fn probe_svn(root: &Path) -> VcsInfo {
    let mut info = VcsInfo {
        kind: Some(VcsKind::Svn),
        ..Default::default()
    };
    let url = svn_show_item(root, "relative-url");
    let rev = svn_show_item(root, "revision");

    info.branch = url
        .as_deref()
        .map(|s| s.trim())
        .and_then(|s| s.strip_prefix("^/"))
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    info.revision = rev.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

    if info.branch.is_none() && info.revision.is_none() {
        info.error = Some("无法读取 SVN 信息（需要 svn 命令，SVN 1.9+）".to_string());
    }
    info
}

fn svn_show_item(root: &Path, item: &str) -> Option<String> {
    // --no-newline 是 1.9+；老版本不支持时退回不带该参数再 trim。
    svn_output(root, &["info", "--show-item", item, "--no-newline"])
        .or_else(|| svn_output(root, &["info", "--show-item", item]))
}

fn svn_output(root: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("svn");
    cmd.args(args).arg(root);
    // Windows 上从 GUI / Unity 里起进程时不要闪控制台。
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    fn tmp_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!(
            "tablet_vcs_{}_{}_{}",
            std::process::id(),
            n,
            label
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(path: impl AsRef<Path>, text: &str) {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, text).unwrap();
    }

    #[test]
    fn git_loose_ref() {
        let d = tmp_dir("git_loose");
        write(d.join(".git/HEAD"), "ref: refs/heads/main\n");
        write(d.join(".git/refs/heads/main"), &format!("{}\n", SHA));

        let info = probe(&d);
        assert_eq!(info.kind, Some(VcsKind::Git));
        assert_eq!(info.branch.as_deref(), Some("main"));
        assert_eq!(info.revision.as_deref(), Some("0123456"));
        assert!(!info.detached);
        assert!(info.error.is_none());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn git_packed_refs() {
        let d = tmp_dir("git_packed");
        write(d.join(".git/HEAD"), "ref: refs/heads/main\n");
        write(
            d.join(".git/packed-refs"),
            &format!("# pack-refs with: peeled fully-peeled sorted\n{} refs/heads/main\n", SHA),
        );

        let info = probe(&d);
        assert_eq!(info.branch.as_deref(), Some("main"));
        assert_eq!(info.revision.as_deref(), Some("0123456"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn git_detached_head() {
        let d = tmp_dir("git_detached");
        write(d.join(".git/HEAD"), &format!("{}\n", SHA));

        let info = probe(&d);
        assert_eq!(info.kind, Some(VcsKind::Git));
        assert!(info.branch.is_none());
        assert!(info.detached);
        assert_eq!(info.revision.as_deref(), Some("0123456"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn git_unborn_branch_is_not_an_error() {
        let d = tmp_dir("git_unborn");
        write(d.join(".git/HEAD"), "ref: refs/heads/main\n");

        let info = probe(&d);
        assert_eq!(info.branch.as_deref(), Some("main"));
        assert!(info.revision.is_none());
        assert!(info.error.is_none());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn gitdir_file_and_nested_start() {
        let d = tmp_dir("git_gitdir");
        let real = d.join("real");
        write(real.join(".git/HEAD"), "ref: refs/heads/dev\n");
        write(real.join(".git/refs/heads/dev"), &format!("{}\n", SHA));
        let work = d.join("work");
        fs::create_dir_all(&work).unwrap();
        write(work.join(".git"), "gitdir: ../real/.git\n");

        let info = probe(&work.join("projects/demo"));
        assert_eq!(info.branch.as_deref(), Some("dev"));
        assert_eq!(info.revision.as_deref(), Some("0123456"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn git_worktree_commondir() {
        let d = tmp_dir("git_worktree");
        let repo_git = d.join("repo/.git");
        write(repo_git.join("refs/heads/main"), &format!("{}\n", SHA));
        let wt_git = repo_git.join("worktrees/wt");
        write(wt_git.join("HEAD"), "ref: refs/heads/main\n");
        write(wt_git.join("commondir"), "../..\n");
        let wt = d.join("wt");
        fs::create_dir_all(&wt).unwrap();
        write(wt.join(".git"), &format!("gitdir: {}\n", wt_git.display()));

        let info = probe(&wt);
        assert_eq!(info.branch.as_deref(), Some("main"));
        assert_eq!(info.revision.as_deref(), Some("0123456"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn git_head_with_crlf() {
        let d = tmp_dir("git_crlf");
        write(d.join(".git/HEAD"), "ref: refs/heads/main\r\n");
        write(d.join(".git/refs/heads/main"), &format!("{}\r\n", SHA));

        let info = probe(&d);
        assert_eq!(info.branch.as_deref(), Some("main"));
        assert_eq!(info.revision.as_deref(), Some("0123456"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn git_reftable_is_reported_not_guessed() {
        let d = tmp_dir("git_reftable");
        write(d.join(".git/HEAD"), "ref: refs/heads/main\n");
        fs::create_dir_all(d.join(".git/reftable")).unwrap();

        let info = probe(&d);
        assert_eq!(info.branch.as_deref(), Some("main"));
        assert!(info.revision.is_none());
        assert!(info.error.as_deref().unwrap_or("").contains("reftable"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn no_vcs() {
        let d = tmp_dir("none");
        let info = probe(&d);
        assert_eq!(info.kind, None);
        assert!(info.display().is_none());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn svn_dir_is_detected_even_without_cli() {
        let d = tmp_dir("svn");
        fs::create_dir_all(d.join(".svn")).unwrap();
        let info = probe(&d);
        assert_eq!(info.kind, Some(VcsKind::Svn));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn display_formats() {
        let git = VcsInfo {
            kind: Some(VcsKind::Git),
            branch: Some("main".into()),
            revision: Some("8c47a4f".into()),
            ..Default::default()
        };
        assert_eq!(git.display().as_deref(), Some("main @ 8c47a4f"));

        let detached = VcsInfo {
            kind: Some(VcsKind::Git),
            revision: Some("abc1234".into()),
            detached: true,
            ..Default::default()
        };
        assert_eq!(detached.display().as_deref(), Some("HEAD@abc1234"));

        let svn = VcsInfo {
            kind: Some(VcsKind::Svn),
            branch: Some("trunk".into()),
            revision: Some("12345".into()),
            ..Default::default()
        };
        assert_eq!(svn.display().as_deref(), Some("trunk@12345"));
    }
}
