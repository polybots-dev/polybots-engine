//! The narrowed fields of the block: a field narrower than the value it
//! holds in the Rust engine is named with the suffix `_n`, and every write to one in the kernel
//! sources must carry a `narrow:` note on its line saying why the value fits (a constant, a copy
//! from another narrowed field, a bound the line itself checks) or go through a setter that
//! refuses what does not fit. This test finds every such write and fails on any without the
//! note, so an unchecked write cannot be added unseen. `-Werror=conversion` in the host build
//! catches a plain assignment from a wider value on top; neither catches `+= 1` alone.
//!
//! Not covered: writes through a pointer to a narrowed field's element (`*p = v` with `p` taken
//! from one), whole-struct copies and `memset` (both copy narrowed values or zero). The pointer
//! writes are in `ef_remove_at_i8` and `ef_effect_remove_at` (core.h, shifting entries and
//! lowering the length) and `ef_modify`'s `field` (reward.h, the engine's u16 wrap); each has its
//! line note.

const ASSIGN: [&str; 10] = ["<<=", ">>=", "+=", "-=", "*=", "/=", "|=", "&=", "^=", "="];

/// The writes to `_n` fields on `line`: the field and the operator.
fn writes(line: &str) -> Vec<(String, String)> {
    let code = line.split("/*").next().unwrap_or(line);
    let b = code.as_bytes();
    let mut out = Vec::new();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = 0;
    while i < b.len() {
        if !word(b[i]) || (i > 0 && word(b[i - 1])) {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && word(b[i]) {
            i += 1;
        }
        let name = &code[start..i];
        if !name.ends_with("_n") {
            continue;
        }
        // the access path before the name (`x->`, `g->a[k].`), then a prefix ++ or --
        let mut p = start;
        loop {
            let s = &code[..p];
            if s.ends_with("->") {
                p -= 2;
            } else if s.ends_with('.') {
                p -= 1;
            } else if s.ends_with(']') {
                p = s.rfind('[').unwrap_or(0);
            } else if s.ends_with(|c: char| c.is_alphanumeric() || c == '_') {
                p = s
                    .trim_end_matches(|c: char| c.is_alphanumeric() || c == '_')
                    .len();
            } else {
                break;
            }
        }
        let before = code[..p].trim_end();
        if before.ends_with("++") || before.ends_with("--") {
            out.push((name.to_owned(), "prefix".into()));
        }
        // skip index brackets after the name
        let mut j = i;
        loop {
            while j < b.len() && b[j] == b' ' {
                j += 1;
            }
            if j < b.len() && b[j] == b'[' {
                let mut depth = 0;
                while j < b.len() {
                    if b[j] == b'[' {
                        depth += 1;
                    } else if b[j] == b']' {
                        depth -= 1;
                        if depth == 0 {
                            j += 1;
                            break;
                        }
                    }
                    j += 1;
                }
            } else {
                break;
            }
        }
        let rest = &code[j..];
        if rest.starts_with("++") || rest.starts_with("--") {
            out.push((name.to_owned(), rest[..2].to_owned()));
        } else if let Some(op) = ASSIGN.iter().find(|op| rest.starts_with(**op)) {
            if !(*op == "=" && rest.starts_with("==")) {
                out.push((name.to_owned(), (*op).to_owned()));
            }
        }
    }
    out
}

#[test]
fn every_write_to_a_narrowed_field_says_why_it_fits() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/kernel");
    let mut found = 0;
    let mut missing = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if !path
            .extension()
            .is_some_and(|e| e == "h" || e == "c" || e == "cu")
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for (k, line) in text.lines().enumerate() {
            for (field, op) in writes(line) {
                found += 1;
                if !line.contains("narrow:") {
                    let file = path.file_name().unwrap().to_string_lossy();
                    missing.push(format!("{file}:{}: {field} {op}", k + 1));
                }
            }
        }
    }
    assert!(
        found > 0,
        "no write to a narrowed field found: the scan is broken"
    );
    assert!(
        missing.is_empty(),
        "writes without a narrow: note: {missing:#?}"
    );
}

/// The scan itself finds each form of write and leaves reads and comparisons alone.
#[test]
fn the_scan_finds_each_form_of_write() {
    let forms = [
        "x->len_n = 0;",
        "x->len_n += k;",
        "x->len_n++;",
        "--x->len_n;",
        "l->items_n[l->len_n] = v;",
        "g->a[k].used_n |= 1;",
    ];
    for f in forms {
        assert_eq!(writes(f).len(), 1, "{f}");
    }
    for f in [
        "if (x->len_n == 0) y = 1;",
        "int k = l->items_n[i];",
        "f(x->len_n);",
    ] {
        assert!(writes(f).is_empty(), "{f}");
    }
    assert_eq!(writes("l->items_n[l->len_n++] = v;").len(), 2);
}

extern "C" {
    fn ef_twin_put(v: i64, out: *mut i32, stored: *mut i64);
}

/// EF_PUT stores a value its field holds and refuses one past either bound, storing nothing.
#[test]
fn a_checked_store_refuses_one_past_each_bound() {
    let put = |v: i64| {
        let (mut out, mut stored) = ([0i32; 4], [0i64; 4]);
        // SAFETY: four results and four stored values are written.
        unsafe { ef_twin_put(v, out.as_mut_ptr(), stored.as_mut_ptr()) };
        (out.map(|c| c == 0), stored)
    };
    // int8, uint8, int16, uint16
    let bounds: [(i64, i64); 4] = [(-128, 127), (0, 255), (-32_768, 32_767), (0, 65_535)];
    for (k, (lo, hi)) in bounds.into_iter().enumerate() {
        for v in [lo, hi] {
            let (ok, stored) = put(v);
            assert!(ok[k] && stored[k] == v, "type {k}: {v} stored");
        }
        for v in [lo - 1, hi + 1] {
            let (ok, stored) = put(v);
            assert!(
                !ok[k] && stored[k] == 0,
                "type {k}: {v} refused, nothing stored"
            );
        }
    }
    // the refusal is the field-range error, a device capacity
    let code = {
        let (mut out, mut stored) = ([0i32; 4], [0i64; 4]);
        // SAFETY: as above.
        unsafe { ef_twin_put(300, out.as_mut_ptr(), stored.as_mut_ptr()) };
        out[0]
    };
    assert!(engine_fast_gpu::step::is_capacity(code));
}
