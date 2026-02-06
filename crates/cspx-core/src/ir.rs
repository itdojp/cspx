//! Core IR（v0.2 設計方針）
//!
//! v0.2 では Frontend（parse/typecheck）の入力として CSPM サブセットを拡張し、後続の LTS/Checker/Refinement が
//! 追加の構文解釈に依存しないよう、IR の責務を明確化する。
//!
//! - AST: 字句/構文情報を保持し、Span を広く付与（実装は別モジュール）
//! - IR: 参照解決・型（値域）整合を済ませ、探索/検査が利用しやすい形に正規化
//!
//! IR で扱う予定の要素（v0.2）
//! - channel 宣言（名前、値域 `{0..N}` のみ）
//! - process 式（STOP / prefix / choice / internal choice / interleaving / interface parallel / hiding / proc ref）
//! - assert 宣言（deadlock/divergence/deterministic、refinement T/F/FD）
//!
//! エラー分類は `docs/frontend.md`（`unsupported_syntax` / `invalid_input`）に従う。
//! Span は `SourceSpan`（1-based, inclusive）を基本とし、原因箇所（識別子/リテラル）へ最短距離で付与する。
//! 詳細な IR 形状案は `docs/ir.md` を参照。

use crate::types::SourceSpan;
use std::fmt::Debug;

pub trait CoreIr: Debug + Send + Sync {}

#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub value: T,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub enum ProcessExpr {
    Stop,
}

#[derive(Debug, Clone)]
pub struct ProcessDecl {
    pub name: String,
    pub expr: Spanned<ProcessExpr>,
}

#[derive(Debug, Clone)]
pub struct Module {
    pub declarations: Vec<ProcessDecl>,
    pub entry: Option<Spanned<ProcessExpr>>,
}

impl CoreIr for Module {}
