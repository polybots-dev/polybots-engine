//! The device loop's copy guard is structural: every copy between the host and a batch's
//! device memory goes through `ef_copy` in kernel/device.cu, which counts the bytes
//! (tests/device_loop.rs checks the counts per step). This scan fails on a `cudaMemcpy`
//! anywhere else in a function that takes a batch (`EfDevice *d`).

/// Whether the line calls any cudaMemcpy function (cudaMemcpy, cudaMemcpyAsync, cudaMemcpy2D,
/// ...), as opposed to naming a copy kind such as cudaMemcpyHostToDevice.
fn calls_memcpy(line: &str) -> bool {
    line.match_indices("cudaMemcpy").any(|(at, m)| {
        let rest = &line[at + m.len()..];
        let name_end = rest
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        let suffix = &rest[..name_end];
        rest[name_end..].trim_start().starts_with('(')
            && !suffix.ends_with("ToDevice")
            && !suffix.ends_with("ToHost")
    })
}

#[test]
fn every_batch_copy_is_counted() {
    let src = include_str!("../kernel/device.cu");
    let mut bad = Vec::new();
    let mut function: Option<(String, bool)> = None;
    for (n, line) in src.lines().enumerate() {
        if !line.starts_with(' ')
            && !line.starts_with('}')
            && line.contains('(')
            && line.trim_end().ends_with('{')
        {
            let name = line
                .split('(')
                .next()
                .unwrap()
                .split_whitespace()
                .last()
                .unwrap_or("")
                .trim_start_matches('*');
            function = Some((name.to_owned(), line.contains("EfDevice *d")));
        }
        if let Some((name, takes_batch)) = &function {
            if *takes_batch && name != "ef_copy" && calls_memcpy(line) {
                bad.push(format!("line {}: {} in {name}", n + 1, line.trim()));
            }
        }
    }
    assert!(bad.is_empty(), "uncounted copies of a batch: {bad:#?}");
    // the scan sees the functions it is about
    assert!(src.contains("static cudaError_t ef_copy(EfDevice *d,"));
    assert!(src.matches("ef_copy(d, ").count() >= 10);
}

#[test]
fn the_scan_sees_every_copy_function() {
    assert!(calls_memcpy(
        "e = cudaMemcpy(a, b, n, cudaMemcpyHostToDevice);"
    ));
    assert!(calls_memcpy(
        "e = cudaMemcpyAsync(a, b, n, cudaMemcpyDeviceToHost, s);"
    ));
    assert!(calls_memcpy("cudaMemcpy2D (a, p, b, q, w, h, k);"));
    assert!(!calls_memcpy(
        "e = ef_copy(d, a, b, n, cudaMemcpyHostToDevice);"
    ));
}
