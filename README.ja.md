# pgdumpx

**PostgreSQL Custom Formatをrestoreせず、byte-orientedなrowとして安全にscanするRustライブラリ / CLI。**

> ステータス: 初期実装段階。Cargo workspaceとbaseline CIは作成済みですが、crate / CLIはまだリリースされていません。

pgdumpxは、PostgreSQL Custom Format (`pg_dump -Fc`) archiveを、データベースへrestoreせずに検査するread-onlyのRustライブラリ / CLIです。

価値の中心は、単にarchive entryを開くことではありません。TOCから対象table-data entryを選択し、offsetへseekし、streaming decompressionし、PostgreSQL `COPY` textをlogicalなrow / fieldとしてparseし、predicateに一致した最初のrowで処理を停止できる一連の経路を提供することです。

代表的な目標は次です。

> 数GB〜数十GBの`-Fc` backupから、PostgreSQLを起動せず、対象テーブル全体をメモリへ載せずに、特定の受注・ユーザーなど1件を探す。

[English README](README.md)

## pgdumpxが解決すること

PostgreSQL Custom FormatにはTOCとentry単位のdata positionがあります。そのため対象**entry**へ選択的にアクセスできますが、列値からrow位置を引くrow-level indexは含まれていません。

pgdumpxはarchive layerとrow layerを、resource-boundedな1本のinspection pathとして構成します。

```text
PostgreSQL custom archive
        │
        ▼
header + TOC metadata
        │
        ▼
select table-data entry + validated seek
        │
        ▼
streaming decompression
        │
        ▼
PostgreSQL COPY text parser
        │
        ├── borrowed Row / byte-oriented Field
        ├── COPY column metadata / column lookup
        ├── structural / scan-work limits
        └── predicate evaluation / first-match retrieval
```

v0.1では次を重視します。

- PostgreSQL Custom Format (`-Fc`) の**read-only parser**
- 実行時にPostgreSQL server、`libpq`、`pg_restore`を必要としない構成
- `Read + Seek`による**lazy entry access**
- テーブル全体を保持しない**streaming decompression**
- 通常のpg_dump `COPY` textをrow / fieldとして扱うAPI
- UTF-8やrowごとのowned allocationを前提にしない**borrowed row / byte-oriented field API**
- SQL parserを導入しない**column-aware first-match filtering**
- location contextを持つtyped error
- structural、row scan、raw extractionを分けたresource limits
- CLIや将来のlanguage/data integrationから再利用できる小さなCore
- fixtureで裏付けた互換性と、benchmarkで裏付けた性能主張

`Pure Rust`という言葉を、全transitive dependencyに対する包括的な保証としては使用しません。default buildはPostgreSQL runtime componentから独立させ、compression backendやnative build上の制約は個別に文書化します。詳細は[ADR 0007](docs/adr/0007-standalone-row-scanner-and-vertical-slices.md)を参照してください。

## 想定ユースケース

PostgreSQL dumpをrestore専用ファイルではなく、offline data sourceとして扱いたい場面を対象にします。

- PostgreSQL serverを起動せずに本番backupを調査する
- 巨大なCustom Format dumpから特定の受注・ユーザーなど1件を探す
- 数GB〜数十GBのarchiveから1つのtable-data streamだけを抽出する
- backup verification、障害調査、support / forensics向けツールを作る
- 顧客や外部から受領したdumpを明示的なparser / work budgetの下で解析する
- 選択したrow streamをCSV、JSON Lines、Arrow、Parquet等へ変換する
- Rust coreを共通基盤としてCLIや将来のPython bindingを構築する

## v0.1の対象

v0.1はPostgreSQL Custom Formatだけに集中します。

```bash
pg_dump -Fc mydb > backup.dump
```

最終的なv0.1 targetはArchive Format Version **1.14〜1.16**と、次のcompressionです。

- none
- gzip
- LZ4
- Zstandard

実装はまずArchive 1.16 + none/gzipで`find`まで通る細いend-to-end sliceを完成させ、その後に互換性を広げます。詳細は[ROADMAP.md](ROADMAP.md)を参照してください。

row-aware APIが対象にするのは、通常のpg_dumpが生成するPostgreSQL `COPY` text形式のtable dataです。次はv0.1 row parserの対象外です。

- `--inserts`
- `--column-inserts`
- `--rows-per-insert`によるINSERT output
- Binary COPY

unsupported representationはCOPY textとして推測せず、row APIからtyped errorで失敗させます。archive entry自体が読める場合はraw extractionを利用できる可能性があります。

「設計上のtarget」と「fixture/testで実証済みの互換性」は[docs/COMPATIBILITY.md](docs/COMPATIBILITY.md)で分けて管理します。実装前の現時点では、互換性表はrelease済みの保証ではありません。

## 想定Rust API

現在のAlpha 2 sliceでは、以下のmetadata、row streaming、first-match、public structural-limit、scan-limit、error-taxonomy APIまで実装済みです。後続のv0.1 APIはROADMAPに従って追加します。

```rust
use pgdumpx::{Archive, FieldRef};

let file = std::fs::File::open("backup.dump")?;
let mut archive = Archive::open(file)?;

println!("archive version: {:?}", archive.header().version());

for entry in archive.entries() {
    println!("{entry:?}");
}

let mut rows = archive.table_rows(b"public", b"orders")?;
while let Some(row) = rows.next_row()? {
    println!("{:?}", row);
}
```

`next_row(&mut self)`を通常の`Iterator`にしないのは意図的です。borrowed `Row`は再利用可能な内部bufferを参照し、次のmutable operationまでだけ有効です。

明示的なtotal-work budgetの下で、最初に一致する1行を取得できます。

```rust
use pgdumpx::ScanLimits;

let mut rows = archive.table_rows(b"public", b"orders")?;
let order_number = rows
    .column_index(b"order_number")?
    .ok_or(/* application error */)?;

let scan_limits = ScanLimits::unlimited()
    .with_max_rows(100_000)
    .with_max_decompressed_bytes(64 * 1024 * 1024);

let row = rows.find_first_with_limits(scan_limits, |row| {
    row.field(order_number) == Some(FieldRef::Bytes(b"123456"))
})?;
```

`find_first`は追加のtotal-work budgetを適用しないconvenience pathとして維持します。`find_first_with_limits`は同じstreaming parser / predicate loopへ`ScanLimits`を適用します。

`column_index()`は次を区別します。

```text
Ok(Some(index))  metadataが正常でcolumnが見つかった
Ok(None)         metadataは正常だがrequested columnが存在しない
Err(...)         column layoutを利用できない、またはmalformed
```

どちらのfirst-match methodもDBのindex lookupではなく**streaming sequential scan**です。TOCによって対象table-data entryへ直接seekできますが、entry内のrowは先頭からdecompress / parseします。早い位置で一致すれば即終了できますが、late matchやno matchでは、configured budgetが停止させない限り対象entry全体を処理する可能性があります。

APIでは次を別のlimitとして扱います。

- structural / per-item limits
- row scan全体のwork limits
- raw entry extractionのdecompressed-byte limit

実装済みのstructural configurationは`Limits`です。`Limits::default()`はfiniteなcompatibility-oriented defaultで、`Archive::open`はこれを利用します。`Archive::open_with_limits`では同じparser pathに対してTOC / string / dependency / row / fieldのより厳しいlimitをcallerが指定できます。

実装済みのtotal-work configurationは`ScanLimits`です。`ScanLimits::default()`と`ScanLimits::unlimited()`は2つのoptional budgetを未設定にします。`max_rows = N`では、一致rowを含めて最大`N`件のcomplete rowだけをyield / evaluateできます。decompressed-byte budgetは、field separator、row terminator、escape spelling、消費したCOPY terminatorを含む、parserが消費した物理COPY byteを数えます。logical fieldのdecode後lengthや、decoder / `BufRead`が先読みした未消費byteは数えません。budgetをcrossするrowはyieldもpredicate評価もされず、limit / consumed-work contextを持つtyped resource errorを返します。

詳細は[docs/API-DESIGN.md](docs/API-DESIGN.md)を参照してください。

## CLI

`inspect`、`list`、`find`は実装済みで、archive / COPY parserをCLI側へ重複実装せずpublic Rust APIを利用します。`extract`は後続のAlpha 2 Issueで実装予定です。

```bash
pgdumpx inspect backup.dump
pgdumpx list backup.dump
pgdumpx extract backup.dump public.orders
pgdumpx find backup.dump public.orders order_number 123456
pgdumpx find --max-rows 100000 --max-decompressed-bytes 67108864 \
  backup.dump public.orders order_number 123456
```

### `inspect` / `list`

`inspect <FILE>`はarchive version、compression、entry / table / table-data件数をdeterministicな`key=value`形式で表示します。`list <FILE>`はTOC orderを維持し、dump ID、object type、schema、nameをtab区切りで表示します。どちらもlibraryのmetadata-open pathだけを使い、TABLE DATA payloadへのseek、entry decompression、COPY row parserには到達しません。diagnosticはstderrへ出力し、malformed archiveはnon-zero exitになります。

### `extract`（planned）

選択したentryの**decompressed table-data body**をbinary-safeなbytesとしてstdoutへ出力します。schema DDL、`COPY` statement wrapper、restore可能な完全SQLは追加しません。

CLIはlibraryのbounded raw-extraction pathを利用します。limit到達時はerrorとし、出力を黙ってtruncateしません。

### `find`

最初の一致rowを取得する狭いequality commandです。SQL風`WHERE` parserや汎用condition DSLは導入しません。

v0.1の形式は次のとおりです。

```text
pgdumpx find [--max-rows <N>] [--max-decompressed-bytes <N>] <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
```

optionalなscan-limit flagは`<FILE>`より前に指定します。各flagは正の10進`u64`を1回まで指定でき、互いに独立しています。省略したbudgetはunlimitedです。`--max-rows <N>`はlibraryのsearch pathがevaluateしたcomplete rowを数え、一致rowも含みます。`--max-decompressed-bytes <N>`はRust APIと同じparser-consumed physical COPY-byte会計を使います。separator、row terminator、escape spelling、消費したCOPY terminatorを含みますが、decompressor / bufferの未消費lookaheadやlogical decodeによるlength変化は含みません。

`<SCHEMA.TABLE>`はASCIIの`.`をちょうど1個含み、schema / tableの両componentをnon-emptyとします。SQL identifierのquote / escapeは未対応です。schema、table、column、value argumentはUTF-8で、Rust APIはbyte-orientedのままです。

一致時はstdoutへ**正規化したCOPY text 1 record**だけを出力します。fieldはCOPY column順のASCII tab区切りで、record末尾はLFです。NULLは`\N`、empty bytesはempty fieldです。backslash / tab / LF / CRは`\\` / `\t` / `\n` / `\r`、その他のnon-printableまたはnon-ASCII byteは`\377`のような3桁octal escapeで出力します。lossy UTF-8変換を行わず、stdoutをdeterministicかつASCII-safeに保ちます。no matchでは何も出力せず、diagnosticはstderrだけへ出力します。

resource limit到達はclean no-matchではなくoperation failureです。一致rowへ到達する前にbudgetを使い切った場合も、stderrへdiagnosticを出し、`2+`で終了します。

stable exit behavior:

```text
0  match found
1  scan完了、matching rowなし
2+ usage / I/O / format / integrity / decompression / COPY / encoding / resource error
```

## Architecture

archive openではmetadataとTOC indexだけを構築し、payloadはlazyに読みます。

```text
Archive<R: Read + Seek>
        │
        ├── header + TOC parser
        ├── ArchiveIndex
        └── on-demand validated seek
                  │
                  ▼
          EntryDataReader
                  │
          streaming decompression
                  │
                  ▼
          COPY text row reader
                  │
                  ├── row iteration
                  └── first-match filtering
```

byte-oriented metadata、integrity check、resource accounting、row-parser errorを1つのmodelで扱えるよう、狭いstandalone read pathを実装します。関連dump libraryはresearch referenceやdifferential test comparatorとして利用できます。

詳細は[ARCHITECTURE.md](ARCHITECTURE.md)を参照してください。

## COPY text contract

`FieldRef::Bytes`はarchive上のescaped spellingではなく、**PostgreSQL COPY text escape decode後のlogical field bytes**を表します。`\N`は`FieldRef::Null`、空の非NULL fieldは長さ0のbytesです。

row framing、escape、column metadata、unsupported representation、resource limitsは[docs/COPY-TEXT.md](docs/COPY-TEXT.md)で定義します。

## Evidence policy

互換性と性能は、実装意図ではなくevidenceが必要なclaimです。

valid archive fixtureには次を記録します。

- official `pg_dump` generator version
- exact generation command
- archive-format version / compression
- checksum
- fixture purpose / expected objects

benchmarkにはfixture、command、hardware、compression、match position、measurement methodを記録します。再現可能な結果が出るまではREADMEへ性能優位を記載しません。

## 関連プロジェクト

- [`libpgdump`](https://github.com/gmr/libpgdump) — PostgreSQL Custom / Directory / Tar dump形式をread/writeするRustライブラリ
- [`pgdumplib`](https://github.com/gmr/pgdumplib) — PostgreSQL Custom Formatをread/writeするPythonライブラリ

これらはPostgreSQL dumpを扱う隣接プロジェクトです。pgdumpxは、Custom Formatをread-only・resource-bounded・byte-orientedなrowとして検査する狭い責務に集中します。

## ドキュメントmap

重複とdriftを減らすため、各documentのprimary responsibilityを分けます。

- [README](README.md) / [日本語 README](README.ja.md) — product value、status、example、high-level scope
- [Requirements](docs/REQUIREMENTS.md) — normativeなv0.1 behavior / acceptance criteria
- [Architecture](ARCHITECTURE.md) — internal boundary / data flow
- [Public API design](docs/API-DESIGN.md) — Rust APIとexact semantics
- [Custom archive format notes](docs/PG-DUMP-CUSTOM-FORMAT.md) — upstream由来のarchive behavior
- [COPY text contract](docs/COPY-TEXT.md) — row / field byte semantics
- [Compatibility matrix](docs/COMPATIBILITY.md) — targetとfixture-verified support
- [Roadmap](ROADMAP.md) — delivery order
- [ADR](docs/adr/) — accepted / superseded decision
- [Contributing](CONTRIBUTING.md) — contribution / document update policy
- [Security policy](SECURITY.md) — vulnerability reporting / resource threat model

## ライセンス

`MIT OR Apache-2.0`のデュアルライセンスです。
