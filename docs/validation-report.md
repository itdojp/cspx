# cspx 有用性・動作検証レポート（v0.1）

更新日: 2026-02-07  
対象リポジトリ: `itdojp/cspx`  
対象コミット（main）: `ea67da4f20374f520ce1d7f4f45b83031f36be33`

## 1. 目的
本書は、`cspx` が以下を満たすことを示すための監査用エビデンスをまとめる。

- **有用性**: CI運用で回帰検知に使えること、検査結果が実務で解釈可能であること
- **動作妥当性**: テスト/Problem Suite/CI で継続的に正常動作していること

## 2. 検証前提
- Rust toolchain:
  - `cargo 1.91.0 (ea2d97820 2025-10-10)`
  - `rustc 1.91.0 (f8297e351 2025-10-28)`
- CI ワークフロー（`.github/workflows/ci.yml`）の主要ゲート:
  - `cargo fmt -- --check`
  - `cargo clippy -- -D warnings`
  - `cargo test`
  - `cargo build -p cspx`
  - `scripts/run-problems --suite fast --cspx target/debug/cspx`

## 3. 有用性の根拠
### 3.1 回帰検知として運用可能
- `problems/` には **29問題**（`fast: 25`, `bench: 4`）を定義。
- `fast suite` は PR/Push CI に常時組み込み済みで、仕様逸脱を即時検知できる。
- `expect.yaml` は完全一致ではなく制約評価（`in`/`min`/`max`/`contains`）を採用し、過剰拘束を避けつつ回帰のみを検知できる。

### 3.2 機能カテゴリのカバレッジ
`fast suite`（25問題）は次の機能帯をカバーする。

- `0xx`: Frontend（構文/型/unsupported/error分類）5件
- `1xx`: deadlock/divergence/determinism 11件
- `2xx`: refinement（T/F/FD）6件
- `3xx`: 反例品質/決定性 3件

### 3.3 結果の説明可能性（Explainability）
反例には `tags` と `source_spans` を付与し、原因と該当箇所を特定できる。

- 例1: `P301`（deadlock）
  - `tags`: `deadlock`, `kind:deadlock`, `explained`
  - `source_spans.path`: `model.cspm`
- 例2: `P220`（FD refinement divergence mismatch）
  - `tags`: `model:FD`, `divergence_mismatch`, `kind:divergence`, `kind:refinement`, `explained`
  - `source_spans.path`: `impl.cspm`, `spec.cspm`

### 3.4 決定性（再現性）
- `P302` は `repeat=2` で実行し、`normalized_json_equal` 比較が `PASS`。
- 2回とも `status=pass`、`stats.states=1`, `stats.transitions=0` で一致。

## 4. 動作妥当性の根拠
### 4.1 ローカルテスト実行結果
`cargo test` 実行結果（2026-02-07）:

- 実行テスト総数: **55**
- 失敗: **0**
- 主な検証領域:
  - JSON schema/golden（CLI出力互換）
  - Frontend v0.2（parse/typecheck）
  - LTS（parallel/hiding/recursion）
  - Checker（deadlock/divergence/determinism/refine）
  - DiskStateStore（index/lock/recovery）
  - 並列探索決定性（`explore_parallel deterministic mode`）

### 4.2 Problem Suite 実行結果（fast）
`scripts/run-problems --suite fast --cspx target/debug/cspx` 実行結果（2026-02-07）:

- 対象: **25問題 / 26実行**（`P302` は repeat=2）
- 集計:
  - `pass`: 9
  - `fail`: 13
  - `error`: 3
  - `unsupported`: 1
- 補足:
  - `fail/error/unsupported` は不具合ではなく、各問題の期待値（意図的な失敗シナリオや未対応機能）に対する適合結果を含む。
  - ランナー自体の総合結果は `exit_code=0`（期待値評価まで含めて整合）。

### 4.3 GitHub CI 実績
- 最新 `main` の CI run: `21768037668`
  - URL: `https://github.com/itdojp/cspx/actions/runs/21768037668`
  - conclusion: `success`
  - created_at: `2026-02-06T22:23:10Z`

## 5. 再現手順
以下を実行すれば、本書の主要エビデンスを再取得できる。

```sh
cargo test
cargo build -p cspx
scripts/run-problems --suite fast --cspx target/debug/cspx
scripts/run-problems --suite bench --list
```

必要に応じて、個別証跡は `problems/.out/<P###>/` を確認する。

## 6. 現時点の制約（既知）
- `bench` は性能評価用のプレースホルダを含み、常時CI実行対象ではない。
- 問題集には「現時点で `unsupported/error` が期待値」のケースが存在する（仕様で明示済み）。
- これは現状機能の境界を固定化するための運用であり、拡張時に期待値を段階更新する。

## 7. 結論
`cspx` は、v0.1 時点で以下を満たしている。

- CIに組み込める回帰検知基盤（fast suite）
- deadlock/divergence/determinism/refinement の検査と反例提示
- 反例タグ・ソース位置を含む説明可能な JSON 出力
- テスト/CI の継続的な成功実績

このため、「有用で、継続運用可能な検査器」として導入可能な状態にある。
