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

### v0.2+ 要件（外部インデックス化・排他・復旧）
現行実装の制約（全スキャン、ロックなし、破損耐性なし）を踏まえ、次の要件を追加する。

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
- `StateCodec::encode` が決定的であること（同一 state → 同一 bytes）。
- v0.1 で既に満たしているように、`TransitionProvider::transitions` が決定的順序であることを維持する（例: `label` 昇順、次状態を `StateCodec` bytes 昇順）。

#### 探索順の規約（SHOULD）
- 各探索レベルごとに、まず現在の frontier を `StateCodec` bytes の昇順に正規化（ソート）する。
- 正規化済み frontier を、その順序を保持したまま **固定順**でワーカーに割当（例: 連続チャンク分割）する。
- 各ワーカーは割り当てられた部分 frontier を処理し、生成した候補状態列を **その入力順を維持した列**として返す。
- すべてのワーカー結果を、割り当てチャンクの元の並び順（昇順に正規化された frontier の順序）どおりに連結して候補集合を構成し、次 frontier は「候補集合を `StateCodec` bytes 昇順にソート → 重複除外 → frontier 化」する。

#### 出力の決定性（MUST）
- 同一入力/同一 `seed`/同一 `workers` で、少なくとも以下が一致する:
  - `states` / `transitions`（統計）
  - counterexample の trace（チェックが反例を返す場合）

### CLI への反映（v0.2+）
- `--parallel <n>`: 並列探索を有効化（`n>=1`）。
- `--deterministic`: deterministic mode を有効化。
- `--seed <n>`: deterministic mode で必須（探索順の将来拡張に備え、結果 JSON に記録する）。
