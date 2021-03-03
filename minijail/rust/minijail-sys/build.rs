// Copyright 2019 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use itertools::Itertools;
use regex::Regex;
use std::{env, fs, io, io::Write, path::PathBuf};

/// This is a 1:1 port of gen_syscalls.sh in libminijail
fn generate_syscall_table() -> io::Result<PathBuf> {
    let artifact = PathBuf::from(env::var("OUT_DIR").unwrap()).join("libsyscalls.gen.c");

    let expanded = cc::Build::new()
        .flag("-dD")
        .flag("../../gen_syscalls.c")
        .expand();
    let preproc = String::from_utf8(expanded).expect("Invalid compiler output");

    let mut out = fs::File::create(&artifact)?;
    writeln!(out, "/* GENERATED by build.rs */")?;
    writeln!(out, "#include <stddef.h>")?;
    writeln!(out, "#include <asm/unistd.h>")?;
    writeln!(out, "#include \"libsyscalls.h\"")?;
    writeln!(out, "const struct syscall_entry syscall_table[] = {{")?;

    let re = Regex::new("#define __(ARM_)?(NR_)([[:lower:]0-9_]*) (.*)$").expect("Invalid regex");
    preproc.lines().try_for_each(|line| -> io::Result<()> {
        if let Some(c) = re.captures(&line) {
            let nr = &c[2];
            let name = &c[3];
            writeln!(out, "#ifdef __{}{}", nr, name)?;
            writeln!(out, "{{ \"{}\", __{}{} }},", name, nr, name)?;
            writeln!(out, "#endif")?;
        }
        Ok(())
    })?;

    writeln!(out, "{{ NULL, -1 }},")?;
    writeln!(out, "}};")?;

    Ok(artifact)
}

/// This is a 1:1 port of gen_constants.sh in libminijail
fn generate_syscall_constants(target_os: &str) -> io::Result<PathBuf> {
    let artifact = PathBuf::from(env::var("OUT_DIR").unwrap()).join("libconstants.gen.c");

    let expanded = cc::Build::new()
        .flag("-dD")
        .flag("../../gen_constants.c")
        .expand();
    let preproc = String::from_utf8(expanded).expect("Invalid compiler output");

    let mut out = fs::File::create(&artifact)?;
    writeln!(out, "/* GENERATED by build.rs */")?;
    writeln!(out, "#include \"gen_constants-inl.h\"")?;
    writeln!(out, "#include \"libconstants.h\"")?;
    writeln!(out, "const struct constant_entry constant_table[] = {{")?;

    let re = Regex::new("#define ([[:upper:]][[:upper:]0-9_]*).*$").expect("Invalid regex");
    let f = Regex::new("^#define [[:upper:]][[:upper:]0-9_]*(\\s)+[[:alnum:]_]")
        .expect("Invalid redgex");
    preproc
        .lines()
        .filter(|l| !l.contains("SIGRTMAX"))
        .filter(|l| !l.contains("SIGRTMIN"))
        .filter(|l| !l.contains("SIG_"))
        .filter(|l| !l.contains("NULL"))
        .filter(|l| f.is_match(&l))
        .sorted_by_key(|&l| l)
        .try_for_each(|line| -> io::Result<()> {
            if let Some(c) = re.captures(&line) {
                let name = &c[1];
                writeln!(out, "#ifdef {}", name)?;
                writeln!(out, "  {{ \"{}\", (unsigned long) {} }},", name, name)?;
                writeln!(out, "#endif  // {}", name)?;
            }
            Ok(())
        })?;

    // Add Android special arg for pr_ctl
    if target_os == "android" {
        writeln!(out, "#ifdef PR_SET_VMA")?;
        writeln!(out, "{{ \"PR_SET_VMA\", 0x53564d41 }}")?;
        writeln!(out, "#endif  // PR_SET_VMA")?;
    }

    writeln!(out, "{{ NULL, 0 }},")?;
    writeln!(out, "}};")?;

    Ok(artifact)
}

fn main() -> io::Result<()> {
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("Failed to get CARGO_CFG_TARGET_OS");

    match target_os.as_str() {
        "linux" | "android" => (),
        _ => return Ok(()),
    };

    let minijail_dir = env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .expect("Faild to get CARGO_MANIFEST_DIR")
        .join("../..");

    let sources = &[
        "../../bpf.c",
        "../../libminijail.c",
        "../../libmj_netns.c",
        "../../libmj_perms.c",
        "../../libmj_vm.c",
        "../../signal_handler.c",
        "../../syscall_filter.c",
        "../../syscall_wrapper.c",
        "../../system.c",
        "../../util.c",
        "../../../libnetlink/libnetlink.c",
        "../../libmj_netlink.c",
    ];

    let mut build = cc::Build::new();

    build
        .define("ALLOW_DEBUG_LOGGING", "1")
        .define("PRELOADPATH", "\"invalid\"")
        .flag("-Wno-implicit-function-declaration")
        .flag("-I../../../libcap-sys")
        .flag("-I../../../libnetlink")
        .files(sources)
        .file(generate_syscall_constants(&target_os)?)
        .file(generate_syscall_table()?)
        .include(minijail_dir)
        .compile("minijail");

    sources
        .iter()
        .for_each(|s| println!("cargo:rerun-if-changed={}", s));

    println!("cargo:rustc-link-lib=static=minijail");

    Ok(())
}
