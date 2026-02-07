# Problem Suite

## 目的
- 回帰テストとして、機能追加時に壊れた箇所を早期検知する
- 反例の短さ・原因タグ・ソース位置など「使える反例」をデモできるようにする
- fast/bench を分け、CI で回す範囲を制御する

## 対象読者
- CI 運用者（PR ごとの回帰検知）
- 問題（Problem）作成者（追加・保守）
- 実装者（失敗時のデバッグ、期待値更新）

## 実行方法（Quickstart）
### fast suite をローカルで実行
```sh
cargo build -p cspx
scripts/run-problems --suite fast --cspx target/debug/cspx
```

### bench suite をローカルで実行
```sh
cargo build -p cspx --release
scripts/run-problems --suite bench --cspx target/release/cspx
```

### 問題一覧を表示
```sh
scripts/run-problems --suite fast --list
```

### bench 問題一覧を表示
```sh
scripts/run-problems --suite bench --list
```

### 特定問題のみ実行（ID 指定）
`--only` は複数回指定できる。
```sh
scripts/run-problems --cspx target/debug/cspx --only P000 --only P101
```

### 特定問題のみ実行（ディレクトリ指定）
相対パス・絶対パスのどちらでも指定できる。
```sh
scripts/run-problems --cspx target/debug/cspx --only-dir problems/P000_hello_typecheck_pass
```

### 主なオプション
- `--suite fast|bench`: suite フィルタ（デフォルト: `fast`）
- `--cspx <path>`: `run.cmd[0] == "cspx"` の場合に `cspx` 実体を差し替える（例: `target/debug/cspx`）
- `--jobs <n>`: 並列実行（問題単位、出力順は ID 昇順で安定化）

## bench 生成問題の運用（#112）
### 再生成手順（`problems/generators`）
1) `problems/generators/regenerate_p900_p905.sh` を実行し、`P900`〜`P905` の `model.cspm` を再生成する  
2) `scripts/run-problems --suite bench --list` で対象問題（`P310`, `P900`〜`P905`）が列挙されることを確認する  
3) `cargo build -p cspx --release` 後に `scripts/run-problems --suite bench --cspx target/release/cspx --only P900 --only P901 --only P902 --only P903 --only P904 --only P905` を実行する  
4) `problems/.out/<P###>/report.txt` と差分（`model.cspm` / `problem.yaml` / `expect.yaml` / `notes.md`）をレビューする

### 推奨パラメータレンジ（tiny / medium）
- `ring(N)`: tiny=`4`, medium=`16`
- `philosophers(K)`: tiny=`3`, medium=`5`
- `ABP(M)`（送受信シーケンス上限）: tiny=`1`（`0..1`）, medium=`3`（`0..3`）

### bench 実行時の timeout/失敗運用
- `P900`〜`P905` は `run.timeout_ms` を明示し、計測時の暴走を防ぐ
- `P310` は timeout 挙動観測用の placeholder として、`pass/timeout/unsupported/error` を許容する
- `run.timeout_ms` に達した run は runner が kill し、`exit_code=124` を記録する（`cspx --timeout-ms` の exit code `4` とは別）
- 期待値不一致が 1 件以上ある場合、`scripts/run-problems` 全体の終了コードは `1`。runner 内部エラー（読み込み/spawn 失敗等）の場合は `2`
- `bench` の timeout/失敗は性能観測の入力として扱い、まず `problems/.out` で原因を切り分けた上で再計測する

## CI での実行
GitHub Actions では以下を実行する（`.github/workflows/ci.yml`）。
```sh
cargo build -p cspx
scripts/run-problems --suite fast --cspx target/debug/cspx
```
失敗時は `problems/.out` を artifact（`problems-out`）として upload する。

### CI 責務分離（fast / bench）
- `.github/workflows/ci.yml` の必須ジョブは `fast` のみを実行する
- `bench` はローカル実行または専用 workflow（nightly / manual、#115 / #116）で扱い、PR 必須ゲートには含めない
- 機能回帰判定は `fast`、性能回帰判定は `bench` 側で扱う

## 実行結果（`problems/.out`）
問題実行の生成物は `problems/.out/<P###>/` 配下に出力される。

- `problems/.out/<P###>/report.txt`: 最終結果（`PASS` / `FAIL` と理由）
- `problems/.out/<P###>/run-<N>/stdout.txt`: 標準出力（通常は Result JSON）
- `problems/.out/<P###>/run-<N>/stderr.txt`: 標準エラー出力
- `problems/.out/<P###>/run-<N>/exit_code.txt`: プロセスの exit code
- `problems/.out/<P###>/run-<N>/result.json`: stdout を JSON として parse できた場合の整形出力
- `problems/.out/<P###>/run-<N>/normalized.json`: `compare` 用に正規化した JSON

`result.json` が無い場合は stdout が JSON でない（または空）ことを示す。
`expect.yaml` で `status` / `checks` を期待する場合、stdout が Result JSON（`--format json`）である必要がある。

### タイムアウトの扱い
- `problem.yaml` / `run.timeout_ms` によるタイムアウトは runner 側でプロセスを kill し、`exit_code=124` として記録する。
- `cspx` 自体の `--timeout-ms` と exit code（`4`）とは別概念である（`docs/cli.md` 参照）。

## 期待値ポリシー（暫定）
現段階では `cspx` の機能は段階的に実装中であり、問題集は「現在の対応範囲」を固定化する目的も持つ。
そのため、多くの問題は **`unsupported` / `error` を期待値として許容**する（例: 未対応構文、未実装機能、入力不正など）。

例（`unsupported` または `error` を許容する）:
```yaml
status:
  in: ["unsupported", "error"]
checks:
  - name: typecheck
    status:
      in: ["unsupported", "error"]
```

期待値の変更（例: `unsupported` → `pass`）を行う場合は、`notes.md` に変更理由と根拠（対応機能・仕様差分）を記録する。

## 決定性・比較（`repeat` / `compare`）
同一問題を複数回実行して比較したい場合、`repeat` と `compare` を使う。

- `repeat`: `expect.yaml` を優先し、未指定の場合は `problem.yaml` の `run.repeat`、それも無ければ `1`
- `compare.kind: normalized_json_equal`: `normalized.json` を run 間で比較する

正規化（`normalized.json`）では以下のフィールドを常に除外する。
- `started_at`
- `finished_at`
- `duration_ms`
- `tool.git_sha`

追加で除外したいフィールドは `compare.ignore_fields` に **ドット区切りパス** で指定する。
```yaml
compare:
  kind: normalized_json_equal
  ignore_fields:
    - tool.version
```

## 失敗時の切り分け（最短手順）
1) `problems/.out/<P###>/report.txt` を確認し、どの run のどの項目が不一致か把握する  
2) `stdout.txt` / `stderr.txt` / `exit_code.txt` を確認する  
3) stdout が JSON でない場合は `problem.yaml` の `run.cmd` が `--format json` を指定しているか確認する  
4) CI では failure artifact（`problems-out`）をダウンロードして同様に確認する

## ディレクトリ規約
- 1問題 = 1ディレクトリ: `problems/P###_<slug>/`
- 最低限以下のファイルを持つ
  - `model.cspm`
  - `problem.yaml`
  - `expect.yaml`
  - `notes.md`（任意）
- 生成型 `bench` 問題は `problems/generators/` に生成ロジックと再生成手順を置く

## `problem.yaml`
### 例
```yaml
id: P100
title: deadlock-free minimal rendezvous
suite: fast
tags: [deadlock, assertion]
run:
  cmd: ["cspx", "check", "--all-assertions", "model.cspm", "--format", "json"]
  timeout_ms: 5000
```

### フィールド
- `id`（必須）: `P###` 形式
- `title`（必須）: 人間向けの短い説明
- `suite`（任意）: `fast` / `bench`（未指定は `fast` とみなす）
- `tags`（任意）: 任意のタグ
- `run`（必須）:
  - `cmd`（必須）: 実行コマンド（配列）
  - `cwd`（任意）: 作業ディレクトリ（未指定は問題ディレクトリ）
  - `env`（任意）: 環境変数（`KEY: value`）
  - `timeout_ms`（任意）: 実行タイムアウト
  - `repeat`（任意）: 同一コマンドの繰り返し回数（デフォルト 1）

## `expect.yaml`
### 例
```yaml
exit_code: 0
status: pass
checks:
  - name: check
    target:
      contains: "deadlock free"
    status: fail
    counterexample:
      present: true
      trace_len: { max: 1 }
      tags:
        contains: ["deadlock"]
```

### 期待値の書き方（制約）
`expect.yaml` は完全一致ではなく **制約** で評価する。

#### 制約オブジェクトの例
```yaml
status: { in: ["pass", "unsupported"] }
exit_code: { min: 0, max: 5 }
target: { contains: "deadlock free" }
```

### フィールド（概要）
- `exit_code`: 数値 または 制約
- `status`: 文字列 または 制約（`pass`/`fail`/`unsupported`/`timeout`/`out_of_memory`/`error`）
- `checks`: チェック期待値の配列（部分一致）
  - `name` / `target` / `model` / `status` / `reason.kind`
  - `counterexample`:
    - `present`（bool）
    - `trace_len`（min/max）
    - `tags`（contains/equals）
    - `source_spans.any`（path/line/col の制約）
  - `stats`:
    - `states` / `transitions`（min/max/eq）
- `repeat`（任意）: 同一問題の実行回数
- `compare`（任意）: 複数回実行の比較条件

## `notes.md`（任意）
問題の意図・背景・期待値の根拠（暫定含む）を記録する。特に以下は `notes.md` に残す。
- 何をデモ/回帰したい問題か（設計意図）
- `unsupported` / `error` を許容している理由（現在地の仕様）
- 将来の期待値変更方針（例: 実装が進んだら `pass` に変更する）

## 新規 Problem の追加手順（作成者向け）
1) `problems/P###_<slug>/` を作成する（`id` は重複不可）  
2) `problem.yaml` / `expect.yaml` を作成する（スキーマ: `schemas/*.schema.json`）  
3) `run.cmd` が JSON を stdout に出すよう `--format json` を指定する（`status` / `checks` を評価するため）  
4) `scripts/run-problems --cspx target/debug/cspx --only P###` でローカル実行し、`problems/.out` を確認する  
5) 期待値の意図を `notes.md` に記録する（暫定の `unsupported`/`error` を含む）

## レビュー観点（チェックリスト）
- `fast` suite は CI で実行可能な時間に収まる（必要に応じ `timeout_ms` を設定）
- 期待値は完全一致ではなく制約で記述し、不要に過剰制約しない（将来の出力拡張に耐える）
- 環境依存（絶対パス、時刻、非決定的順序）を避ける。必要なら `compare.ignore_fields` で除外する
- `notes.md` に設計意図と暫定期待値の理由が記載されている

## 関連ドキュメント
- `docs/cli.md`（exit code 規約、timeout など）
- `docs/result-json.md`（Result JSON 形状と status/reason）
- `problems/generators/README.md`（bench 生成問題の再生成手順）

## スキーマ
- `schemas/problem.schema.json`
- `schemas/expect.schema.json`
