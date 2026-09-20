# TrigA（Automation Manager）

Rust + Tauri 製の常駐型デスクトップ自動化アプリです。GUIで自動化処理を登録しておくと、フォルダ／ファイルの変化・指定時刻・指定間隔・手動実行をきっかけに、コマンドやPythonコードを自動実行します。

## 現在の機能

- TrigAデスクトップウィンドウ（Vanilla JavaScript管理UI、日英切替・6テーマ対応）
- SQLiteによる自動化定義・実行履歴の保存（`data/automation.db`）
- ループバックREST API（`127.0.0.1:47821`、外部AIエージェント連携用）
- 手動実行（無効化中でも実行可）
- アクション: Python / コマンド / シェル / コピー / 移動
- 条件: ファイル名 / フォルダ名 / 拡張子 / パス / 存在確認
- タイムアウト付き外部プロセス実行（子プロセスの取り残しなし）
- コマンド許可リスト・許可パス検証
- OSファイルイベント駆動の監視（フォルダ作成・ファイル作成・ファイル変更、移動投入・原子保存にも対応）
- インターバルトリガー（`start_immediately`対応、Tokioタイマー駆動・取りこぼしスキップ）
- スケジュールトリガー（cron `0 9 * * *` または `HH:MM`＋タイムゾーン対応）
- 重複ポリシー既定`skip`、ミスファイア`skip`（大量の追いつき実行なし）
- 重複排除ウィンドウ、条件不一致スキップ、`skipped`履歴記録
- Tauriコマンド＋REST（`/api/jobs`、`/api/history?limit=20`、`/api/schema/automation`）
- 履歴画面に発火対象ファイル名（Target列）表示、5秒ごとの自動更新

## 開発

```bash
cargo test --manifest-path src-tauri/Cargo.toml
node --test src/js/*.test.js
cargo check --manifest-path src-tauri/Cargo.toml
```

Tauriアプリの起動には、Tauriに必要なLinux WebKitGTK開発パッケージを先にインストールしてから実行します。

```bash
cargo run --manifest-path src-tauri/Cargo.toml
```

## 本番ビルド
```bash
cargo install tauri-cli
cargo tauri build
```

アプリは起動時に`data/automation.db`を作成し、Tauriウィンドウからフロントエンドを提供します。REST APIはループバックのみで公開されます。

## 画面

- **ダッシュボード**: 自動化数・有効数・成功／失敗件数、最近の実行（ポイントカラー付きカード）
- **自動化**: 一覧（実行・編集・有効化／無効化・削除）、有効は緑・無効は赤で表示
- **履歴**: 最新20件（自動化・トリガー・状態・対象・開始時刻）
- **設定**: テーマ切替（ライトブルー／ライト／ダーク／ライトオレンジ／ライトピンク／ダークトーキョー）、言語切替（英語／日本語）

テーマと言語の選択はアプリ内に保存され、次回起動時も維持されます。

## 外部AIエージェント連携

`Skill/TrigA-skill/SKILL.md`に、REST API経由で自動化の作成・実行・状態確認を行う手順をまとめています。自然言語の依頼（例：「InputフォルダにCSVが入ったらPythonで処理してCompletedへ移動して」）を自動化定義に変換して`POST /api/automations`で登録できます。

## 補足

システムトレイ、OS自動起動、ワーカープール／ジョブテーブルの本格化は今後の拡張予定です。インターバル・定期実行はイベント駆動（ポーリングなし）で動作します。現在の契約は`src-tauri/tests/interval_scheduled_mvp.rs`と`src-tauri/tests/file_trigger_history.rs`を参照してください。
