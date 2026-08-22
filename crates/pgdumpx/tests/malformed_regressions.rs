use pgdumpx::{Archive, CopyRowReader, PgDumpError};
use std::io::Cursor;

const TRUNCATED_MAGIC: &[u8] = include_bytes!("../../../fuzz/corpus/archive_open/truncated_magic");
const OVERSIZED_TOC: &[u8] = include_bytes!("../../../fuzz/corpus/archive_open/oversized_toc.dump");
const DANGLING_ESCAPE: &[u8] = include_bytes!("../../../fuzz/corpus/copy_rows/dangling_escape");
const MALFORMED_TERMINATOR: &[u8] =
    include_bytes!("../../../fuzz/corpus/copy_rows/malformed_terminator");

#[test]
fn truncated_archive_corpus_returns_typed_eof() {
    let error = Archive::open(Cursor::new(TRUNCATED_MAGIC)).unwrap_err();
    assert!(matches!(error, PgDumpError::UnexpectedEof { offset: 3 }));
}

#[test]
fn oversized_toc_corpus_hits_the_structural_limit() {
    let error = Archive::open(Cursor::new(OVERSIZED_TOC)).unwrap_err();
    assert!(matches!(error, PgDumpError::TocEntryLimitExceeded { .. }));
}

#[test]
fn dangling_copy_escape_corpus_returns_typed_error() {
    let mut reader = CopyRowReader::new(Cursor::new(DANGLING_ESCAPE));
    let error = reader.next_row().unwrap_err();
    assert!(matches!(
        error,
        PgDumpError::MalformedCopyEscape {
            row: 1,
            byte_offset: 3,
        }
    ));
}

#[test]
fn unterminated_copy_terminator_corpus_returns_typed_error() {
    let mut reader = CopyRowReader::new(Cursor::new(MALFORMED_TERMINATOR));
    let error = reader.next_row().unwrap_err();
    assert!(matches!(
        error,
        PgDumpError::MalformedCopyTerminator {
            row: 1,
            byte_offset: 0,
        }
    ));
}
