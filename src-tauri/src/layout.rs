//! ウィンドウのサイズプリセットと表示位置（画面の隅）の算出・適用（#4）。
//!
//! サイズは論理ピクセルの固定プリセットで持つ（DPI 差は Tauri が吸収する）。様々な解像度に
//! 対応できるよう段階を多めに用意し、ユーザーが画面に合うものを選べるようにする（殿の方針）。
//! 位置はモニタの論理サイズと原点から四隅を算出する。設定での選択・永続化は #5 で本モジュールを使う。

use serde::{Deserialize, Serialize};
use tauri::{LogicalPosition, LogicalSize, WebviewWindow};

/// 画面の隅からのマージン（論理ピクセル）。
const MARGIN: f64 = 16.0;

/// ウィンドウサイズのプリセット（論理ピクセル、約 1.6:1）。
///
/// 低解像度ノート〜高解像度大画面まで選べるよう 6 段階。既定は「邪魔にならないが
/// 見づらくない」`Medium`。`tauri.conf.json` の初期 width/height はこの Medium と一致させてある
/// （visible:false → setup で再適用するが、表示確定前のサイズを近づけてチラつきを抑えるため）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SizePreset {
    XSmall,
    Small,
    Medium,
    Large,
    XLarge,
    XXLarge,
}

impl SizePreset {
    /// 論理ピクセルでの (幅, 高さ)。
    pub fn logical(self) -> (f64, f64) {
        match self {
            SizePreset::XSmall => (160.0, 100.0),
            SizePreset::Small => (200.0, 125.0),
            SizePreset::Medium => (240.0, 150.0),
            SizePreset::Large => (300.0, 188.0),
            SizePreset::XLarge => (380.0, 238.0),
            SizePreset::XXLarge => (480.0, 300.0),
        }
    }
}

/// 表示位置（画面の四隅）。既定は右上（spec）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Corner {
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
}

/// プリセットと隅からサイズ・位置を算出してウィンドウに適用する。
pub fn apply(window: &WebviewWindow, size: SizePreset, corner: Corner) -> tauri::Result<()> {
    let win = size.logical();
    window.set_size(LogicalSize::new(win.0, win.1))?;
    // モニタが取得できないとき（稀）は位置はそのまま。サイズは適用済み。
    if let Some(monitor) = window.current_monitor()? {
        let scale = monitor.scale_factor();
        let screen = monitor.size().to_logical::<f64>(scale);
        let origin = monitor.position().to_logical::<f64>(scale);
        let (x, y) = corner_offset(
            (origin.x, origin.y),
            (screen.width, screen.height),
            win,
            corner,
        );
        window.set_position(LogicalPosition::new(x, y))?;
    }
    Ok(())
}

/// モニタ原点 `origin`・論理サイズ `screen` 上で、ウィンドウ `win`(幅,高さ) を四隅に置く
/// ための論理座標を返す純粋関数（Monitor 非依存にしてテスト可能にしてある）。
///
/// 注: `screen` はタスクバー等の作業領域を除かない全画面サイズを渡す想定。既定の右上は
/// 通常下部にあるタスクバーと干渉しないため実用上問題ない。下側の隅はタスクバーと重なりうる。
/// 下側の隅を設定 UI で選べるようにする #5 の時点で work area 対応を検討する。
fn corner_offset(origin: (f64, f64), screen: (f64, f64), win: (f64, f64), corner: Corner) -> (f64, f64) {
    let (ox, oy) = origin;
    let (sw, sh) = screen;
    let (ww, wh) = win;

    let right = ox + sw - ww - MARGIN;
    let left = ox + MARGIN;
    let top = oy + MARGIN;
    let bottom = oy + sh - wh - MARGIN;

    match corner {
        Corner::TopRight => (right, top),
        Corner::TopLeft => (left, top),
        Corner::BottomRight => (right, bottom),
        Corner::BottomLeft => (left, bottom),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 単一モニタ 1000x800、原点(0,0)、ウィンドウ 200x100、マージン 16 を基準にする。
    const ORIGIN: (f64, f64) = (0.0, 0.0);
    const SCREEN: (f64, f64) = (1000.0, 800.0);
    const WIN: (f64, f64) = (200.0, 100.0);

    #[test]
    fn top_left_is_margin_from_origin() {
        assert_eq!(corner_offset(ORIGIN, SCREEN, WIN, Corner::TopLeft), (16.0, 16.0));
    }

    #[test]
    fn top_right_hugs_right_edge() {
        // x = 0 + 1000 - 200 - 16 = 784, y = 16
        assert_eq!(corner_offset(ORIGIN, SCREEN, WIN, Corner::TopRight), (784.0, 16.0));
    }

    #[test]
    fn bottom_right_hugs_bottom_right() {
        // x = 784, y = 0 + 800 - 100 - 16 = 684
        assert_eq!(
            corner_offset(ORIGIN, SCREEN, WIN, Corner::BottomRight),
            (784.0, 684.0)
        );
    }

    #[test]
    fn bottom_left_hugs_bottom_left() {
        assert_eq!(
            corner_offset(ORIGIN, SCREEN, WIN, Corner::BottomLeft),
            (16.0, 684.0)
        );
    }

    #[test]
    fn respects_nonzero_monitor_origin() {
        // セカンダリモニタが主モニタの左側（負の原点）にあるケース。
        let origin = (-1920.0, 0.0);
        let screen = (1920.0, 1080.0);
        let win = (240.0, 150.0);
        // TopRight: x = -1920 + 1920 - 240 - 16 = -256, y = 16
        assert_eq!(corner_offset(origin, screen, win, Corner::TopRight), (-256.0, 16.0));
        // TopLeft: x = -1920 + 16 = -1904
        assert_eq!(corner_offset(origin, screen, win, Corner::TopLeft), (-1904.0, 16.0));
    }

    #[test]
    fn presets_are_ordered_and_positive() {
        // 段階が単調増加し、全て正であること（破綻したプリセットを混入させない）。
        let order = [
            SizePreset::XSmall,
            SizePreset::Small,
            SizePreset::Medium,
            SizePreset::Large,
            SizePreset::XLarge,
            SizePreset::XXLarge,
        ];
        let mut prev = (0.0, 0.0);
        for p in order {
            let (w, h) = p.logical();
            assert!(w > prev.0 && h > prev.1, "{p:?} is not larger than previous");
            prev = (w, h);
        }
    }

    #[test]
    fn size_preset_deserializes_from_lowercase() {
        // フロント(#5)から送る文字列契約を固定する。
        assert_eq!(
            serde_json::from_str::<SizePreset>("\"medium\"").unwrap(),
            SizePreset::Medium
        );
        assert_eq!(
            serde_json::from_str::<SizePreset>("\"xxlarge\"").unwrap(),
            SizePreset::XXLarge
        );
    }

    #[test]
    fn corner_deserializes_from_camel_case() {
        assert_eq!(
            serde_json::from_str::<Corner>("\"topRight\"").unwrap(),
            Corner::TopRight
        );
        assert_eq!(
            serde_json::from_str::<Corner>("\"bottomLeft\"").unwrap(),
            Corner::BottomLeft
        );
    }
}
