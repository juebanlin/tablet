# Unity / 外部宿主调用 tablet

tablet **不内置 Unity 专用配置或命令**。Unity 插件 / bat 只是一个外部调用方：

1. 先在 tablet 里把导出路径设好（绝对 / 相对都可以）；
2. 宿主调用 `tablet-cli` 的单格式导出命令；
3. 需要显示分支时调 `workspace info`。

这样客户端、服务端各自调用自己的格式，tablet 不需要知道 Unity 的存在。

---

## 1. 参考布局（只是参考）

```text
<game>/                          ← 本地 checkout 根
  config/                        ← SVN / Git 配置仓库（tablet 可放在里面）
    tools/tablet/tablet-cli.exe  ← 纯 CLI，随 config 仓库分发
    tablet.toml
    projects/game/...
    build_config.bat             ← 宿主侧脚本（也可以放 client 仓库）
  client/                        ← Unity 工程
  server/
```

硬约束只有三条：

- **工作区不能放在 `Assets/` 下面**（Unity 会导入 `.tbl` / 生成物，且 `.excel` / 缓存会很乱）。
- 生成目录必须**专用**：导出 sync 会删除目录里未生成的 `.json` / `.cs`。
- 宿主用**相对路径**定位 config 仓库和 `tablet-cli`；不要让提交的配置里出现某台机器的绝对路径。

其他布局也可以：

| 布局 | 说明 |
|---|---|
| 工具在独立 tools 仓库 / 已加入 PATH | 只把 `config/` 作为工作区，`tablet-cli` 从 PATH 找 |
| config 作为 Unity 的子模块（`<unity>/ConfigData`） | 工作区仍在 `Assets/` 外；导出路径用相对路径指回 `Assets/...` |
| 一个工作区多个项目 | 每次调用显式 `--project <id>`，不要依赖 `last_project` |

---

## 2. 一次性配置导出路径

在 tablet GUI「项目设置 → 导出」里设置（写入 `project.toml`，会跟随项目提交）：

| 用途 | 配置项 | 落点 |
|---|---|---|
| Unity JSON 数据 | `[export.server] data_output` | `<data_output>/json/*.json` |
| Unity C# 代码 | `[export.client.csharp_unity] code_output` | `<code_output>/*.cs` |

例（布局见 §1，项目根是 `config/projects/game`）：

```toml
[export.server]
data_output = "../../../client/Assets/Resources/Config"

[export.client.csharp_unity]
code_output = "../../../client/Assets/Scripts/Config"
```

- **相对路径的基准是 `projects/<id>/`**，不是当前目录、也不是 Unity 工程根。
  从 `config/projects/game` 回到 `<game>` 需要三级 `..`，所以是 `../../../client/...`。
- 绝对路径也可以（适合本机临时设置），但不要提交。
- 不想改配置时，CLI 也可以临时 `-o` 覆盖（见 §3）。
- Unity 的 Loader 用 `Resources.LoadAll<TextAsset>(...)`，所以 JSON 必须落在
  `Assets/Resources/` 下。上例数据在 `Assets/Resources/Config/json`，运行时调用：

```csharp
Game.Config.Client.Loader.LoadAll("Config/json");
```

---

## 3. 客户端调用命令

参数顺序：`-w` / `--project` / `-s` 必须写在子命令**前面**；`--fmt` 是全局参数，位置随意。

```bash
# 1. 先验证（有错就停，避免导出半套数据）
tablet-cli -w <config仓库根> --project game validate

# 2. 只导 JSON 数据（落到 [export.server] data_output/json/）
tablet-cli -w <config仓库根> --project game export data --json

# 3. 只导 Unity C#（落到 [export.client.csharp_unity] code_output/）
tablet-cli -w <config仓库根> --project game export code --csharp-unity
```

- `--csharp-unity` 只生成 Unity 这一套；`--csharp` 会同时生成 dotnet / unity / godot 三套。
  给 Unity 用 `--csharp-unity` 就够了，别顺带生成另外两套。
- `export data --json -o <dir>` / `export code --csharp-unity -o <dir>` 可以临时覆盖输出目录。
  多格式同时 `-o` 时按 `server/...`、`client/...` 标准子目录展开（例如 `--csharp -o <root>` 会写到
  `<root>/server/csharp`、`<root>/client/csharp_unity`、`<root>/client/csharp_godot`），不会互相覆盖。
- 服务端照常用自己的组合（`export data --xml`、`export code --java|--go|--cpp|...`）。
- 不需要 `workspace save`；`export` 只读磁盘上的 `.tbl` / `.xlsx`，不会写 `projects/`。
  手改过 `.tbl` 后需要把数据/结构落盘时才跑 `workspace save`；manifest 只刷新索引。
- xlsx 组：先保存 Excel 再点刷新；未保存的编辑 CLI 看不到。
- tablet GUI 开着同一工作区时，CLI 会被 `.data/tablet.lock` 拒绝（`workspace info` 除外）。
- 退出码：`0` 成功；`1` 验证 / 导出失败；`2` 用法错误。错误在 stderr。

---

## 4. 显示当前配置分支

```bash
tablet-cli -w <config仓库根> --fmt json workspace info
```

```json
{
  "workdir": "D:/game/config",
  "vcs": "git",
  "branch": "main",
  "revision": "8c47a4f",
  "detached": false,
  "vcs_error": null,
  "last_project": "game",
  "projects": [{"id": "game", "name": "SLG"}]
}
```

- Git：直接读 `.git`（含 submodule / worktree），不要求 PATH 里有 git。
- SVN：用 `svn info --show-item`（SVN 1.9+）；没装命令行工具时 `branch` / `revision` 为空，
  `vcs_error` 说明原因。TortoiseSVN 安装时记得勾选 command line client tools。
- detached HEAD：`branch = null`、`detached = true`、`revision` 是短 hash；UI 显示 `HEAD@abc1234`。
- **不加载 `.tbl`、不占工作区锁**，即使某张表坏了或 GUI 开着也能读。
- 它是只读快照；外部切分支后重新调用一次即可。

---

## 5. Windows：`build_config.bat`

放在 `config/` 根目录（按实际布局改 `CONFIG` / `CLIENT` / `TABLET` 三行）：

```bat
@echo off
setlocal
rem 中文输出统一 UTF-8；诊断信息一律走 stderr，stdout 留给 CLI
chcp 65001 >nul

set "CONFIG=%~dp0"
set "CLIENT=%~dp0..\client"
set "TABLET=%~dp0tools\tablet\tablet-cli.exe"

if not exist "%TABLET%" (
    echo [错误] 找不到 tablet-cli.exe: %TABLET% 1>&2
    exit /b 1
)
if not exist "%CLIENT%\Assets" (
    echo [错误] 找不到 Unity 工程: %CLIENT% 1>&2
    exit /b 1
)

"%TABLET%" -w "%CONFIG%" --project game validate
if errorlevel 1 exit /b 1

"%TABLET%" -w "%CONFIG%" --project game export data --json
if errorlevel 1 exit /b 1

"%TABLET%" -w "%CONFIG%" --project game export code --csharp-unity
if errorlevel 1 exit /b 1

echo [成功] 配置已刷新
exit /b 0
```

- 不要在这里 `cargo build` / `git pull` / `pause`：按钮要能无人值守地失败并返回退出码。
- macOS / Linux 把同样的命令写进 `.sh`（`cd "$(dirname "$0")"` 后执行）即可。

---

## 6. Unity 编辑器窗口示例

一个带「分支显示 + 刷新配置」的 `EditorWindow`。放进 `Assets/Editor/` 即可（需要 Newtonsoft，
生成的 Unity Loader 本来就依赖它）。

```csharp
using System;
using System.Diagnostics;
using System.IO;
using System.Text;
using Newtonsoft.Json.Linq;
using UnityEditor;
using UnityEngine;

public class TabletConfigWindow : EditorWindow
{
    // 相对 Unity 工程根；按实际布局改
    const string ConfigRelPath = "../config";
    const string ProjectId = "game";
    const string TabletCliRelPath = "tools/tablet/tablet-cli.exe";

    string _branch = "读取中...";
    bool _busy;

    [MenuItem("Tools/配置/刷新配置")]
    static void Open() => GetWindow<TabletConfigWindow>("配置");

    void OnEnable() => RefreshInfo();

    void OnGUI()
    {
        EditorGUILayout.LabelField("配置分支", _branch);
        EditorGUILayout.Space();

        using (new EditorGUI.DisabledScope(_busy))
        {
            if (GUILayout.Button("刷新配置")) RefreshConfig();
            if (GUILayout.Button("重新读取分支")) RefreshInfo();
        }
    }

    void OnInspectorUpdate() => Repaint();

    string ProjectRoot
    {
        get
        {
            var parent = Directory.GetParent(Application.dataPath);
            return parent != null ? parent.FullName : Application.dataPath;
        }
    }
    string ConfigRoot => Path.GetFullPath(Path.Combine(ProjectRoot, ConfigRelPath));
    string Cli => Path.Combine(ConfigRoot, TabletCliRelPath);

    void RefreshInfo()
    {
        var r = RunTablet($"-w \"{ConfigRoot}\" --fmt json workspace info");
        if (r.exit != 0)
        {
            _branch = $"读取失败：{Tail(r.err)}";
            return;
        }
        try
        {
            var jo = JObject.Parse(r.out);
            string vcs = (string)jo["vcs"];
            string branch = (string)jo["branch"];
            string rev = (string)jo["revision"];
            if (vcs == null) _branch = "未检测到 Git / SVN";
            else if (branch != null) _branch = $"{branch}{(rev != null ? " @ " + rev : "")}";
            else if (rev != null) _branch = $"HEAD@{rev}（detached）";
            else _branch = $"{vcs}（无法读取分支）";
        }
        catch (Exception e)
        {
            _branch = $"解析失败：{e.Message}";
        }
    }

    void RefreshConfig()
    {
        if (EditorApplication.isCompiling || EditorApplication.isUpdating) return;
        _busy = true;
        try
        {
            string prefix = $"-w \"{ConfigRoot}\" --project {ProjectId}";
            var v = RunTablet($"{prefix} validate");
            if (v.exit != 0) { Fail("验证失败", v); return; }

            var d = RunTablet($"{prefix} export data --json");
            if (d.exit != 0) { Fail("导出数据失败", d); return; }

            var c = RunTablet($"{prefix} export code --csharp-unity");
            if (c.exit != 0) { Fail("导出 C# 失败", c); return; }

            AssetDatabase.Refresh();
            RefreshInfo();
            UnityEngine.Debug.Log("[tablet] 配置已刷新");
        }
        finally
        {
            _busy = false;
        }
    }

    void Fail(string title, (int exit, string @out, string err) r)
    {
        UnityEngine.Debug.LogError($"[tablet] {title}\n{r.@out}\n{r.err}");
        EditorUtility.DisplayDialog(title, Tail(r.err), "好");
    }

    static string Tail(string s) =>
        string.IsNullOrEmpty(s) ? "(无输出)"
        : (s.Length <= 2000 ? s : s.Substring(s.Length - 2000));

    (int exit, string @out, string err) RunTablet(string args)
    {
        var psi = new ProcessStartInfo
        {
            FileName = Cli,
            Arguments = args,
            WorkingDirectory = ConfigRoot,
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            StandardOutputEncoding = Encoding.UTF8,
            StandardErrorEncoding = Encoding.UTF8,
        };
        var sbOut = new StringBuilder();
        var sbErr = new StringBuilder();
        using (var p = new Process { StartInfo = psi })
        {
            p.OutputDataReceived += (_, e) => { if (e.Data != null) sbOut.AppendLine(e.Data); };
            p.ErrorDataReceived += (_, e) => { if (e.Data != null) sbErr.AppendLine(e.Data); };
            p.Start();
            p.BeginOutputReadLine();
            p.BeginErrorReadLine();
            p.WaitForExit();
            return (p.ExitCode, sbOut.ToString(), sbErr.ToString());
        }
    }
}
```

> **不要**先 `ReadToEnd()` 再 `WaitForExit()`，stdout/stderr 任一管道写满会互相卡死；
> 用 `BeginOutputReadLine` / `BeginErrorReadLine` 或异步 `Task` 并发读。

---

## 7. 常见失败

| 现象 | 原因 / 处理 |
|---|---|
| `无法获取工作区锁` | tablet GUI 开着同一工作区；先关掉 GUI。`workspace info` 不受影响 |
| 导出成功但数据是旧的 | xlsx 组在 Excel 里改了没保存；先保存再刷新 |
| Unity 里 `Resources.LoadAll` 加载不到 | `data_output` 不在 `Assets/Resources/` 下，或 `LoadAll` 的子目录算错（数据实际在 `<data_output>/json`） |
| 手写的 `.cs` / `.json` 不见了 | 输出目录不是专用目录，导出 sync 删除了未生成的同扩展名文件；换专用子目录 |
| `找不到 tablet-cli.exe` | config 仓库里没放二进制；从发布包复制 `tablet-cli.exe`（Git 用 LFS，SVN 直接提交） |
| 分支显示为空、`vcs = "svn"` | 没装 SVN 命令行工具，或 SVN < 1.9；装 command line client tools |
| 每点一次按钮都重新编译 | 不要在按钮里 `cargo build`；二进制由 CI / 一次性构建产出 |
| 中文乱码 | bat 里 `chcp 65001`；C# 里 `StandardOutputEncoding/StandardErrorEncoding = UTF8` |

---

## 8. 版本库忽略

Git 见 `SKILL.md` §九。SVN 用同样的清单：

```text
.data/
tablet.log
tablet-crash.log
projects/*/.excel
projects/*/.tbl-cache
*.xlsx.tmp
~$*.xlsx
.~lock.*#
projects/*/gen            （如果生成物由 CI 产出）
```

```bash
svn propset svn:ignore -F .svnignore .
```

`.data/` 是每个 checkout 的本机用户数据（列宽等）+ 进程锁，**不要提交**。

**不要**把 `target/`、`.excel/`、`.tbl-cache/` 或绝对路径配置提交进 config 仓库。
