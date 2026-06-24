// ウィンドウ操作は Rust コマンド経由で行う（JS の window API より確実に効く）。
// 対応する実装は src-tauri/src/lib.rs の set_always_on_top / hide_window / open_settings。
import { invoke } from "@tauri-apps/api/core";

export const setAlwaysOnTop = (on: boolean): Promise<void> =>
  invoke("set_always_on_top", { on });

/** 呼び出し元ウィンドウを隠す（トレイ常駐のため終了はしない）。 */
export const hideWindow = (): Promise<void> => invoke("hide_window");
