# スケール設計（M5 / Phase 5）

本ドキュメントは Phase 5（#59）における「再現性」「説明性」「スケール」のうち、特に
- `DiskStateStore` の永続化
- `explore_parallel` の決定性
について、v0.1（現行実装）と v0.2+（拡張要件）を分けて仕様として明文化する。

## DiskStateStore の永続化方式
### 目的
- 大規模モデル向けに状態集合をディスクへ退避し、メモリ消費を抑える。
- CI-first の再現性を損なわないこと（同一入力で同一の状態集合を再構築可能）。
-（v0.2+）クラッシュ/並列実行でも破損せず、復旧導線があること。

### 用語
- **log**: 状態の追記ログ（append-only）。
- **index**: 重複判定（存在判定）を高速化する外部インデックス。
- **record**: 1状態ぶんの永続化単位。

### 方式（v0.1 / 現行実装準拠）
現行実装は `crates/cspx-core/src/disk_store.rs` に準拠する。

- **ファイル**: 単一ファイル（例: `state.log` 相当）に追記する。
- **record 形式**: `StateCodec::encode(state)` のバイト列を 16進文字列化し、1行1record として `append` する。
- **同一判定**: エンコード済みバイト列の完全一致（`Vec<u8>` の一致）。
- **ロード**:
  - 起動時に全行を読み込み、hex decode して `HashSet<Vec<u8>>` を構築する。
  - decode したバイト列は `StateCodec::decode` で **妥当性検証**し、1件でも不正な record があれば `open` 全体を `InvalidData` エラーとして失敗させる（不正な record のスキップや部分的なロードは行わない）。
- **書き込み**:
  - `insert` のたびに `append` で開き、1行を書き込む（fsync/flush は仕様化していない）。
  - ロック（排他）は行わない。

### 決定性/衝突回避（v0.1）
- `StateCodec` は **決定的（deterministic）** であること（同一状態 → 同一バイト列）。
- 仕様上、同一バイト列は同一状態とみなすため、`StateCodec` は **衝突しない完全シリアライズ**を提供することを前提とする。

### 方式（v0.2 / 現行実装）
`feat/phase5-disk-store-81` 以降の実装は、`path` を log パスとして次の 3 ファイルを使用する。

- `path`（例: `states.log`）: append-only の状態ログ。
- `path.with_extension("idx")`（例: `states.idx`）: 外部インデックス。
- `path.with_extension("lock")`（例: `states.lock`）: 排他用ロックファイル。

#### index フォーマット（v0.2 現行）
- 1行目: `cspx-disk-index-v1 log_len=<n>`
- 2行目以降: `StateCodec::encode(state)` を hex 化した 1 行 1 record。
- `open` 時は `log_len` と実 log サイズを照合し、一致しない場合は idx を破棄して log から再構築する。

#### 復旧（v0.2 現行）
- `idx` が欠損/破損/不整合のとき、`state.log` を正として idx を再生成する。
- `state.log` の末尾に改行なしの不完全 record がある場合は無視し、次回以降の破損伝播を防ぐため log を末尾改行境界まで truncate する。
- 末尾以外の完全行に不正 record がある場合は `InvalidData` として `open` を失敗させる。

#### 排他（v0.2 現行）
- `open` で `state.lock` を `create_new` し、取得できない場合は `WouldBlock` で失敗させる。
- プロセス正常終了時は lock を削除する（異常終了で lock が残る場合は手動削除が必要）。

### WS5-A 計測（v0.2）
`DiskStateStore::metrics()` で、次の観測値を取得する。

- `open_ns`, `lock_wait_ns`, `lock_contention_events`, `lock_retries`
- `index_load_ns`, `index_rebuild_ns`, `index_entries_loaded`, `index_entries_rebuilt`
- `log_read_bytes`, `index_read_bytes`
- `insert_calls`, `insert_collisions`
- `log_write_ns`, `log_write_bytes`, `index_write_ns`, `index_write_bytes`

代表負荷の取得は `cargo run -q -p cspx-core --example store_profile_compare` を使用する。
同一ワークロードで `InMemoryStateStore` と `DiskStateStore` を比較し、WS5-B の最適化優先順位（I/O vs 衝突 vs lock）を判断する。

### v0.3+ 要件（高度化）
現行 v0.2 実装（lock file 排他、hex index）を踏まえ、将来の高度化要件を以下に示す。

#### ファイルレイアウト
- `state.log`: append-only の record ログ。
- `state.idx`: 外部インデックス（log を全スキャンせず存在判定できること）。
- `state.lock`: 排他制御用（ロックファイル or OS ロック）。

#### 外部インデックス（state.idx）
目的は「起動時の全スキャン回避」と「存在判定の高速化」であり、hash 衝突を考慮した設計とする。

- **基本方針**:
  - `state.idx` は `state.log` から再構築可能であること（破損時復旧/移植を容易にする）。
  - hash のみで同一判定せず、必要に応じて `state.log` の実体で検証できること（衝突回避）。
- **推奨フォーマット（案）**:
  - 1行（または fixed-size レコード）に `hash64` と `offset`（log 内位置）を保存する。
  - `hash64` は `StateCodec::encode(state)` のバイト列から計算（アルゴリズムと `exploration_seed`（ハッシュ計算にも用いる）をヘッダに記録）。
  - `open` 時は idx を読み込み、`hash64 -> offsets` を構築し、衝突時は `offset` から log を参照して最終一致を確認する。
- **再構築**:
  - `state.idx` が無い/破損している場合、`state.log` をスキャンして idx を再生成する。

#### 排他制御（ロック）
- 同一 store（同一パス）への複数プロセス同時書き込みは **禁止**する。
- `open` は排他ロック取得に失敗した場合、エラーで失敗させる（診断可能なメッセージを返す）。
- ロック方式は OS の advisory lock を優先し、取得に失敗した場合は `state.log` と同一ディレクトリに `state.lock` という名前のロックファイルを用いるフォールバック方式を仕様化する。

#### クラッシュ復旧
- `state.log` の末尾が部分書き込みで壊れている場合に備え、少なくとも「末尾の不完全 record を無視して復旧」できること。
- `state.idx` が `state.log` に追随できていない場合（書き込み中クラッシュ等）は、`state.log` を正として idx を再構築する。

#### コンパクション
- 重複除外済みの log を生成して `state.log` を置換できる（rename + fsync 等の手順を仕様化）。

## explore_parallel の決定性要件
### 目的
- 並列探索の性能を得つつ、CI での再現性を損なわない。
-（v0.2+）同一入力/同一条件で **同一の反例**が得られること（レビュー容易性）。

### 現状の位置づけ（v0.1）
- `explore_parallel` は **探索順序の決定性を保証しない**（counterexample 生成器から利用する場合、反例が変動し得る）。
- ただし、次の前提が満たされる限り、**最終的な到達状態集合**と**統計値（`states` / `transitions`）**が単一スレッド探索と一致することを目標とする（探索の途中経過や各レベル内での探索順序の一致は保証しない）:
  - `TransitionProvider::transitions` が決定的順序で遷移を返す
  - 探索対象が有限

### deterministic mode（v0.2+ 要件）
deterministic mode は「スケジュールに依存しない探索順」を仕様として固定する。

#### 前提条件（MUST）
- ワーカー数が固定であること（同一 `workers`）。
- 状態順序キーが決定的であること（現行実装では `State: Ord` による全順序）。
- v0.1 で既に満たしているように、`TransitionProvider::transitions` が決定的順序であることを維持する（例: `label` 昇順、次状態も決定的順序）。

#### 探索順の規約（SHOULD）
- 各探索レベルごとに、まず現在の frontier を決定的な状態順序（現行実装では `Ord`）で正規化（ソート）する。
- 正規化済み frontier を、その順序を保持したまま **固定順**でワーカーに割当（例: 連続チャンク分割）する。
- 各ワーカーは割り当てられた部分 frontier を処理し、生成した候補状態列を **その入力順を維持した列**として返す。
- すべてのワーカー結果を、割り当てチャンクの元の並び順（昇順に正規化された frontier の順序）どおりに連結して候補集合を構成し、次 frontier は「候補集合を決定的順序でソート → 重複除外 → frontier 化」する。

#### 出力の決定性（MUST）
- 同一入力/同一 `seed`/同一 `workers` で、少なくとも以下が一致する:
  - `states` / `transitions`（統計）
  - counterexample の trace（チェックが反例を返す場合）

### CLI への反映（v0.2+）
- `--parallel <n>`: 並列探索を有効化（`n>=1`）。
- `--deterministic`: deterministic mode を有効化。
- `--seed <n>`: deterministic mode で必須（現行は将来拡張向け予約値として結果 JSON に記録する）。

## 計測ノイズ対策（WS2-B / v0.2）
### 目的
- ベンチ測定の誤判定を抑制し、比較可能な数値を継続的に取得する。
- deterministic 実行時の同一性を runner で検証し、回帰を早期検出する。

### ルール（runner）
- `scripts/run-problems` は `--measure-runs`（デフォルト 1）と `--warmup-runs`（デフォルト 0）を受け付ける。
- 問題ごとの測定 run 回数は `max(problem repeat, --measure-runs)` とする。
- 集計値は `median`（および `min/max`）を採用し、外れ値の自動除外は行わない（`outlier_policy=none`）。
- 各問題の測定結果を `problems/.out/<P###>/metrics-summary.json` に保存する。

### deterministic 整合チェック
- 測定 run が 2 回以上かつ、全 run の `invocation.deterministic=true` の場合に整合チェックを評価する。
- 判定は正規化 JSON の同一性で行う。
  - 既定除外: `started_at`, `finished_at`, `duration_ms`, `tool.git_sha` に加え、`metrics` の時間依存項目。
- 不一致時は `report.txt` を `FAIL` とし、run 間差分があることを明示する。

## baseline 比較と閾値判定（WS3-B / v0.2）
### 目的
- bench 実行結果を baseline と比較し、性能劣化を `warn/fail` で機械判定する。
- 判定根拠（baseline値、current値、劣化率）を CI 出力に残す。

### 判定仕様
- 比較対象: `metrics-summary.json` の `aggregate.duration_ms.median`
- 劣化率: `delta_pct = ((current - baseline) / baseline) * 100`
- 既定閾値:
  - `warn_threshold_pct = 20`
  - `fail_threshold_pct = 40`
  - `min_baseline_ms = 100`（baseline が短すぎる問題は `skipped`）

### CI 反映
- `.github/workflows/bench.yml` で `scripts/bench-baseline compare` を実行する。
- `fail` が 1 件以上なら workflow を失敗させる。
- `warn` は失敗にしないが、`GITHUB_STEP_SUMMARY` と artifact に記録する。

### 過検知抑制ルール
- bench 計測は `--measure-runs 5 --warmup-runs 1` を既定とする。
- baseline 未定義の問題は `new_problem` として扱い、fail にしない。
- 非数値/欠損メトリクスは `skipped` とし、即時failにしない。

### baseline 更新
- baseline は `benchmarks/baseline/bench-metrics-baseline.json` を repo 管理とする。
- workflow_dispatch の `update_baseline_candidate=true` で候補 JSON を artifact 出力し、PR で更新する。
- 詳細運用は `docs/bench-baseline.md` を参照。
