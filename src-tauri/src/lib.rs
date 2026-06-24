pub mod layout;
pub mod settings;
pub mod timer;

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

use layout::Corner;
use settings::AppSettings;

use timer::{Config, Status, Timer, TimerEvent, TimerSnapshot};

/// タイマー状態と、tick 駆動スレッドを起こすための条件変数をまとめた共有状態。
///
/// 並行性の不変条件: `timer` のロックを保持している間は panic しうる処理を呼ばない
/// （Timer 操作は純粋で panic せず、emit はエラーを握りつぶす）。よって Mutex は poison せず、
/// `lock().unwrap()` は安全。
struct Shared {
    timer: Mutex<Timer>,
    /// 稼働状態の変化を tick スレッドへ通知する。start で park 中のスレッドを起こす。
    wake: Condvar,
}

type SharedState = Arc<Shared>;

// イベント名はフロント（timer.ts / settings.ts）と一致させる文字列契約。
// 単純な英数字 + ハイフンにする（":" や "/" を含む名前はトラブルの元になりうる）。
const EVENT_SNAPSHOT: &str = "timer-snapshot";
const EVENT_TIMER_EVENTS: &str = "timer-events";
/// 設定変更を全ウィンドウへ通知する（snapshot に乗らない設定を #6 等が購読する）。
const EVENT_SETTINGS: &str = "settings-changed";

/// ウィンドウラベル。フロント（main.ts / settings.ts）と一致させる文字列契約。
const MAIN_LABEL: &str = "main";
const SETTINGS_LABEL: &str = "settings";

// snapshot / フェーズ境界イベントはブロードキャストで送る（フロントの listen() に確実に届く）。
// 設定ウィンドウはこれらを購読しないので二重処理にはならない。
fn emit_snapshot(app: &AppHandle, snapshot: TimerSnapshot) {
    // 送信失敗（ウィンドウ破棄直後など）は致命ではないので握りつぶす。
    let _ = app.emit(EVENT_SNAPSHOT, snapshot);
}

fn emit_events(app: &AppHandle, events: &[TimerEvent]) {
    if !events.is_empty() {
        let _ = app.emit(EVENT_TIMER_EVENTS, events);
    }
}

// コマンドは状態を「戻り値」では返さない。更新は必ず emit(EVENT_SNAPSHOT) の単一経路で流す
// （戻り値と emit の二重経路だと到着順でフロント表示が巻き戻るレースが起きるため）。
// mutate と emit は同一ロック区間で行い、tick スレッドの emit と順序が入れ替わらないようにする。

#[tauri::command]
fn timer_snapshot(state: State<'_, SharedState>) -> TimerSnapshot {
    state.timer.lock().unwrap().snapshot()
}

#[tauri::command]
fn timer_start(app: AppHandle, state: State<'_, SharedState>) {
    {
        let mut timer = state.timer.lock().unwrap();
        timer.start();
        emit_snapshot(&app, timer.snapshot());
    }
    state.wake.notify_all(); // park 中の tick スレッドを起こす
}

#[tauri::command]
fn timer_pause(app: AppHandle, state: State<'_, SharedState>) {
    {
        let mut timer = state.timer.lock().unwrap();
        timer.pause();
        emit_snapshot(&app, timer.snapshot());
    }
    state.wake.notify_all();
}

#[tauri::command]
fn timer_reset(app: AppHandle, state: State<'_, SharedState>) {
    {
        let mut timer = state.timer.lock().unwrap();
        timer.reset();
        emit_snapshot(&app, timer.snapshot());
    }
    state.wake.notify_all();
}

#[tauri::command]
fn timer_skip(app: AppHandle, state: State<'_, SharedState>) {
    {
        let mut timer = state.timer.lock().unwrap();
        // 手動 skip はフェーズ境界イベントを出さない（自分で送ったのに通知音が鳴る違和感を避ける、殿の判断）。
        // 状態変化は snapshot で反映する。時間切れの遷移（tick）だけが通知音を鳴らす。
        let _ = timer.skip();
        emit_snapshot(&app, timer.snapshot());
    }
    state.wake.notify_all();
}

/// メインウィンドウを指定の隅へ移動する（設定保存・起動時の共通経路）。サイズは触らない。
fn apply_main_position(app: &AppHandle, corner: Corner) -> Result<(), String> {
    let main = app
        .get_webview_window(MAIN_LABEL)
        .ok_or_else(|| "main window not found".to_string())?;
    layout::apply_position(&main, corner).map_err(|e| e.to_string())
}

/// 現在の永続化設定を返す（設定ウィンドウの初期表示用）。
#[tauri::command]
fn get_settings(app: AppHandle) -> AppSettings {
    settings::load(&app)
}

/// 設定を保存し、タイマーとウィンドウへ反映する（設定ウィンドウから呼ぶ）。
#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<'_, SharedState>,
    settings: AppSettings,
) -> Result<(), String> {
    // Rust が値検証の権威。保存値=実効値になるよう先に正規化する。
    let settings = settings.sanitized();
    settings::save(&app, &settings)?;
    // タイマーへ反映（Idle なら新時間で reset、稼働中はセッション維持し次フェーズ以降）。
    {
        let mut timer = state.timer.lock().unwrap();
        timer.set_config(settings.to_config());
        emit_snapshot(&app, timer.snapshot());
    }
    state.wake.notify_all();
    let corner = settings.corner;
    // 設定変更を通知（snapshot に乗らない設定＝通知音/BGM/背景色を App が購読して反映）。
    let _ = app.emit(EVENT_SETTINGS, settings);
    // メインウィンドウを指定の隅へ移動（失敗はフロントへ伝播）。サイズは触らない。
    apply_main_position(&app, corner)
}

/// 設定ウィンドウを表示する（conf で定義済みの非表示ウィンドウを見せる）。別ウィンドウ方式（ADR-0002）。
#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> {
    let win = app
        .get_webview_window(SETTINGS_LABEL)
        .ok_or_else(|| "settings window not found".to_string())?;
    win.show().map_err(|e| e.to_string())?;
    let _ = win.unminimize();
    let _ = win.set_focus();
    Ok(())
}

/// 呼び出し元ウィンドウの最前面表示を切り替える（メインのピンボタンから）。
/// JS の window API ではなく Rust から直接制御し、確実に効くようにする。
#[tauri::command]
fn set_always_on_top(window: tauri::WebviewWindow, on: bool) -> Result<(), String> {
    window.set_always_on_top(on).map_err(|e| e.to_string())
}

/// 呼び出し元ウィンドウを隠す（メインの ✕ / 設定の Close から）。トレイ常駐のため終了はしない。
#[tauri::command]
fn hide_window(window: tauri::WebviewWindow) -> Result<(), String> {
    window.hide().map_err(|e| e.to_string())
}

/// メインウィンドウを表示して前面に出す（トレイから呼ぶ）。
fn show_main(app: &AppHandle) {
    if let Some(main) = app.get_webview_window(MAIN_LABEL) {
        let _ = main.show();
        let _ = main.set_focus();
    }
}

/// 起動時に永続化設定を読み込み、タイマーとウィンドウへ適用してメインを表示する。
fn apply_loaded_settings(app: &AppHandle, state: &SharedState) {
    let loaded = settings::load(app);
    {
        let mut timer = state.timer.lock().unwrap();
        timer.set_config(loaded.to_config());
    }
    // ウィンドウは conf で visible:false。サイズは window-state プラグインが復元済み。
    // 位置を確定してから表示し、既定位置への一瞬のジャンプ（チラつき）を避ける。
    if let Some(main) = app.get_webview_window(MAIN_LABEL) {
        let _ = layout::apply_position(&main, loaded.corner);
        let _ = main.show();
    }
}

/// システムトレイを構築する。左クリックでウィンドウ表示、メニューで表示/終了（トレイ常駐）。
fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

    let mut builder = TrayIconBuilder::with_id("main").tooltip("simpomo");
    // アイコンは conf で定義済みだが、念のため取得できたときだけ設定する（panic を避ける）。
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// 実時間の駆動（ADR-0002）。Rust 側のバックグラウンドスレッドが単調時計（`Instant`）の
/// 差分で実経過秒を算出して `tick` に渡す。固定 `tick(1)` のドリフトとスリープ復帰のずれを避ける。
///
/// アイドル時 CPU を最小化するため、非稼働中（Idle/Paused）は条件変数で park し、
/// CPU を消費しない。start の `notify_all` で起き、稼働中のみ毎秒 tick して emit する。
fn spawn_tick_loop(app: AppHandle, state: SharedState) {
    std::thread::spawn(move || {
        let mut last = Instant::now();
        let mut timer = state.timer.lock().unwrap();
        loop {
            // 非稼働中は wake されるまで park（この間ロックは解放され CPU はゼロ）。
            while timer.snapshot().status != Status::Running {
                timer = state.wake.wait(timer).unwrap();
                // 稼働再開の起点を切り直す（park 中の経過を取り込んで過剰進行するのを防ぐ）。
                last = Instant::now();
            }
            // 稼働中: 最大 1 秒待つ。pause/skip 等の通知で早く起きる。
            let (guard, _timeout) = state
                .wake
                .wait_timeout(timer, Duration::from_secs(1))
                .unwrap();
            timer = guard;
            if timer.snapshot().status != Status::Running {
                continue; // 待機中に停止された → park へ戻る
            }
            let now = Instant::now();
            // スリープ復帰で巨大になりうるので飽和させる（u32 への wrap を防ぐ）。
            let elapsed = now.duration_since(last).as_secs().min(u32::MAX as u64) as u32;
            // 端数（1 秒未満）は次回へ持ち越し、長期ドリフトを防ぐ。
            last += Duration::from_secs(elapsed as u64);
            if elapsed == 0 {
                continue;
            }
            let events = timer.tick(elapsed);
            let snapshot = timer.snapshot();
            // ロック保持中に emit し、コマンド由来の emit と順序が入れ替わらないようにする。
            emit_events(&app, &events);
            emit_snapshot(&app, snapshot);
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state: SharedState = Arc::new(Shared {
        timer: Mutex::new(Timer::new(Config::default())),
        wake: Condvar::new(),
    });

    tauri::Builder::default()
        // 「更新を確認」でリリースページを既定ブラウザで開く。
        .plugin(tauri_plugin_opener::init())
        // ウィンドウのサイズ/最大化状態を永続化・復元する（位置は Corner 設定で別途決めるので除外）。
        .plugin(
            tauri_plugin_window_state::Builder::default()
                // サイズのみ永続化（位置は Corner 設定で決める。最大化は復元しない＝位置適用との競合回避）。
                .with_state_flags(StateFlags::SIZE)
                .build(),
        )
        .manage(state.clone())
        .setup(move |app| {
            let handle = app.handle();
            setup_tray(handle)?;
            apply_loaded_settings(handle, &state);
            spawn_tick_loop(handle.clone(), state.clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            // トレイ常駐: ✕ や Alt+F4 では終了せず非表示にする（終了はトレイメニューから）。
            // 設定ウィンドウも破棄せず隠す（conf で定義済みなので再表示で開き直せる）。
            if let WindowEvent::CloseRequested { api, .. } = event {
                let label = window.label();
                if label == MAIN_LABEL || label == SETTINGS_LABEL {
                    api.prevent_close();
                    // 隠す前にサイズを保存しておく（強制終了されても直近サイズが残るように）。
                    let _ = window.app_handle().save_window_state(StateFlags::SIZE);
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            timer_snapshot,
            timer_start,
            timer_pause,
            timer_reset,
            timer_skip,
            get_settings,
            save_settings,
            open_settings,
            set_always_on_top,
            hide_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
