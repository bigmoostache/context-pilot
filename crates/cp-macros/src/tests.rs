//! Expansion tests for `deref_match!`: borrow-not-move semantics on every
//! place shape the workspace uses (`self`, `&T`, `&&T`, `&Result`).

/// Mixed enum: one non-`Copy` payload (needs `ref`), one `Copy`, one unit.
enum Sample {
    /// Non-`Copy` payload, bound with `ref`.
    Text(String),
    /// `Copy` payload, bound by value.
    Num(u32),
    /// No payload.
    Empty,
}

impl Sample {
    /// Calls the macro on `self`, the most common call shape (Display impls).
    fn label(&self) -> String {
        crate::deref_match!(self, {
            Self::Text(ref s) => s.clone(),
            Self::Num(n) => n.to_string(),
            Self::Empty => String::new(),
        })
    }
}

/// Struct-variant enum, mirroring `ChannelMismatch { ref expected, ref found }`.
enum Pair {
    /// Two non-`Copy` fields bound by `ref`.
    Both {
        /// First field.
        left: String,
        /// Second field.
        right: String,
    },
}

#[test]
fn binds_non_copy_by_ref_without_moving() {
    let v = Sample::Text("hello".to_owned());
    assert_eq!(v.label(), "hello");
    // `v` still owns its String: the macro borrowed, it did not move.
    assert_eq!(v.label(), "hello");
}

#[test]
fn copy_and_unit_arms() {
    assert_eq!(Sample::Num(7).label(), "7");
    assert_eq!(Sample::Empty.label(), "");
}

#[test]
fn ref_binding_points_into_the_original() {
    let text = "x".to_owned();
    // Moving a String into the enum keeps its heap buffer in place.
    let heap = text.as_ptr();
    let v = Sample::Text(text);
    let inner: &str = crate::deref_match!(&v, {
        Sample::Text(ref s) => s.as_str(),
        Sample::Num(_) | Sample::Empty => "",
    });
    assert_eq!(inner.as_ptr(), heap);
}

#[test]
fn struct_variant_fields() {
    let p = Pair::Both { left: "l".to_owned(), right: "r".to_owned() };
    let joined = crate::deref_match!(&p, {
        Pair::Both { ref left, ref right } => format!("{left}{right}"),
    });
    assert_eq!(joined, "lr");
}

#[test]
fn nested_result_place() {
    let ok: Result<Sample, u8> = Ok(Sample::Text("ok".to_owned()));
    let got_ok = crate::deref_match!(&ok, {
        Ok(Sample::Text(ref s)) => s.clone(),
        Ok(Sample::Num(_) | Sample::Empty) => "other".to_owned(),
        Err(code) => code.to_string(),
    });
    assert_eq!(got_ok, "ok");
    let err: Result<Sample, u8> = Err(4);
    let got_err = crate::deref_match!(&err, {
        Ok(Sample::Text(ref s)) => s.clone(),
        Ok(Sample::Num(_) | Sample::Empty) => "other".to_owned(),
        Err(code) => code.to_string(),
    });
    assert_eq!(got_err, "4");
}
