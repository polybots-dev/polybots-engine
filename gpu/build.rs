//! Builds the kernel sources twice: always as a host C library (the twin), and with the
//! `cuda` feature also as device code through nvcc.
//!
//! Both builds switch off what would let floating point differ from Rust: no merging of a
//! multiply and an add (`-ffp-contract=off`, `--fmad=false`) and no fast-math. The kernel code
//! never relies on signed overflow: arithmetic that wraps in Rust is written on unsigned
//! types (`ef_wadd` and the rest). `-fwrapv` on the host build is a second guard only; nvcc
//! has no such flag.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=kernel");
    println!("cargo:rerun-if-env-changed=EF_NVCC");
    println!("cargo:rerun-if-env-changed=EF_NVCC_HOST");
    println!("cargo:rerun-if-env-changed=EF_CUDA_ARCH");
    println!("cargo:rerun-if-env-changed=EF_SANITIZE");
    println!("cargo:rerun-if-env-changed=EF_NVCC_LINEINFO");
    // EF_SANITIZE=1: the host build under AddressSanitizer and UndefinedBehaviorSanitizer, so
    // a read or write outside an array, which the state comparison cannot see when it lands
    // in unused room, stops the test
    let sanitize = std::env::var_os("EF_SANITIZE").is_some();
    let mut twin = cc::Build::new();
    if sanitize {
        for flag in [
            "-fsanitize=address,undefined",
            "-fno-sanitize-recover=all",
            "-fno-omit-frame-pointer",
            "-g",
        ] {
            twin.flag(flag);
        }
        // Rust links without the C compiler's default libraries, so the runtimes are named
        if cfg!(target_os = "macos") {
            let dir = Command::new("clang")
                .arg("-print-resource-dir")
                .output()
                .expect("clang");
            let dir = format!("{}/lib/darwin", String::from_utf8_lossy(&dir.stdout).trim());
            println!("cargo:rustc-link-search=native={dir}");
            println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
            println!("cargo:rustc-link-lib=dylib=clang_rt.asan_osx_dynamic");
            println!("cargo:rustc-link-lib=dylib=clang_rt.ubsan_osx_dynamic");
        } else {
            println!("cargo:rustc-link-lib=dylib=asan");
            println!("cargo:rustc-link-lib=dylib=ubsan");
        }
    }
    twin.file("kernel/twin.c")
        .include("kernel")
        .std("c11")
        .flag("-ffp-contract=off")
        .flag("-fwrapv")
        .flag("-fno-fast-math")
        // no implicit padding: every byte of a block is a field, so blocks compare as bytes
        .flag("-Werror=padded")
        // no implicit narrowing: a narrowed field (`_n`, tests/narrow.rs) takes no wider value
        // unconverted
        .flag("-Werror=conversion")
        // a list helper given a list of another width reads the wrong bytes
        .flag("-Werror=incompatible-pointer-types")
        .warnings(true)
        .extra_warnings(true)
        .compile("ef_twin");
    println!("cargo:rerun-if-env-changed=EF_CHECK_ONLY");
    // EF_CHECK_ONLY: type-check the `cuda` feature's Rust code (`cargo check`) on a machine
    // without the CUDA toolkit; nothing built this way can be linked
    if std::env::var_os("CARGO_FEATURE_CUDA").is_none()
        || std::env::var_os("EF_CHECK_ONLY").is_some()
    {
        return;
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let nvcc = std::env::var("EF_NVCC").unwrap_or_else(|_| "nvcc".into());
    // CUDA 12.0 supports GCC up to 12; the server's default is 13
    let host = std::env::var("EF_NVCC_HOST").unwrap_or_else(|_| "g++-12".into());
    let arch = std::env::var("EF_CUDA_ARCH").unwrap_or_else(|_| "sm_86".into());
    let object = out.join("ef_device.o");
    let status = Command::new(&nvcc)
        .args([
            "-O2",
            "--fmad=false",
            // IEEE division and square root and no flush to zero are nvcc's defaults; named
            // so that a changed default cannot change results
            "-prec-div=true",
            "-prec-sqrt=true",
            "-ftz=false",
            "-Xcompiler",
            "-fPIC",
            "-ccbin",
            &host,
        ])
        .arg(format!("-arch={arch}"))
        // EF_NVCC_LINEINFO: source lines for Nsight Compute (no change to the generated code)
        .args(std::env::var_os("EF_NVCC_LINEINFO").map(|_| "-lineinfo"))
        .args(["-Ikernel", "-c", "kernel/device.cu", "-o"])
        .arg(&object)
        .status()
        .unwrap_or_else(|e| panic!("could not run {nvcc}: {e}"));
    assert!(status.success(), "nvcc failed on kernel/device.cu");
    let library = out.join("libef_device.a");
    let _ = std::fs::remove_file(&library);
    let status = Command::new("ar")
        .arg("crs")
        .arg(&library)
        .arg(&object)
        .status()
        .expect("could not run ar");
    assert!(status.success(), "ar failed");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=ef_device");
    println!("cargo:rustc-link-lib=dylib=cudart");
    println!("cargo:rustc-link-lib=dylib=stdc++");
}
