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
        "        // Wait briefly for a task from the ring buffer. A pure busy consume loop can miss\n        // the wakeup timing we are trying to exercise in this repro.\n        match self.queued.poll_raw(std::time::Duration::from_millis(1)) {",
    );
    let patched = replace_required(
        &patched,
        "            1 => {\n                // A valid task is received, convert data to a proper task struct.\n                let task = unsafe { EnqueuedMessage::from_bytes(&BUF.0).to_queued_task() };\n                bss_data.nr_queued = bss_data.nr_queued.saturating_sub(1);\n\n                Ok(Some(task))\n            }\n            res if res < 0 => Err(res),\n            res => panic!(\"Unexpected return value from libbpf-rs::consume_raw(): {res}\"),",
        "            res if res > 0 => {\n                // A valid task is received, convert data to a proper task struct.\n                // The callback keeps the last consumed sample; this repro drives one workload\n                // task, so consuming more than one record is still enough to advance the trace.\n                let task = unsafe { EnqueuedMessage::from_bytes(&BUF.0).to_queued_task() };\n                bss_data.nr_queued = bss_data.nr_queued.saturating_sub(res as u64);\n\n                Ok(Some(task))\n            }\n            res if res < 0 => Err(res),\n            res => panic!(\"Unexpected return value from libbpf-rs::poll_raw(): {res}\"),",
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
