use crate::{Compression, ErrorCategory, PgDumpError, ResourceLimit};
use std::{error::Error as _, io};

#[test]
fn callers_can_classify_representative_errors_without_display_parsing() {
    let utf8_source = String::from_utf8(vec![0xff]).unwrap_err().utf8_error();
    let cases = [
        (
            PgDumpError::Io {
                offset: 4,
                source: io::Error::other("read failed"),
            },
            ErrorCategory::Io,
        ),
        (
            PgDumpError::InvalidArchiveMagic { offset: 0 },
            ErrorCategory::Format,
        ),
        (
            PgDumpError::DataBlockDumpIdMismatch {
                expected: 7,
                actual: 8,
                offset: 41,
            },
            ErrorCategory::Integrity,
        ),
        (
            PgDumpError::DecompressionFailed {
                dump_id: 7,
                algorithm: "gzip",
                source: io::Error::new(io::ErrorKind::InvalidData, "bad stream"),
            },
            ErrorCategory::Decompression,
        ),
        (
            PgDumpError::MalformedCopyEscape {
                row: 2,
                byte_offset: 19,
            },
            ErrorCategory::Copy,
        ),
        (
            PgDumpError::UnsupportedTableDataRepresentation {
                dump_id: 7,
                representation: crate::TableDataRepresentation::Insert,
            },
            ErrorCategory::Representation,
        ),
        (
            PgDumpError::InvalidUtf8 {
                context: "archive metadata",
                source: utf8_source,
            },
            ErrorCategory::Encoding,
        ),
        (
            PgDumpError::ArchiveStringLimitExceeded {
                length: 11,
                limit: 10,
                offset: 5,
            },
            ErrorCategory::Resource,
        ),
        (
            PgDumpError::ArithmeticOverflow { offset: 9 },
            ErrorCategory::Arithmetic,
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.category(), expected, "error={error}");
    }
}

#[test]
fn representative_errors_expose_typed_context() {
    let truncation = PgDumpError::UnexpectedEof { offset: 123 };
    assert_eq!(truncation.byte_offset(), Some(123));

    let mismatch = PgDumpError::DataBlockDumpIdMismatch {
        expected: 17,
        actual: 18,
        offset: 456,
    };
    assert_eq!(mismatch.byte_offset(), Some(456));
    assert_eq!(mismatch.dump_id().unwrap().as_i32(), 17);

    let malformed = PgDumpError::MalformedCopyEscape {
        row: 9,
        byte_offset: 321,
    };
    assert_eq!(malformed.row_number(), Some(9));
    assert_eq!(malformed.byte_offset(), Some(321));

    let decompression = PgDumpError::DecompressionFailed {
        dump_id: 20,
        algorithm: "gzip",
        source: io::Error::new(io::ErrorKind::InvalidData, "bad stream"),
    };
    assert_eq!(decompression.dump_id().unwrap().as_i32(), 20);
    assert_eq!(decompression.compression(), Some(Compression::Gzip));
}

#[test]
fn structural_limit_errors_expose_resource_limit_and_consumed_work() {
    let error = PgDumpError::ArchiveStringLimitExceeded {
        length: 11,
        limit: 10,
        offset: 5,
    };
    let context = error.limit_context().expect("resource limit context");

    assert_eq!(context.resource(), ResourceLimit::ArchiveStringBytes);
    assert_eq!(context.limit(), 10);
    assert_eq!(context.consumed(), 11);
}

#[test]
fn io_decompression_and_typed_io_adapter_sources_remain_reachable() {
    let io_error = PgDumpError::Io {
        offset: 0,
        source: io::Error::other("read failed"),
    };
    assert!(io_error.source().is_some());

    let decompression = PgDumpError::DecompressionFailed {
        dump_id: 1,
        algorithm: "gzip",
        source: io::Error::new(io::ErrorKind::InvalidData, "bad stream"),
    };
    assert!(decompression.source().is_some());

    let adapted = crate::error::into_io_error(PgDumpError::ArchiveStringLimitExceeded {
        length: 2,
        limit: 1,
        offset: 0,
    });
    let typed = adapted
        .get_ref()
        .and_then(|source| source.downcast_ref::<PgDumpError>())
        .expect("typed pgdumpx error must be the io::Error source");
    assert_eq!(typed.category(), ErrorCategory::Resource);
}
