# Bench Baseline 運用（WS3-B / #116）

`bench` suite の性能劣化を自動検知するため、`scripts/bench-baseline` を用いて
- baseline 生成（`write`）
- baseline 比較（`compare`）
を行う。

## 比較対象と判定
- 比較対象: `problems/.out/<P###>/metrics-summary.json` の `aggregate.duration_ms.median`
- 判定:
  - `delta_pct >= fail_threshold` -> `fail`（exit code 1）
  - `delta_pct >= warn_threshold` -> `warn`（exit code 0）
  - それ以外 -> `ok`
- suppress ルール:
  - baseline が未定義 -> `new_problem`（fail にしない）
  - baseline が `min_baseline_ms` 未満 -> `skipped`（短時間ケースの過検知抑制）
  - 数値欠損/非数値 -> `skipped`

`delta_pct` は `((current - baseline) / baseline) * 100` で計算する。

## ベースラインファイル
- 既定パス: `benchmarks/baseline/bench-metrics-baseline.json`
- 形式（抜粋）:
  - `thresholds.warn_pct` / `thresholds.fail_pct`
  - `thresholds.min_baseline_ms`
  - `problems.<P###>.duration_ms_median`

## コマンド
### baseline 生成
```sh
scripts/bench-baseline write \
  --out-dir problems/.out \
  --baseline benchmarks/baseline/bench-metrics-baseline.json \
  --warn-threshold-pct 20 \
  --fail-threshold-pct 40 \
  --min-baseline-ms 100
```

### baseline 比較
```sh
scripts/bench-baseline compare \
  --out-dir problems/.out \
  --baseline benchmarks/baseline/bench-metrics-baseline.json \
  --report-json artifacts/bench/baseline-compare.json \
  --report-md artifacts/bench/baseline-compare.md \
  --warn-threshold-pct 20 \
  --fail-threshold-pct 40 \
  --min-baseline-ms 100
```

## CI 統合
`.github/workflows/bench.yml` は次を行う。
- `scripts/run-problems --suite bench --measure-runs 5 --warmup-runs 1`
- `scripts/bench-baseline compare` を実行し、結果を `GITHUB_STEP_SUMMARY` に追記
- `fail` 判定が 1 件でもあれば workflow を失敗させる

## baseline 更新フロー
1. `main` を対象に `Bench` workflow を `workflow_dispatch` で実行し、`update_baseline_candidate=true` を指定する。  
2. artifact `bench-results` から `artifacts/bench/bench-metrics-baseline.candidate.json` を取得する。  
3. 取得した JSON で `benchmarks/baseline/bench-metrics-baseline.json` を更新し、PR でレビュー・マージする。  
4. 次回以降の `Bench` workflow で自動比較が有効化される。  
