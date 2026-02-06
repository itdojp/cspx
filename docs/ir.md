# Core IR（v0.2 設計）

## 目的
Frontend（parse/typecheck）は CSPM サブセットを読み取り、後続の LTS/Checker/Refinement が利用できる中間表現（IR）を生成する。
v0.2 では Problem Suite（fast）で使用される構文を中心に、IR の責務と拡張方針を固定する。

## 前提（v0.2）
- CSPM サブセットは `docs/frontend.md` に従う。
- 型（値域）は「整数レンジ `{0..N}`」のみを対象とする（`datatype` などは `unsupported_syntax`）。
- IR は後続が扱いやすいよう **参照解決**（process/channel/変数）と **値域整合** を済ませる。

## AST と IR の責務分離
- AST
  - 字句/構文構造を保持（エラー復元のために局所情報を保持してよい）
  - 可能な限り全ノードに `SourceSpan` を付与
- IR
  - 後続処理が必要とする情報に正規化し、曖昧な構文糖を排除
  - 名前解決・型整合を済ませ、不整合は `invalid_input` として報告

## IR の基本方針（データモデル）
`crates/cspx-core/src/ir.rs` の `Spanned<T>` を基礎とし、v0.2 では次を扱う。

### 1) Channel
- `channel a`
- `channel ch : {0..N}`
- `channel send, ack, out : {0..N}`

IR では channel 名はユニークであること。
値域未指定（`channel a`）は payload を持たないチャネルとして扱う。

### 2) Event（channel 通信）
v0.2 の event は「単一チャネル + 1 セグメント」まで。

- no-payload: `a`
- dot（定数）: `fork.0`
- output（定数/変数）: `send!0`, `out!b`
- input（定数/束縛）: `ack?0`, `send?b`

IR では event を「channel + 0/1 個の通信セグメント」として表現する。

### 3) Process
process 式（v0.2）:
- `STOP`
- 参照
- prefix
- external choice（`[]`）
- internal choice（`|~|`）
- interleaving（`|||`）
- interface parallel（`[|{|X|}|]`）
- hiding（`\\ {|X|}`）

IR では process 名はユニークであること。

### 4) Assertion
`assert` は後続で `check --all-assertions` を実装するために IR 上で保持する。

- 性質:
  - `deadlock free [F]`
  - `divergence free [FD]`
  - `deterministic [FD]`
- refinement:
  - `SPEC [T= IMPL`
  - `SPEC [F= IMPL`
  - `SPEC [FD= IMPL`

## 型/名前解決（v0.2）
### 名前空間
channel/process/変数（input 束縛）は別管理とする。

### 変数束縛
`ch?x -> ...` の `x` は `->` の右側（継続）で参照可能とする（prefix chain を含む）。
`ch!x` の `x` は束縛済みであること（未束縛は `invalid_input`）。

### 値域
`channel ch : {0..N}` の場合、`ch.0`/`ch!0`/`ch?0` 等の payload は整数かつ `[0, N]` に収まること。
値域未指定の channel に payload が付く場合は `invalid_input` とする。

## 提案する IR 形状（擬似コード）
将来の実装差分が読みやすいよう、v0.2 では概ね以下の形を想定する（実装は後続 Issue）。

```rust
pub struct Module {
  pub channels: Vec<ChannelDecl>,
  pub processes: Vec<ProcessDecl>,
  pub assertions: Vec<AssertionDecl>,
  pub entry: Option<ProcessRef>,
}

pub struct ChannelDecl {
  pub names: Vec<Spanned<Ident>>,
  pub domain: Option<IntRange>, // None = no payload
}

pub struct IntRange { pub min: u64, pub max: u64 }

pub enum EventSeg {
  Dot(u64),
  Out(ValueExpr),
  In(Pattern),
}

pub enum ValueExpr { Int(u64), Var(Spanned<Ident>) }
pub enum Pattern { Int(u64), Bind(Spanned<Ident>) }

pub struct Event { pub channel: Spanned<Ident>, pub seg: Option<EventSeg> }

pub enum ProcessExpr {
  Stop,
  Ref(Spanned<Ident>),
  Prefix { event: Spanned<Event>, next: Box<Spanned<ProcessExpr>> },
  Choice { kind: ChoiceKind, left: Box<Spanned<ProcessExpr>>, right: Box<Spanned<ProcessExpr>> },
  Parallel { kind: ParallelKind, left: Box<Spanned<ProcessExpr>>, right: Box<Spanned<ProcessExpr>>, sync: EventSet },
  Hide { inner: Box<Spanned<ProcessExpr>>, hide: EventSet },
}

pub enum ChoiceKind { External, Internal }
pub enum ParallelKind { Interleaving, Interface }

pub struct EventSet { pub channels: Vec<Spanned<Ident>> }

pub enum AssertionDecl {
  Property { target: Spanned<Ident>, kind: PropertyKind, model: PropertyModel },
  Refinement { spec: Spanned<Ident>, model: RefinementModel, impl_: Spanned<Ident> },
}
```

## Span（SourceSpan）規約
Span 規約は `docs/frontend.md` に従う（1-based, inclusive）。
IR は「原因箇所（識別子/リテラル）へ最短距離で付与」を優先する。
