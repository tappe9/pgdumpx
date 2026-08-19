# pgdumpx

**PostgreSQL dumpを高速・安全に読み取り、必要なデータだけを抽出するPure Rustライブラリ。**

> ステータス: 設計段階。crate / CLI はまだリリースされていません。

pgdumpxは、PostgreSQLのCustom Format (`pg_dump -Fc`) アーカイブを、データベースへrestoreせずに検査・抽出するための再利用可能なRustエンジンです。

このプロジェクトは意図的に**read-only**です。`pg_dump`や`pg_restore`の代替を目指すのではなく、巨大dumpのメタデータ確認、必要なテーブルだけの選択的抽出、streaming読み取り、安全なバイナリ解析に集中します。

[English README](README.md)

## 目標

- PostgreSQLサーバーを必要としない**Pure Rust core**
- PostgreSQL Custom Formatの**read-only parser**
- `Read + Seek`を利用した**lazy / random access**
- テーブル全体をメモリへ載せない**streaming decompression**
- PostgreSQL `COPY` text形式を行・field単位で扱える**row-aware API**
- machine-readableなtyped error
- 悪意あるdumpに備えたresource limits
- CLI / Python / Arrow等を後から載せられる小さなCore
- 実測前に「高速」と断定せず、benchmarkで性能を証明する開発方針

## v0.1の対象

最初は次の形式だけに集中します。

```bash
pg_dump -Fc mydb > backup.dump
```

Archive Format Version **1.14〜1.16**を初期対象とし、古いversionやDirectory/Tar Formatは後続milestoneへ延期します。

圧縮は将来的に次をv0.1で扱う計画です。

- none
- gzip
- LZ4
- Zstandard

## 想定API

```rust
use pgdumpx::Archive;

let file = std::fs::File::open("backup.dump")?;
let mut archive = Archive::open(file)?;

for entry in archive.entries() {
    println!("{} {:?} {}", entry.id(), entry.kind(), entry.name());
}

let mut rows = archive.table_rows("public", "orders")?;
while let Some(row) = rows.next_row()? {
    println!("{:?}", row);
}
```

APIは実装前の設計契約であり、初回releaseまで変更される可能性があります。詳細は[docs/API-DESIGN.md](docs/API-DESIGN.md)を参照してください。

## 想定CLI

```bash
pgdumpx inspect backup.dump
pgdumpx list backup.dump
pgdumpx extract backup.dump public.orders
```

CLIはCore parserの別consumerとして実装し、解析ロジックを重複させません。

## 競合との位置づけ

既存のRustライブラリ[`libpgdump`](https://github.com/gmr/libpgdump)はCustom/Directory/Tar形式のread/write、lazy custom reader等を既に備えています。

pgdumpxは機能範囲をあえて狭くし、次へ集中します。

- read-only
- Custom Format first
- 巨大dumpのinspection / selective extraction
- `COPY` textのrow-aware parsing
- resource limitsとfuzzingを含むmalformed-input hardening
- 再現可能なbenchmarkに基づく性能改善

比較対象より速いという主張は、benchmarkで確認できるまでは行いません。

## ドキュメント

- [要件](docs/REQUIREMENTS.md)
- [アーキテクチャ](ARCHITECTURE.md)
- [Public API設計](docs/API-DESIGN.md)
- [Custom Format調査ノート](docs/PG-DUMP-CUSTOM-FORMAT.md)
- [ロードマップ](ROADMAP.md)
- [ADR](docs/adr/)
- [コントリビューション](CONTRIBUTING.md)
- [セキュリティポリシー](SECURITY.md)

## ライセンス

`MIT OR Apache-2.0` のデュアルライセンスです。
