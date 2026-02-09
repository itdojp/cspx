# FD最適化バックログ（WS6-B）

## 目的
WS6-A（`#121`）で固定した計測導線を基準に、FD最適化の着手順を明確化する。
本バックログは「効果」「リスク」「検証コスト」の3軸で優先順位を定義し、着手可否を事前判定する。

## 評価軸
- 効果: `P220_fd_refine_fail_impl_diverges` と bench 問題（例: `P905`）での計測改善幅（実行時間/`fd_*` 指標）。
- リスク: 判定同値性（`pass/fail`、counterexample trace、主要tags）の維持可否。
- 検証コスト: 追加テスト・再現手順・回帰確認に必要な工数。

## 優先順位
1. P1 `#134` FD tau-closure/cycle 判定メモ化
2. P2 `#135` FD SCC判定最適化（divergence heavy path）
3. P3 `#136` FD 並列化/POR適用の事前検証スパイク
   - 検証条件テンプレート: `docs/fd-parallel-por-spike.md`

## 子Issue共通ルール
- 各Issueは、実装前に「測定対象」「成功条件」「撤退条件」を本文に明記する。
- 効果検証は、最低でも以下を実行する。
  - `cargo run -q -p cspx -- refine --model FD problems/P220_fd_refine_fail_impl_diverges/spec.cspm problems/P220_fd_refine_fail_impl_diverges/impl.cspm`
  - `scripts/run-problems --suite bench --only P905 --measure-runs 3 --warmup-runs 1`
- 判定差分が出た場合は、最適化を巻き戻して原因分析Issueへ切り出す。

## 依存関係
- 起点Issue: `#122`（本バックログ確立Issue、完了）
- 計測導線: `#121`
- 親Epic: `#110`
