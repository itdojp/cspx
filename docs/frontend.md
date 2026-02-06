# Frontend 対応サブセット（v0.2）

## 目的
Frontend（parse/typecheck）の拡張は後続（LTS/Checker/Refinement）に波及するため、先に「CSPM サブセット v0.2」と AST/IR・エラー分類・Span 規約を固定する。

## 実装状況（現行）
現行（M1）の実装は最小サブセットのみ対応している。
- `STOP`（トップレベル式として可）
- 単純なプロセス定義: `NAME = STOP`
- 行コメント: `--` 以降は無視

v0.2 の対応範囲は本書で定義し、実装は Phase 1（#64/#65）で段階的に追従する。

## Problem Suite fast に出現する構文要素（棚卸し）
v0.2 は Problem Suite（`fast`）で使用される構文を優先して定義する。

- `channel` 宣言（単一/複数、値域あり）: P000, P100, P902 ほか多数
- channel 通信（`ch!1` / `ch?x` / `fork.0`）: P100, P902, P901, P003
- 前置（`->`）: ほぼ全問題
- 外部選択（`[]`）: P200, P212 ほか
- 内部選択（`|~|`）: P131, P132
- 並行合成（interleaving `|||`）: P002
- 並行合成（interface parallel `[|{|X|}|]`）: P100, P902 ほか
- hiding（`\\ {|X|}`）: P121, P122, P123
- `assert`（性質）: P100（deadlock free）, P120（divergence free）, P130（deterministic）ほか
- `assert`（refinement）: P212（`[T=` / `[F=`）
- 参考（意図的に未対応）: `datatype`（P004）

## 対応構文（v0.2）
### 字句
- 識別子: `[A-Za-z_][A-Za-z0-9_]*`
- 整数リテラル: `0|[1-9][0-9]*`
- 行コメント: `--` 以降を無視

### 宣言
- channel 宣言
  - `channel a`
  - `channel ch : {0..1}`
  - `channel send, ack, out : {0..1}`
- プロセス定義: `NAME = <process-expr>`
- assert 宣言
  - 性質: `assert <proc-ref> :[deadlock free [F]]` / `:[divergence free [FD]]` / `:[deterministic [FD]]`
  - refinement: `assert <proc-ref> [T= <proc-ref>` / `[F= ...` / `[FD= ...`

### プロセス式
- `STOP`
- 参照: `<proc-ref>`
- 前置: `<event> -> <process-expr>`
- 外部選択: `<process-expr> [] <process-expr>`
- 内部選択: `<process-expr> |~| <process-expr>`
- interleaving: `<process-expr> ||| <process-expr>`
- interface parallel: `<process-expr> [|{|<event-set>|}|] <process-expr>`
- hiding: `<process-expr> \\ {|<event-set>|}`
- 括弧: `(<process-expr>)`

### event / set（v0.2）
- event（v0.2 は「単一チャネル + 1 セグメント」までを対象とする）
  - no-payload: `a`
  - dot（定数）: `fork.0`
  - output（定数/変数）: `send!0`, `out!b`
  - input（定数/束縛）: `ack?0`, `send?b`
- 値域（channel payload 用）: `{0..N}`（整数レンジ）
- event-set: `{|a,b|}`（hiding / interface parallel の同期集合）

### 型/名前解決（v0.2, typecheck）
- 名前空間
  - channel 名、process 名、変数（input による束縛）は別管理とする
- channel 値域
  - `channel ch : {0..N}` の場合、payload は整数かつ `[0, N]` に収まること
  - 値域未指定（`channel a`）の channel は v0.2 では payload なし（`a`）のみを対象とする
- 変数束縛
  - `ch?x` の `x` は後続の process 式（`->` の右側）で参照可能とする（例: `send?b -> out!b -> ...`）
  - `ch!x` の `x` はスコープ内で束縛済みであること（未束縛は `invalid_input`）
  - dot 形式（`ch.0`）は v0.2 では整数リテラルのみを対象とする（`ch.A` は `unsupported_syntax`）

## エラー分類（v0.2）
CLI の status/exit code は `docs/cli.md` の規約に従う。

- `unsupported_syntax`（status: `unsupported`, exit code: 3）
  - v0.2 の字句/構文に存在しない要素（例: `datatype`、未定義の演算子、未対応のリテラル等）
  - パーサが継続不能な構文エラー（v0.2 の字句規約に反する等）
- `invalid_input`（status: `error`, exit code: 2）
  - v0.2 の構文としては妥当だが、入力が不正（例: 未定義参照、重複宣言、payload が値域外、assert の target 不正等）

## SourceSpan 付与規約（v0.2）
- `start_line/start_col/end_line/end_col` は 1-based、かつ `end_*` は **終端の文字位置を含む**（inclusive）。
- `path` は CLI から渡された入力パス文字列（相対/絶対は入力に従う）。
- Span の優先順位（可能な限り狭く、原因箇所を指す）
  - 未定義参照: 参照箇所（識別子）に span を付与
  - channel payload の値域外: payload（整数）に span を付与
  - 構文未対応: 先頭トークンから当該構文要素の終端まで（best-effort）
