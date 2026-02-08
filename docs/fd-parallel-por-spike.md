# FD 並列化/POR 事前検証スパイク（WS6-B / P3）

## 目的
- `#136` の Go/No-Go 判定に必要な検証条件を固定する。
- FD 最適化候補（並列化 / POR）について、正当性リスクと実装コストを事前に可視化する。

## 非目的
- 本ドキュメントで本実装を確定しない。
- 判定同値性を崩す最適化を採用しない。

## 候補A: FD 探索並列化
### 想定アプローチ
- BFS レベル単位でノード展開を並列化し、`node_check`（divergence/refusal）を worker に分配する。
- frontier の正規化順序を固定し、結果マージ順を deterministic に統制する。

### 正当性リスク
- 反例 trace の最短性/安定性が崩れる。
- `fd_*` タグ（nodes/edges/pruned）の値定義が逐次実装と乖離する。
- shared cache 導入時に更新順序依存が混入する。

### 最低限の検証条件
- 同一入力・同一 seed・同一 workers で `status` / counterexample events / tags が一致する。
- 逐次実装との差分比較で `pass/fail` が一致する。
- 逐次より遅い場合（中央値で +5% 以上）は不採用。

## 候補B: POR（Partial Order Reduction）
### 想定アプローチ
- 可視イベントの独立性を満たす遷移のみ、ample set 相当の縮約を適用する。
- τ遷移・hiding を含む節点では POR を無効化する保守的モードを初期値とする。

### 正当性リスク
- refusal 集合評価に必要な遷移を誤って削除し、偽陽性/偽陰性を招く。
- divergence 判定の前提（τ閉包）を破壊する。

### 最低限の検証条件
- POR ON/OFF で `status`、counterexample、`fd_*` が一致すること。
- `RefinementModel::FD` の既存テスト群（`refine` / `check_divergence`）が全通過すること。
- 保守モード（τ/hiding検知時にPOR無効）が常に利用可能であること。

## 評価テンプレート（Issue/PR 用）
各検証Issueは以下テンプレートを埋める。

- 対象: `並列化` または `POR`
- 想定効果:
  - 指標: `duration_ns`, `fd_divergence_checks`, `fd_impl_closure_max`
  - 目標: `中央値 -X%`（X はPR本文で明示）
- 正当性確認:
  - 逐次比較コマンド:
    - `cargo run -q -p cspx-core --example fd_divergence_bench`
    - `scripts/run-problems --suite bench --only P905 --measure-runs 3 --warmup-runs 1`
  - 一致判定: `status` / counterexample trace / `fd_*` tags
- 工数見積:
  - 実装: `S/M/L`
  - テスト: `S/M/L`
- 撤退条件:
  - 判定不一致が1件でも再現した場合
  - 速度改善が閾値未達（中央値 +5% 以上悪化含む）の場合

## 依存と出口条件
- 依存: `#134`, `#135` のマージ後を前提とする。
- 出口条件:
  - `#136` に Go/No-Go 判定結果を記録できること。
  - Go の場合のみ、実装Issueを追加分割して着手すること。
