import { mount } from "svelte";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./app.css";
import App from "./App.svelte";
import Settings from "./Settings.svelte";
import { SETTINGS_WINDOW_LABEL } from "./lib/settings";

// 1 つの index.html を両ウィンドウで読み込み、ウィンドウのラベルで表示を切り替える
// （メイン = タイマー / settings = 設定。別ウィンドウ方式は ADR-0002）。
const Component =
  getCurrentWindow().label === SETTINGS_WINDOW_LABEL ? Settings : App;

const app = mount(Component, {
  target: document.getElementById("app")!,
});

export default app;
