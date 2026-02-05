# Frontend（M1）対応サブセット

## 目的
M1 では `cspx typecheck` の最小実装として、CSPM の対応サブセットを限定する。
以降の拡張は M2 以降で段階的に広げる。

## 対応構文（M1）
- `STOP`（トップレベル式として可）
- 単純なプロセス定義: `NAME = STOP`
  - `NAME` は `[A-Za-z_][A-Za-z0-9_]*`
- 行コメント: `--` 以降は無視

## 非対応（M1）
- それ以外の CSPM 構文は `unsupported` として扱う
- 複数のトップレベル式は `invalid_input`

## エラー分類
- 構文的に未対応: `unsupported_syntax`（status: `unsupported`, exit code: 3）
- 入力不正（識別子不正/重複/空ファイル等）: `invalid_input`（status: `error`, exit code: 2）
