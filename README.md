# TrigA（Automation Manager）

Rust + Tauri 製の常駐型デスクトップ自動化アプリです。GUIで自動化処理を登録しておくと、フォルダ／ファイルの変化・指定時刻・指定間隔・手動実行をきっかけに、コマンドやPythonコードを自動実行します。

## 現在の機能

- TrigAデスクトップウィンドウ（Vanilla JavaScript管理UI、日英切替・6テーマ対応）
- SQLiteによる自動化定義・実行履歴の保存（OSユーザーデータ領域、`TRIGA_DATA_DIR` で変更可）
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

アプリは起動時にOSユーザーデータ領域（Linux: `~/.local/share/com.triga.automation-manager/automation.db`）へSQLite DBを作成し、Tauriウィンドウからフロントエンドを提供します。旧版の `./data/automation.db` / `./src-tauri/data/automation.db` があれば初回起動時に自動移行（原本は保持）します。REST APIはループバックのみで公開されます。

## 画面

- **ダッシュボード**: 自動化数・有効数・成功／失敗件数、最近の実行（ポイントカラー付きカード）
- **自動化**: 一覧（実行・編集・有効化／無効化・削除）、有効は緑・無効は赤で表示
- **ワークフロー**: 一覧（実行・編集・有効化／無効化・複製・YAML入出力）、エディタ（ノード追加・接続・設定・inputs編集・自動化トリガー連携）、実行詳細（ノード別状態・入出力・ログ・キャンセル）
- **履歴**: 最新20件（自動化・トリガー・状態・対象・開始時刻）
- **設定**: テーマ切替（ライトブルー／ライト／ダーク／ライトオレンジ／ライトピンク／ダークトーキョー）、言語切替（英語／日本語）

テーマと言語の選択はアプリ内に保存され、次回起動時も維持されます。

## ワークフロー

ノード（コマンド／Python／ファイル操作／分岐など）を線でつないで実行する、直列DAG形式の自動化です。SQLiteが正本、YAMLはインポート／エクスポート用です。インポートだけでは実行されません。

### trigger.input の使い方（実行時入力）

`trigger.input` は「実行時に入力を受け取る開始ノード」です。入力の宣言はトップレベルの `inputs:` に書きます。

```yaml
schema_version: 1
workflow:
  id: "wf-input-demo"
  name: "入力デモ"
  enabled: true
inputs:
  target_dir:
    description: "処理対象フォルダ"
    default: "/tmp"
  mode:
    description: "動作モード"
    required: true        # defaultがなければ実行時に必須
nodes:
  - id: "start"
    type: "trigger.input"
    position: { x: 80, y: 160 }
    config: {}
  - id: "list_files"
    type: "file.list"
    position: { x: 320, y: 160 }
    config:
      path:
        $ref: "workflow.input.target_dir"   # 入力参照
      recursive: false
  - id: "finish"
    type: "flow.end"
    position: { x: 560, y: 160 }
    config: {}
edges:
  - id: "e1"
    source: "start"
    source_port: "next"
    target: "list_files"
    target_port: "in"
  - id: "e2"
    source: "list_files"
    source_port: "next"
    target: "finish"
    target_port: "in"
```

- 参照は `{"$ref": "workflow.input.<名前>"}` の型付き参照を使います（`${...}`展開はしません）。
- `trigger.input` ノードの出力にも同じ入力が入るため `nodes.<inputノードID>.output.<名前>` でも読めます。
- 実行時は `default` が適用され、`required: true` が未指定だと実行が拒否されます。
- GUIでは「実行」ボタンが inputs をJSON入力ダイアログで聞きます。RESTでは次のように渡します。

```bash
curl -s -X POST http://127.0.0.1:47821/api/workflows/wf-input-demo/run \
  -H 'Content-Type: application/json' \
  -d '{"inputs": {"target_dir": "/data/inbox", "mode": "fast"}}'
# => {"job_id": "..."}
```

### 既存の自動化からワークフローを実行する方法

既存の自動化（ファイル監視・スケジュール・インターバル・手動）の発火を、そのままワークフローの起動に使えます。自動化定義の複製はしません（関連付けのみ）。

1. GUI: ワークフロー編集画面 → ノード未選択時の右パネル「Automation triggers」で自動化を選んで「Link automation」。解除は「Unlink」。
2. REST:

```bash
# 自動化 {automation_id} の発火でワークフロー {workflow_id} も起動するよう関連付け
curl -s -X POST http://127.0.0.1:47821/api/workflows/{workflow_id}/triggers \
  -H 'Content-Type: application/json' -d '{"automation_id": "{automation_id}"}'
# 関連付け一覧 / 解除
curl -s http://127.0.0.1:47821/api/workflows/{workflow_id}/triggers
curl -s -X DELETE http://127.0.0.1:47821/api/workflow-triggers/{link_id}
```

- 関連付けがない発火は従来どおり自動化だけが動きます（二重起動なし）。
- 関連付けがある発火は「自動化の既存アクション＋リンクされた全ワークフロー」が同じイベントで動きます。
- ワークフローの `enabled: false` は自動起動しません（手動実行は可）。
- イベント値（`file_path` など）は `workflow.context.event.*` で読めます。`inputs:` に `file_path` と同名の入力を宣言しておくと、自動実行時にイベント値が自動補完されます。

### ノードの出力を次のノードに渡す方法

各ノードの出力は `nodes.<ノードID>.output.<出力名>` で参照します。設定内の `{"$ref": "..."}` オブジェクトが実行時に型付き値へ解決されます。

| ノード | 出力名 |
|---|---|
| `command.exec` / `python.script` / `shell.exec` | `stdout`, `stderr`, `exit_code` |
| `file.list` | `files`（配列）, `path` |
| `file.copy` / `file.move` | `source`, `destination` |
| `logic.condition` | `branch`（`true`/`false`） |
| `logic.switch` | `branch`（選択ポート名）, `value`（判定値） |
| `notification.log` | `message`, `level` |
| `trigger.input` | 宣言した入力名すべて |

例：`file.list` → `command.exec` → `notification.log` の受け渡し

```yaml
nodes:
  - id: "list_files"
    type: "file.list"
    config:
      path: { $ref: "workflow.input.target_dir" }
  - id: "process"
    type: "command.exec"
    config:
      program: "echo"
      args:
        - value: { $ref: "nodes.list_files.output.files" }
  - id: "notify"
    type: "notification.log"
    config:
      message: { $ref: "nodes.process.output.stdout" }
```

- 存在しない出力名（例：`stout`）は保存時・実行前の検証で拒否されます。
- 前任ノード以外の参照（後方参照・循環参照）も検証で拒否されます。
- 実行詳細画面ではノードごとに入力・出力JSON（`input / output`）を確認できます。

### 分岐ノードの使い方

2分岐は `logic.condition`、3分岐以上は `logic.switch` を使います。選択されなかった経路は `skipped` として記録されます。

**logic.condition（true/falseの2分岐）**

```yaml
- id: "check"
  type: "logic.condition"
  config:
    left: { $ref: "nodes.process.output.exit_code" }
    operator: "eq"
    right: 0
edges:
  - { id: "e-t", source: "check", source_port: "true", target: "on_true", target_port: "in" }
  - { id: "e-f", source: "check", source_port: "false", target: "on_false", target_port: "in" }
```

`operator` は `eq/ne/gt/gte/lt/lte/contains/starts_with/ends_with/exists` に対応します。GUIで `logic.condition` から接続すると `true`/`false` のどちらを使うか聞かれます。

**logic.switch（値一致による多分岐）**

```yaml
- id: "sw"
  type: "logic.switch"
  config:
    value: { $ref: "workflow.input.mode" }
    cases:
      - equals: "a"
        port: "case1"
      - equals: "b"
        port: "case2"
    default_port: "default"
edges:
  - { id: "e1", source: "sw", source_port: "case1", target: "do_a", target_port: "in" }
  - { id: "e2", source: "sw", source_port: "case2", target: "do_b", target_port: "in" }
  - { id: "e3", source: "sw", source_port: "default", target: "do_other", target_port: "in" }
```

- `value` と `equals` は文字列比較されます（数値・真偽値も文字列化して比較）。
- 先に一致した `cases` のポートが実行され、どれにも一致しなければ `default_port` が実行されます。
- `cases` の `port` 名と `default_port` がそのまま出力ポート名になります（エッジの `source_port` と一致させること）。
- 出力 `branch` には選択されたポート名、`value` には判定値が入ります。

### HTTP Request（`http.request`）— 外部Web API呼び出し

GET・POST・PUT・DELETE・PATCH・HEAD、ヘッダー・クエリ・JSONボディに対応します。

```yaml
- id: "fetch"
  type: "http.request"
  config:
    method: "GET"
    url: "https://api.example.com/items"
    headers:
      Authorization: "Bearer xxx"   # 秘密情報はYAMLに直書きせず参照経由推奨
    query:
      limit: 10
    timeout_seconds: 30
    max_response_bytes: 1048576     # 既定1MB・上限10MB
```

- 出力：`status_code`、`headers`、`body`（文字列）、`body_json`（JSONならパース結果）、`is_success`（2xx判定）、`url`、`elapsed_ms`。
- `body` にオブジェクト／配列を渡すとJSON送信、`body_json` は常にJSON送信になります。
- 許可されるURLは `http://`・`https://` のみです。それ以外は実行前に拒否されます。
- タイムアウト・リトライはワークフロー共通の実行ポリシー（ノード個別 `timeout_seconds`／`retry` 上書き可）が適用されます。
- 例：API応答から値だけ抜いて次へ渡すには `data.transform` と組み合わせます（下記）。

### Data Transform（`data.transform`）— ノード間のデータ変換

任意コードは実行せず、定義済みの変換操作だけを組み合わせます。

```yaml
- id: "pick"
  type: "data.transform"
  config:
    operations:
      - op: "extract"          # JSONパス抽出（例：body_json.data.0.name）
        value: { $ref: "nodes.fetch.output.body_json" }
        path: "items"
      - op: "map"              # 配列の各要素から field を集める
        field: "name"
      - op: "join"             # 配列→文字列
        separator: ", "
```

操作一覧：`replace`（文字列置換）、`template`（`{{名}}` 展開）、`extract`（`a.b.0.c` 形式のパス）、`filter`（`where: {field, equals/contains/starts_with/ends_with/gt/lt}`）、`map`、`join`、`split`、`trim`、`upper`、`lower`、`date_format`（`output_format: "%Y-%m-%d"` 等、日付文字列→書式変換）。

- 単一操作なら `config: {op: "upper", value: ...}` の省略形も使えます。
- 省略された `value` は前段の結果を引き継ぐため、複数操作を縦に繋げられます。
- 出力：`result`（最終値）、`steps`（実行段数）。
- 例：`{"$ref": "nodes.fetch.output.body"}` を受けて必要な項目だけ抽出し、コマンド引数やログ文面に渡せます。

### Delay / Wait（`flow.delay`）— 指定時間の待機

```yaml
- id: "wait"
  type: "flow.delay"
  config:
    seconds: 5            # 0〜86400（milliseconds との合計は24hまで）
    # milliseconds: 500
```

- 出力：`waited_ms`、`requested_ms`。
- 非同期スリープのためワーカースレッドを占有しません。待機中もキャンセル可能です。
- 用途：ファイル作成後の数秒待ち、API連続アクセスの間引き、別アプリの処理待ち。

### Wait for File Ready（`file.wait_ready`）— 書き込み完了待ち

ファイルの存在だけでなく、サイズ・更新時刻が `stable_seconds` の間変わらないことを確認してから進みます。生成途中のCSVを読み込む事故を防げます。

```yaml
- id: "wait_csv"
  type: "file.wait_ready"
  config:
    path: { $ref: "workflow.input.target_dir" }
    stable_seconds: 2     # 1〜300
    timeout_seconds: 60   # 1〜86400（超過時は timeout）
```

- 出力：`path`、`size`、`stable_seconds`、`waited_ms`。
- ファイル監視トリガー（`file_created` 連携）と相性がよく、コピー完了後の変換処理の前段に置きます。

### Notify Desktop（`notify.desktop`）— OS通知

完了・失敗をデスクトップ通知で知らせます（Linux／Windows／macOS対応）。

```yaml
- id: "ping"
  type: "notify.desktop"
  config:
    title: "TrigA"
    message: { $ref: "nodes.process.output.stdout" }
    level: "info"    # info/warning/error/success
```

- 出力：`delivered`、`title`、`message`。
- 通知基盤がない環境（ヘッドレス等）では `delivered: false` を返して成功扱いにします（ワークフローは失敗しません）。確実な履歴には `notification.log` を併用してください。

### Trigger Url Changed（`trigger.url_changed`）— Web更新起点

URLを取得してsha256ハッシュ化し、変化を検知します。開始ノードにも途中ノードにも置けます。

```yaml
- id: "watch"
  type: "trigger.url_changed"
  config:
    url: "https://example.com/news"
    # json_path: "data.updated_at"   # JSON APIの一部だけ監視
    # expected_hash: "abc..."        # 指定時は一致不一致で changed を判定
```

- 出力：`url`、`status_code`、`body`、`hash`、`changed`、`bytes`。
- 定期ポーリングの推奨構成：`interval` 自動化をリンク → 本ノードで取得 → `logic.condition` で `nodes.watch.output.changed == true` の経路だけ後続処理へ。専用スケジューラは今後の拡張とします。
- `expected_hash` 指定時はハッシュ比較、未指定の初回は `changed: true` です。

### Flow for Each（`flow.for_each`）— 繰り返し処理

配列の各項目に同じ後続チェーンを実行します（例：`file.list` の `files`）。

```yaml
- id: "loop"
  type: "flow.for_each"
  config:
    items: { $ref: "nodes.list_files.output.files" }
    max_items: 100        # 1〜1000の安全上限
edges:
  - { id: "e1", source: "loop", source_port: "next", target: "do_thing", target_port: "in" }  # 本体
  - { id: "e2", source: "loop", source_port: "done", target: "finish", target_port: "in" }    # 終了後
```

- 本体では `nodes.<loopID>.output.item`（当回要素）・`index`・`count` が使えます。
- 本体ノードの実行履歴は `do_thing[0]`、`do_thing[1]`…と回ごとに記録されます。本体内の分岐（condition／switch）は回ごとに評価されます（入れ子の for_each／parallel はv1非対応）。
- 出力：`count`、`items`。空配列なら本体を実行せず `done` へ進みます。
- 本体で失敗・タイムアウトが起きるとループ全体が停止します（`failure_policy: stop`）。

### Flow parallel（`flow.parallel`）— 並列処理

独立した複数処理を分岐実行し、`done` で合流します。

```yaml
- id: "fan"
  type: "flow.parallel"
  config:
    max_branches: 8       # 1〜32
edges:
  - { id: "e1", source: "fan", source_port: "branch1", target: "job_a", target_port: "in" }
  - { id: "e2", source: "fan", source_port: "branch2", target: "job_b", target_port: "in" }
  - { id: "e3", source: "fan", source_port: "done", target: "finish", target_port: "in" }
```

- `done` 以外の全ポートが分岐です（ポート名は自由）。`done` の先が合流後の処理になります。
- 出力：`branches`（実行分岐数）。いずれかの分岐が失敗すると全体が `failed` になります。
- v1は分岐をポート順に逐次実行します（結果は同時実行と同一。SQLite永続化の直列化のため。真の同時実行は今後の拡張とします）。分岐同士は独立したノード集合にしてください（共有ノードは先勝ち）。

## 外部AIエージェント連携

`Skill/TrigA-skill/SKILL.md`に、REST API経由で自動化の作成・実行・状態確認を行う手順をまとめています。自然言語の依頼（例：「InputフォルダにCSVが入ったらPythonで処理してCompletedへ移動して」）を自動化定義に変換して`POST /api/automations`で登録できます。

## License

このプロジェクトは [MIT License](./LICENSE) のもとで公開されています。

## 補足

システムトレイ、OS自動起動、ワーカープール／ジョブテーブルの本格化は今後の拡張予定です。インターバル・定期実行はイベント駆動（ポーリングなし）で動作します。現在の契約は`src-tauri/tests/interval_scheduled_mvp.rs`と`src-tauri/tests/file_trigger_history.rs`を参照してください。
