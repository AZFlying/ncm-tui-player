mod daily_lists_panel;
mod lyric_panel;
mod playlist_panel;
mod songlist_candidates_panel;

pub use daily_lists_panel::*;
pub use lyric_panel::*;
pub use playlist_panel::*;
pub use songlist_candidates_panel::*;

#[derive(PartialEq)]
/// 面板是否被聚焦，聚焦在面板整体还是面板内
pub enum PanelFocusedStatus {
    Outside,
    Inside,
    Nop,
}

/// 光标居中（首尾除外）的滚动 offset：中间段居中，接近首尾时取消居中
fn centered_offset(selected: usize, row_count: usize, visible_row_count: usize) -> usize {
    selected
        .saturating_sub(visible_row_count / 2)
        .min(row_count.saturating_sub(visible_row_count))
}

#[cfg(test)]
mod tests {
    use super::centered_offset;

    #[test]
    fn centers_selection_except_near_boundaries() {
        assert_eq!(centered_offset(2, 20, 7), 0);
        assert_eq!(centered_offset(10, 20, 7), 7);
        assert_eq!(centered_offset(18, 20, 7), 13);
        assert_eq!(centered_offset(3, 5, 7), 0);
        assert_eq!(centered_offset(10, 20, 9), 6);
        assert_eq!(centered_offset(5, 20, 8), 1);
    }
}
