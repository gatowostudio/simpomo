// ウィンドウ操作は基本 Rust コマンド経由で行う（JS の window API より確実に効く）。
// 対応する実装は src-tauri/src/lib.rs の set_always_on_top / hide_window / open_settings。
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

export const setAlwaysOnTop = (on: boolean): Promise<void> =>
  invoke("set_always_on_top", { on });

/** 呼び出し元ウィンドウを隠す（トレイ常駐のため終了はしない）。 */
export const hideWindow = (): Promise<void> => invoke("hide_window");

// 端をつまんでのリサイズはマウス操作に紐づくため JS API を使う（capability: start-resize-dragging）。
// ResizeDirection は @tauri-apps/api から export されていないため、同じ文字列ユニオンをここで定義する。
export type ResizeDir =
  | "East"
  | "North"
  | "NorthEast"
  | "NorthWest"
  | "South"
  | "SouthEast"
  | "SouthWest"
  | "West";
export const startResize = (direction: ResizeDir): Promise<void> =>
  getCurrentWindow().startResizeDragging(direction);
