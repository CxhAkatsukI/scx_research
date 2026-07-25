fn main() {
    scx_rustland_core::RustLandBuilder::new()
        .unwrap()
        .build()
        .unwrap();
    patch_generated_bpf_wrapper().unwrap();
}

fn patch_generated_bpf_wrapper() -> std::io::Result<()> {
    let path = std::path::Path::new("src/bpf.rs");
    let source = std::fs::read_to_string(path)?;
    let patched = replace_required(
        &source,
        "        skel.struct_ops.rustland_mut().flags =\n            *compat::SCX_OPS_ENQ_LAST | *compat::SCX_OPS_ALLOW_QUEUED_WAKEUP;",
        "        skel.struct_ops.rustland_mut().flags = *compat::SCX_OPS_ENQ_LAST;",
    );
    let patched = replace_required(
        &patched,
        "        // Try to consume the first task from the ring buffer.\n        match self.queued.consume_raw_n(1) {",
        "        // Consume at most one task. If no record is immediately available, wait briefly on\n        // the ring buffer fd and then try a bounded consume again. libbpf's poll helper greedily\n        // consumes all available records, which can overwrite this generated wrapper's single\n        // static callback buffer.\n        let mut consumed = self.queued.consume_raw_n(1);\n        if consumed == 0 {\n            let mut pollfd = libc::pollfd {\n                fd: self.queued.epoll_fd(),\n                events: libc::POLLIN,\n                revents: 0,\n            };\n            let ready = unsafe { libc::poll(&mut pollfd, 1, 1) };\n            if ready < 0 {\n                let errno = std::io::Error::last_os_error()\n                    .raw_os_error()\n                    .unwrap_or(libc::EIO);\n                return Err(-errno);\n            }\n            if ready > 0 {\n                consumed = self.queued.consume_raw_n(1);\n            }\n        }\n\n        match consumed {",
    );
    let patched = replace_required(
        &patched,
        "            0 => {\n                // Ring buffer is empty.\n                bss_data.nr_queued = 0;\n                Ok(None)\n            }",
        "            0 => {\n                // Ring buffer is empty.\n                Ok(None)\n            }",
    );

    if patched != source {
        std::fs::write(path, patched)?;
    }

    Ok(())
}

fn replace_required(source: &str, needle: &str, replacement: &str) -> String {
    let patched = source.replace(needle, replacement);
    assert!(
        patched != source,
        "generated rustland wrapper patch target was not found"
    );
    patched
}
