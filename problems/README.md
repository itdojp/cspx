# Problem Suite

## 目的
- 回帰テストとして、機能追加時に壊れた箇所を早期検知する
- 反例の短さ・原因タグ・ソース位置など「使える反例」をデモできるようにする
- fast/bench を分け、CI で回す範囲を制御する

## ディレクトリ規約
- 1問題 = 1ディレクトリ: `problems/P###_<slug>/`
- 最低限以下のファイルを持つ
  - `model.cspm`
  - `problem.yaml`
  - `expect.yaml`
  - `notes.md`（任意）

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

## スキーマ
- `schemas/problem.schema.json`
- `schemas/expect.schema.json`
