# 開発手順 — simpomo

Tauri v2（Rust）+ Svelte（Vite / TypeScript）構成のローカル開発メモ。

## 前提ツール

- **Rust** ツールチェイン（`rustup`、安定版）+ `cargo`
- **Node.js**（LTS）+ **pnpm**
- 各 OS の Tauri 前提パッケージ:
  - Windows: Microsoft C++ Build Tools / WebView2（多くの環境は既定で導入済み）
  - macOS: Xcode Command Line Tools
  - Linux: `webkit2gtk` 等（ディストリ依存）
  - 参照: https://v2.tauri.app/start/prerequisites/

## セットアップ

```sh
pnpm install          # フロントエンド依存の取得
```

## 開発（ホットリロード）

```sh
pnpm tauri dev        # Vite + Tauri を起動。UI 変更は即反映
```

## ビルド（配布物の生成）

```sh
pnpm tauri build      # 各 OS 向けインストーラ/実行ファイルを生成
```

生成物は `src-tauri/target/release/bundle/` 配下に出る（OS により .msi/.exe, .dmg, .AppImage/.deb）。

## リリース（GitHub Releases 配布）

各 OS のインストーラは GitHub Actions（`.github/workflows/release.yml`）でまとめてビルドし、
GitHub Releases に**下書き**として作成する。手元（Windows）だけでは mac/Linux 版を作れないため、
配布はこのワークフローを使う。

手順:

1. バージョンを更新する。**`package.json` の `version` が正**（`tauri.conf.json` は
   `"version": "../package.json"` で自動参照する）。`src-tauri/Cargo.toml` の `version` も
   揃えておく（クレートのバージョン。インストーラには影響しないが整合のため）。
2. 変更をコミットする。
3. （任意・推奨）`build-check` ワークフローを手動実行（Actions タブ → Build check → Run）して、
   3 OS のビルドが通ることをタグ前に確認する。
4. `package.json` と同じバージョンのタグを打って push する:
   ```sh
   git tag v0.1.0
   git push origin v0.1.0
   ```
5. `release` ワークフローがタグと `package.json` の一致を検証し、Windows / macOS(Intel+ARM) /
   Linux のインストーラをビルドして `simpomo v0.1.0` の**下書き Release** を作る
   （タグとバージョンが食い違うとビルド前に失敗する）。
6. GitHub の Releases 画面で内容を確認し、問題なければ **Publish** する。

> 現在インストーラは未署名のため、初回起動時に Windows SmartScreen / macOS Gatekeeper の警告が
> 出る。コード署名は未導入（[`spec.md`](spec.md) のオープン課題）。

## CI

- `ci.yml`: push / PR ごとに走る軽量ゲート（フロント型チェック + `cargo test` + `clippy`）。
- `build-check.yml`: 手動 / PR で 3 OS のネイティブビルドが通るか確認（Release は作らない）。
- `release.yml`: タグ push で各 OS インストーラをビルドし、下書き Release を作る。

## テスト

```sh
cd src-tauri
cargo test            # コアロジック（タイマー進行・サイクル遷移）の単体テスト
```

テスト方針は CLAUDE.md「テスト方針」を正とする（コアロジックのみ単体テスト、UI は手動確認）。

## ディレクトリ構成（想定）

```
simpomo/
├─ src/              # Svelte フロントエンド（UI）
├─ src-tauri/        # Rust バックエンド（コアロジック・ウィンドウ制御）
│  └─ src/           # タイマー進行・サイクル状態など
├─ docs/             # 仕様・設計判断
└─ ...
```

## 公開リポジトリでの注意（機密の扱い）

- 署名鍵 / updater 秘密鍵 / `.env` は **絶対にコミットしない**（`.gitignore` 済み）。
- 将来コード署名やオートアップデータを導入する場合、鍵はリポジトリ外（OS のキーチェーン /
  GitHub の **Secrets**）に置き、ワークフローの `env` で渡す。鍵そのものはファイルに残さない。
- 同梱する通知音はロイヤリティフリー / 自作のもののみ。出典・ライセンスを記録する
  （現状は Web Audio 合成のため同梱音源なし。[`credits.md`](credits.md) 参照）。
