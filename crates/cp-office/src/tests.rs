use std::io::Write as _;

use super::{OfficeError, OfficeKind, read, version, write};

/// Minimal zip with the given entry names (empty bodies).
fn package(names: &[&str]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut buf);
        for name in names {
            zw.start_file(*name, zip::write::SimpleFileOptions::default()).expect("start");
            zw.write_all(b"<x/>").expect("body");
        }
        let _done = zw.finish().expect("finish");
    }
    buf.into_inner()
}

#[test]
fn kind_from_extension() {
    assert_eq!(OfficeKind::from_path("a/B.XLSX".as_ref()), Some(OfficeKind::Xlsx));
    assert_eq!(OfficeKind::from_path("deck.pptx".as_ref()), Some(OfficeKind::Pptx));
    assert_eq!(OfficeKind::from_path("doc.docx".as_ref()), Some(OfficeKind::Docx));
    assert_eq!(OfficeKind::from_path("old.xls".as_ref()), None);
}

#[test]
fn write_then_read_round_trips() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("book.xlsx");
    let bytes = package(&["[Content_Types].xml", "xl/workbook.xml"]);
    let v = write(&path, &bytes, None).expect("write");
    let file = read(&path).expect("read");
    assert_eq!(file.bytes, bytes);
    assert_eq!(file.version, v);
    assert_eq!(file.kind, OfficeKind::Xlsx);
}

#[test]
fn stale_base_is_a_conflict_and_leaves_file() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("deck.pptx");
    let first = package(&["[Content_Types].xml", "ppt/presentation.xml"]);
    let _v = write(&path, &first, None).expect("write");
    let second = package(&["[Content_Types].xml", "ppt/presentation.xml", "ppt/slides/slide1.xml"]);
    let err = write(&path, &second, Some("0-0")).expect_err("conflict");
    assert!(matches!(err, OfficeError::Conflict(_)));
    assert_eq!(std::fs::read(&path).expect("read"), first);
    let current = version(&path).expect("version");
    let _ok = write(&path, &second, Some(&current)).expect("matching base");
}

#[test]
fn rejects_wrong_kind_and_garbage() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("doc.docx");
    let xlsx = package(&["[Content_Types].xml", "xl/workbook.xml"]);
    assert!(matches!(write(&path, &xlsx, None), Err(OfficeError::Invalid(_))));
    assert!(matches!(write(&path, b"hello", None), Err(OfficeError::Invalid(_))));
    assert!(!path.exists());
}

#[test]
fn missing_file_is_not_found() {
    let dir = tempfile::tempdir().expect("tmp");
    assert!(matches!(read(&dir.path().join("doc.docx")), Err(OfficeError::NotFound)));
    assert_eq!(OfficeError::NotFound.status(), 404);
}
