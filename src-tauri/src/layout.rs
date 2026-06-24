//! ウィンドウの表示位置（画面の隅）の算出・適用。
//!
//! サイズはユーザーが端をつまんでリサイズし、tauri-plugin-window-state が永続化/復元する。
//! 本モジュールは「初期位置（どの隅に出すか）」だけを担う。位置はモニタの論理サイズと原点、
//! および現在のウィンドウサイズから算出する。

use serde::{Deserialize, Serialize};
use tauri::{LogicalPosition, WebviewWindow};

/// 画面の隅からのマージン（論理ピクセル）。
const MARGIN: f64 = 16.0;

/// 表示位置（画面の四隅）。既定は右上（spec）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Corner {
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
}

/// 現在のウィンドウサイズのまま、指定した隅へ移動する。
pub fn apply_position(window: &WebviewWindow, corner: Corner) -> tauri::Result<()> {
    // 最大化中は位置を触らない（最大化解除やはみ出しを避ける）。
    if window.is_maximized().unwrap_or(false) {
        return Ok(());
    }
    // モニタが取得できないとき（稀）は位置はそのまま。
    let Some(monitor) = window.current_monitor()? else {
        return Ok(());
    };
    let scale = monitor.scale_factor();
    let screen = monitor.size().to_logical::<f64>(scale);
    let origin = monitor.position().to_logical::<f64>(scale);
    let win = window.outer_size()?.to_logical::<f64>(scale);
    let (x, y) = corner_offset(
        (origin.x, origin.y),
        (screen.width, screen.height),
        (win.width, win.height),
        corner,
    );
    // ウィンドウが画面より大きい/別モニタ構成等でも、左上が必ず画面内に残るようクランプする。
    let (x, y) = clamp_to_screen(
        (origin.x, origin.y),
        (screen.width, screen.height),
        (win.width, win.height),
        (x, y),
    );
    window.set_position(LogicalPosition::new(x, y))?;
    Ok(())
}

/// ウィンドウ左上が画面内（モニタ範囲）に収まるよう座標をクランプする純粋関数。
fn clamp_to_screen(origin: (f64, f64), screen: (f64, f64), win: (f64, f64), pos: (f64, f64)) -> (f64, f64) {
    let max_x = origin.0 + (screen.0 - win.0).max(0.0);
    let max_y = origin.1 + (screen.1 - win.1).max(0.0);
    (
        pos.0.clamp(origin.0, max_x),
        pos.1.clamp(origin.1, max_y),
    )
}

/// モニタ原点 `origin`・論理サイズ `screen` 上で、ウィンドウ `win`(幅,高さ) を四隅に置く
/// ための論理座標を返す純粋関数（Monitor 非依存にしてテスト可能にしてある）。
///
/// 注: `screen` はタスクバー等の作業領域を除かない全画面サイズを渡す想定。既定の右上は
/// 通常下部にあるタスクバーと干渉しないため実用上問題ない。下側の隅はタスクバーと重なりうる
/// （将来 work area 対応の余地）。
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
    fn clamp_keeps_normal_window_in_place() {
        // 画面に収まるサイズなら座標はそのまま。
        assert_eq!(
            clamp_to_screen(ORIGIN, SCREEN, WIN, (784.0, 16.0)),
            (784.0, 16.0)
        );
    }

    #[test]
    fn clamp_pulls_offscreen_window_back() {
        // 画面より大きいウィンドウは左上が原点にクランプされる（画面外に出ない）。
        let big = (1200.0, 900.0); // SCREEN(1000x800) より大きい
        assert_eq!(clamp_to_screen(ORIGIN, SCREEN, big, (-200.0, -50.0)), (0.0, 0.0));
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
