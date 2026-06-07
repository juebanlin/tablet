// 数据导出对话框：12 个格式勾选框 + 确认。
//
// TypeScript 拆双 side：
// - 客户端 → `[export.client.typescript]`，对应勾选「TypeScript (前端)」
// - Node.js → `[export.server.typescript]`，对应勾选「TypeScript (Node.js)」

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use crate::state::AppState;
use crate::{refresh, AppWindow};

pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let de = &st.data_export;
    ui_h.set_dlg_export_open(de.open);
    ui_h.set_ex_json(de.json);
    ui_h.set_ex_xml(de.xml);
    ui_h.set_ex_java(de.java);
    ui_h.set_ex_go(de.go);
    ui_h.set_ex_cpp(de.cpp);
    ui_h.set_ex_csharp_dotnet(de.csharp_dotnet);
    ui_h.set_ex_typescript_server(de.typescript_server);
    ui_h.set_ex_lua(de.lua);
    ui_h.set_ex_gdscript(de.gdscript);
    ui_h.set_ex_typescript(de.typescript);
    ui_h.set_ex_csharp_unity(de.csharp_unity);
    ui_h.set_ex_csharp_godot(de.csharp_godot);
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ex_confirm(move || {
            let Some(ui_h) = weak.upgrade() else { return; };
            {
                let mut st = s.borrow_mut();
                let d = &mut st.data_export;
                d.json = ui_h.get_ex_json();
                d.xml = ui_h.get_ex_xml();
                d.java = ui_h.get_ex_java();
                d.go = ui_h.get_ex_go();
                d.cpp = ui_h.get_ex_cpp();
                d.csharp_dotnet = ui_h.get_ex_csharp_dotnet();
                d.typescript_server = ui_h.get_ex_typescript_server();
                d.lua = ui_h.get_ex_lua();
                d.gdscript = ui_h.get_ex_gdscript();
                d.typescript = ui_h.get_ex_typescript();
                d.csharp_unity = ui_h.get_ex_csharp_unity();
                d.csharp_godot = ui_h.get_ex_csharp_godot();
                d.open = false;
            }
            run(&s);
            push(&ui_h, &s);
            refresh::after_log(&ui_h, &s);
        });
    }
    {
        // 取消时同步 Rust 端 open，否则下次刷新会把对话框重新推开（ctx fan-out 必踩）
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ex_cancel(move || {
            s.borrow_mut().data_export.open = false;
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
}

/// 执行数据导出（按 data_export 选项）。
fn run(state: &Rc<RefCell<AppState>>) {
    let opts = state.borrow().data_export.clone();
    let mut st = state.borrow_mut();
    if opts.json {
        match st.engine.export_json() {
            Ok(r) => log_result(&mut st, "JSON", &r),
            Err(e) => st.engine.log(format!("[JSON] 错误: {}", e)),
        }
    }
    if opts.xml {
        match st.engine.export_xml() {
            Ok(r) => log_result(&mut st, "XML", &r),
            Err(e) => st.engine.log(format!("[XML] 错误: {}", e)),
        }
    }
    if opts.java {
        match st.engine.export_java() {
            Ok(r) => log_result(&mut st, "Java", &r),
            Err(e) => st.engine.log(format!("[Java] 错误: {}", e)),
        }
    }
    if opts.go {
        match st.engine.export_go() {
            Ok(r) => log_result(&mut st, "Go", &r),
            Err(e) => st.engine.log(format!("[Go] 错误: {}", e)),
        }
    }
    if opts.cpp {
        match st.engine.export_cpp() {
            Ok(r) => log_result(&mut st, "C++", &r),
            Err(e) => st.engine.log(format!("[C++] 错误: {}", e)),
        }
    }
    if opts.csharp_dotnet {
        match st.engine.export_csharp_dotnet() {
            Ok(r) => log_result(&mut st, "C# (.NET)", &r),
            Err(e) => st.engine.log(format!("[C# (.NET)] 错误: {}", e)),
        }
    }
    if opts.typescript_server {
        match st.engine.export_typescript_server() {
            Ok(r) => log_result(&mut st, "TypeScript (Node.js)", &r),
            Err(e) => st.engine.log(format!("[TypeScript (Node.js)] 错误: {}", e)),
        }
    }
    if opts.lua {
        match st.engine.export_lua() {
            Ok(r) => log_result(&mut st, "Lua", &r),
            Err(e) => st.engine.log(format!("[Lua] 错误: {}", e)),
        }
    }
    if opts.gdscript {
        match st.engine.export_gdscript() {
            Ok(r) => log_result(&mut st, "GDScript", &r),
            Err(e) => st.engine.log(format!("[GDScript] 错误: {}", e)),
        }
    }
    if opts.typescript {
        match st.engine.export_typescript_client() {
            Ok(r) => log_result(&mut st, "TypeScript (前端)", &r),
            Err(e) => st.engine.log(format!("[TypeScript (前端)] 错误: {}", e)),
        }
    }
    if opts.csharp_unity {
        match st.engine.export_csharp_unity() {
            Ok(r) => log_result(&mut st, "C# (Unity)", &r),
            Err(e) => st.engine.log(format!("[C# (Unity)] 错误: {}", e)),
        }
    }
    if opts.csharp_godot {
        match st.engine.export_csharp_godot() {
            Ok(r) => log_result(&mut st, "C# (Godot)", &r),
            Err(e) => st.engine.log(format!("[C# (Godot)] 错误: {}", e)),
        }
    }
}

fn log_result(st: &mut AppState, label: &str, result: &tablet_core::export::ExportResult) {
    use tablet_core::export::FileStatus;
    st.engine.log(format!("[{}] {} 新增, {} 修改, {} 删除, {} 不变",
        label, result.added(), result.modified(), result.deleted(), result.unchanged()));
    for f in &result.files {
        match f.status {
            FileStatus::Added => st.engine.log(format!("  [新增] {}", f.path)),
            FileStatus::Modified => st.engine.log(format!("  [修改] {}", f.path)),
            FileStatus::Deleted => st.engine.log(format!("  [删除] {}", f.path)),
            FileStatus::Unchanged => {}
        }
    }
}
