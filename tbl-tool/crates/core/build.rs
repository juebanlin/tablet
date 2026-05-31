// 让 cargo 在 schemas/ 下任何文件改动时重编 tbl-core。
//
// 原因：src/test_util.rs 用 `include_str!("../schemas/*.tblschema")` 把模板嵌进二进制，
// 但 cargo 默认只看 .rs 源文件 mtime，单独改 schema 文件不会触发 core 重编，
// 导致运行时仍是旧模板（如 HeroBase.type 改 kind 后用户依然看到 type 列）。
//
// 这里逐文件 emit rerun-if-changed；目录形式只追踪目录条目本身（增删文件），
// 不追踪文件内容修改，所以必须展开。
//
// 加新文件：放进 schemas/ 即被覆盖，无需改 build.rs。

use std::fs;
use std::path::Path;

fn main() {
    let schemas_dir = Path::new("schemas");
    println!("cargo:rerun-if-changed=schemas");
    if let Ok(entries) = fs::read_dir(schemas_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
}
