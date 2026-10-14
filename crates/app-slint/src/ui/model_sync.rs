// Slint VecModel 原地同步：逐行 set_row_data / push / remove，复用现有行元素。
//
// 直接替换整张 ModelRc 会让 slint Repeater 销毁并重建所有子元素（含 TouchArea）：
// 第一次单击触发 rebuild 后，第二次点击落在新 TouchArea 实例上 → 永远凑不齐
// double-clicked，行内「单击选中 / 双击确认」双双失效。
// 凡是要在列表行里同时支持单击与双击的地方，都必须走这里复用同一个 model。

use std::rc::Rc;

/// 把 model 内容刷新成 new_items，复用尽可能多的行（in-place set_row_data），
/// 使 slint Repeater 不销毁现有 TouchArea。
pub(crate) fn sync_vec_model<T: Clone + 'static>(model: &Rc<slint::VecModel<T>>, new_items: Vec<T>) {
    use slint::Model;
    let old_len = model.row_count();
    let new_len = new_items.len();
    let common = old_len.min(new_len);
    for (i, item) in new_items.iter().take(common).enumerate() {
        model.set_row_data(i, item.clone());
    }
    if new_len > old_len {
        for item in new_items.into_iter().skip(common) {
            model.push(item);
        }
    } else {
        for _ in new_len..old_len {
            model.remove(new_len);
        }
    }
}
