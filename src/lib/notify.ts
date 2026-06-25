// OS トースト通知（#20）。ウィンドウをトレイに隠している間だけ、フェーズ境界を視覚通知でも知らせる。
// 表示中は背景色の切替と通知音で十分なので鳴らさない（邪魔にならない方針）。
//
// 送信はフロントから tauri-plugin-notification 経由で行う。隠れていても WebView は生きており
// timer-events を受け取れるため、ここで判定できる。
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import type { TimerEvent } from "./timer";

export interface Toast {
  title: string;
  body: string;
}

/**
 * 1 回の tick で届いたイベント列から、表示すべきトースト 1 件を決める純粋関数。
 *
 * - セッション完了が含まれれば完了のみ（最終休憩終了より完了を優先）。
 * - それ以外は **最後の境界**で現在フェーズを表す: 直近が作業終了→休憩へ、休憩終了→作業へ。
 *   スリープ復帰で複数境界をまとめて跨いでも、最終的に入ったフェーズだけを 1 件で知らせる。
 */
export function notificationForEvents(events: TimerEvent[]): Toast | null {
  if (events.includes("sessionFinished")) {
    return { title: "simpomo", body: "All sets complete 🎉" };
  }
  const last = events[events.length - 1];
  if (last === "workEnded") return { title: "simpomo", body: "Focus done — time for a break" };
  if (last === "breakEnded") return { title: "simpomo", body: "Break over — back to focus" };
  return null;
}

/**
 * 通知権限を確保する（未確定なら要求ダイアログを出す）。granted かを返す。
 *
 * 要求は「ユーザーが意識している可視の瞬間」だけで行う（設定で ON にした時 / 有効状態での起動時）。
 * トレイに隠れている最中に要求ダイアログで前面を奪わないため、送信側 notifyIfHidden は要求しない。
 */
export async function ensureNotificationPermission(): Promise<boolean> {
  try {
    return (await isPermissionGranted()) || (await requestPermission()) === "granted";
  } catch (e) {
    console.error("failed to request notification permission", e);
    return false;
  }
}

/**
 * メインが隠れているときだけ、フェーズ境界を OS トーストで知らせる。
 * 表示中・権限なしは何もしない（権限の**要求はしない**＝隠れている最中にダイアログを出さない）。
 */
export async function notifyIfHidden(events: TimerEvent[]): Promise<void> {
  const toast = notificationForEvents(events);
  if (!toast) return;
  try {
    if (await getCurrentWindow().isVisible()) return; // 見えているなら通知不要
    if (!(await isPermissionGranted())) return;
    sendNotification(toast);
  } catch (e) {
    // 通知が出せない環境でもタイマー本体は止めない。
    console.error("failed to send notification", e);
  }
}
