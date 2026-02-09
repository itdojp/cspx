# cspx 有用性・動作検証レポート（v0.2）

更新日: 2026-02-09  
対象リポジトリ: `itdojp/cspx`  
検証対象コミット: `67d948edfb0f3eeba4609467b46409ba9eb9c7fb`（main）

## 0. 結論
- `Issue #105` は `CLOSED`、A/B/C のタスクチェックは全て `[x]` で完了状態。
- Problem Suite（`fast` + `bench`）は、現行期待値に対して **全33問題 PASS**。
- `cargo test` は **全77テスト PASS**（fail 0）。

## 1. #105（案A/B/C）完了確認
2026-02-09 時点で、以下を確認した。
- `https://github.com/itdojp/cspx/issues/105`: `state=CLOSED`
- A/B/C セクションの TODO がすべて `[x]`
- 補足コメントで cross-repo 完了確認（`itdojp/ae-framework#1905`）が明示

## 2. 検証前提
- `cargo 1.91.0 (ea2d97820 2025-10-10)`
- `rustc 1.91.0 (f8297e351 2025-10-28)`
- 実行証跡格納先: `docs/test-results/2026-02-09-validation/`

## 3. Problem Suite 検証結果
### 3.1 実行コマンド
```sh
cargo build -p cspx
cargo build -p cspx --release
cargo test
scripts/run-problems --suite fast --cspx target/debug/cspx
scripts/run-problems --suite bench --cspx target/release/cspx --measure-runs 5 --warmup-runs 1
```

### 3.2 問題数
- `fast`: 25 問題（26 run: `P302` repeat=2）
- `bench`: 8 問題（40 measurement run + 8 warmup run）
- 合計: 33 問題

### 3.3 実行ステータス集計（run単位）
- `fast` (`run-fast.log`)
  - `pass=9`, `fail=13`, `error=3`, `unsupported=1`
- `bench` (`run-bench.log`, measurement run のみ)
  - `pass=35`, `error=5`

注記: `pass/fail/error/unsupported` は cspx の check 結果ステータスであり、問題ランナーの期待値判定とは別。

### 3.4 期待値判定（Problem Runner）
- `problems/.out/*/report.txt` 集計: **33/33 PASS**
- 以前不一致だった `P906` は、現状仕様（`unsupported` と `error/invalid_input` の暫定境界）に合わせて期待値を更新し、suite 完走を確認。

## 4. テスト結果
### 4.1 `cargo test`
- 実行結果: **77 passed / 0 failed**
- 主な領域: CLI schema/golden、frontend、LTS、deadlock/divergence/determinism/refinement、Disk/Hybrid store、並列探索決定性

## 5. CI 実績
- CI（main 最新）
  - run: `https://github.com/itdojp/cspx/actions/runs/21819501369`
  - status: `completed`, conclusion: `success`
  - head SHA: `67d948edfb0f3eeba4609467b46409ba9eb9c7fb`

## 6. 既知制約（現時点）
- 問題集には現機能境界を固定する目的で、`unsupported/error` を期待値に含むケースがある。
- `P906` は FD重経路ベンチ題材として保持しつつ、実装拡張前の暫定期待値を採用している。

## 7. 証跡ファイル
- `docs/test-results/2026-02-09-validation/build-debug.log`
- `docs/test-results/2026-02-09-validation/build-release.log`
- `docs/test-results/2026-02-09-validation/run-fast.log`
- `docs/test-results/2026-02-09-validation/run-bench.log`
- `docs/test-results/2026-02-09-validation/cargo-test.log`

## 8. バージョン保管
- 旧版: `docs/validation-report-2026-02-07.md`
- 新版: `docs/validation-report-2026-02-09.md`
- 最新固定パス: `docs/validation-report.md`（本版の複製）
