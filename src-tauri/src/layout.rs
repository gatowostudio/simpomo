//! ウィンドウの表示位置の算出・適用。
//!
//! サイズと位置はユーザーが端でリサイズ/ドラッグし、tauri-plugin-window-state が永続化/復元する。
//! 本モジュールが担うのは 2 つ:
//! - 「初期/リセット位置（どの隅に出すか）」の算出と適用（初回起動・Position 設定変更時）。
//! - 画面外に取り残されたウィンドウを作業領域へ引き戻す救済（解像度/スケール/モニタ変更時）。
//!
//! 位置はモニタの**作業領域（work area: タスクバー等を除いた領域）**を基準に算出する。これにより
//! 下側の隅（BottomLeft/BottomRight）でもタスクバーに重ならない（#15）。

use serde::{Deserialize, Serialize};
use tauri::{LogicalPosition, Monitor, WebviewWindow};

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

/// 対象モニタを返す。ウィンドウが今載っているモニタ（current）を優先し、取得できなければ
/// プライマリへフォールバックする（モニタを取り外した直後など current が None になりうる、#17）。
fn target_monitor(window: &WebviewWindow) -> tauri::Result<Option<Monitor>> {
    if let Some(monitor) = window.current_monitor()? {
        return Ok(Some(monitor));
    }
    window.primary_monitor()
}

/// モニタの作業領域を論理ピクセルの `(原点(x,y), サイズ(幅,高さ))` で返す。
/// 作業領域はタスクバー等を除いた領域なので、ここを基準に置けば隅でも被らない（#15）。
fn work_area_logical(monitor: &Monitor) -> ((f64, f64), (f64, f64)) {
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let origin = area.position.to_logical::<f64>(scale);
    let size = area.size.to_logical::<f64>(scale);
    ((origin.x, origin.y), (size.width, size.height))
}

/// 現在のウィンドウサイズのまま、指定した隅へ移動する。
pub fn apply_position(window: &WebviewWindow, corner: Corner) -> tauri::Result<()> {
    // 最大化中は位置を触らない（最大化解除やはみ出しを避ける）。
    if window.is_maximized().unwrap_or(false) {
        return Ok(());
    }
    // モニタが取得できないとき（稀）は位置はそのまま。
    let Some(monitor) = target_monitor(window)? else {
        return Ok(());
    };
    let scale = monitor.scale_factor();
    let (origin, area) = work_area_logical(&monitor);
    let win = window.outer_size()?.to_logical::<f64>(scale);
    let (x, y) = corner_offset(origin, area, (win.width, win.height), corner);
    // ウィンドウが作業領域より大きい/別モニタ構成等でも、左上が必ず領域内に残るようクランプする。
    let (x, y) = clamp_to_area(origin, area, (win.width, win.height), (x, y));
    window.set_position(LogicalPosition::new(x, y))?;
    Ok(())
}

/// 現在位置を保ったまま、ウィンドウが作業領域内に収まるよう引き戻す（#17）。
/// 解像度/スケール/モニタ構成の変更で画面外に取り残されたウィンドウの救済に使う。
/// 隅へは寄せず、はみ出した分だけ最小移動する（ユーザーが置いた位置を尊重する）。
pub fn ensure_on_screen(window: &WebviewWindow) -> tauri::Result<()> {
    if window.is_maximized().unwrap_or(false) {
        return Ok(());
    }
    let Some(monitor) = target_monitor(window)? else {
        return Ok(());
    };
    let scale = monitor.scale_factor();
    let (origin, area) = work_area_logical(&monitor);
    let win = window.outer_size()?.to_logical::<f64>(scale);
    let pos = window.outer_position()?.to_logical::<f64>(scale);
    let (x, y) = clamp_to_area(origin, area, (win.width, win.height), (pos.x, pos.y));
    // 既に領域内なら動かさない（無用な Moved イベント＝位置の再保存とジッタを避ける）。
    if (x - pos.x).abs() > 0.5 || (y - pos.y).abs() > 0.5 {
        window.set_position(LogicalPosition::new(x, y))?;
    }
    Ok(())
}

/// ウィンドウ左上が作業領域内に収まるよう座標をクランプする純粋関数。
fn clamp_to_area(origin: (f64, f64), area: (f64, f64), win: (f64, f64), pos: (f64, f64)) -> (f64, f64) {
    let max_x = origin.0 + (area.0 - win.0).max(0.0);
    let max_y = origin.1 + (area.1 - win.1).max(0.0);
    (pos.0.clamp(origin.0, max_x), pos.1.clamp(origin.1, max_y))
}

/// 作業領域の原点 `origin`・論理サイズ `area` 上で、ウィンドウ `win`(幅,高さ) を四隅に置く
/// ための論理座標を返す純粋関数（Monitor 非依存にしてテスト可能にしてある）。
///
/// `area` はタスクバー等を除いた作業領域サイズを渡す想定（呼び出し側 `work_area_logical`）。
/// よって下側の隅でもタスクバーに重ならない（#15）。
fn corner_offset(origin: (f64, f64), area: (f64, f64), win: (f64, f64), corner: Corner) -> (f64, f64) {
    let (ox, oy) = origin;
    let (aw, ah) = area;
    let (ww, wh) = win;

    let right = ox + aw - ww - MARGIN;
    let left = ox + MARGIN;
    let top = oy + MARGIN;
    let bottom = oy + ah - wh - MARGIN;

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

    // 単一モニタの作業領域 1000x800、原点(0,0)、ウィンドウ 200x100、マージン 16 を基準にする。
    const ORIGIN: (f64, f64) = (0.0, 0.0);
    const AREA: (f64, f64) = (1000.0, 800.0);
    const WIN: (f64, f64) = (200.0, 100.0);

    #[test]
    fn top_left_is_margin_from_origin() {
        assert_eq!(corner_offset(ORIGIN, AREA, WIN, Corner::TopLeft), (16.0, 16.0));
    }

    #[test]
    fn top_right_hugs_right_edge() {
        // x = 0 + 1000 - 200 - 16 = 784, y = 16
        assert_eq!(corner_offset(ORIGIN, AREA, WIN, Corner::TopRight), (784.0, 16.0));
    }

    #[test]
    fn bottom_right_hugs_bottom_right() {
        // x = 784, y = 0 + 800 - 100 - 16 = 684
        assert_eq!(corner_offset(ORIGIN, AREA, WIN, Corner::BottomRight), (784.0, 684.0));
    }

    #[test]
    fn bottom_left_hugs_bottom_left() {
        assert_eq!(corner_offset(ORIGIN, AREA, WIN, Corner::BottomLeft), (16.0, 684.0));
    }

    #[test]
    fn bottom_corner_uses_work_area_not_full_screen() {
        // 全画面 1000x800 だがタスクバー 40px を除いた作業領域 800-40=760 を渡すと、
        // 下隅はタスクバーの上（y=760-100-16=644）に収まる（#15: タスクバー重なり対策）。
        let work = (1000.0, 760.0);
        assert_eq!(corner_offset(ORIGIN, work, WIN, Corner::BottomRight), (784.0, 644.0));
    }

    #[test]
    fn respects_nonzero_monitor_origin() {
        // セカンダリモニタが主モニタの左側（負の原点）にあるケース。
        let origin = (-1920.0, 0.0);
        let area = (1920.0, 1080.0);
        let win = (240.0, 150.0);
        // TopRight: x = -1920 + 1920 - 240 - 16 = -256, y = 16
        assert_eq!(corner_offset(origin, area, win, Corner::TopRight), (-256.0, 16.0));
        // TopLeft: x = -1920 + 16 = -1904
        assert_eq!(corner_offset(origin, area, win, Corner::TopLeft), (-1904.0, 16.0));
    }

    #[test]
    fn clamp_keeps_normal_window_in_place() {
        // 作業領域に収まるサイズなら座標はそのまま。
        assert_eq!(clamp_to_area(ORIGIN, AREA, WIN, (784.0, 16.0)), (784.0, 16.0));
    }

    #[test]
    fn clamp_pulls_offscreen_window_back() {
        // 作業領域より大きいウィンドウは左上が原点にクランプされる（画面外に出ない）。
        let big = (1200.0, 900.0); // AREA(1000x800) より大きい
        assert_eq!(clamp_to_area(ORIGIN, AREA, big, (-200.0, -50.0)), (0.0, 0.0));
    }

    #[test]
    fn clamp_pulls_window_in_from_bottom_right() {
        // 右下に飛び出したウィンドウを作業領域内へ引き戻す（#17 の救済ロジック）。
        // max_x = 1000-200 = 800, max_y = 800-100 = 700
        assert_eq!(clamp_to_area(ORIGIN, AREA, WIN, (5000.0, 5000.0)), (800.0, 700.0));
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
