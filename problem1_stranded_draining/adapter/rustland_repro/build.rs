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
    let patched = source.replace(
        "        skel.struct_ops.rustland_mut().flags =\n            *compat::SCX_OPS_ENQ_LAST | *compat::SCX_OPS_ALLOW_QUEUED_WAKEUP;",
        "        skel.struct_ops.rustland_mut().flags = *compat::SCX_OPS_ENQ_LAST;",
    );

    if patched != source {
        std::fs::write(path, patched)?;
    }

    Ok(())
}
