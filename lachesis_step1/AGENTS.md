# Stage 1 rules

- This is a vendored Lachesis snapshot. See README.md for provenance.
- Keep the original policy readable; explain each change from upstream.
- Generated files belong in build/; downloaded Verus belongs in dep/.
- Keep unsafe code and assumed contracts in src/runtime/trusted/.
  The loader and imported compilation pipeline are also trusted.
- Do not add assume/admit/external_body to make a policy proof pass.
- Run make check and make build after changes to verified Rust code.
- Kernel loading belongs on the user's test machine or in an isolated VM.
- Distinguish Verus success, ELF generation, and actual kernel loading.
- Problem 1, fairness, and progress proofs are outside this stage.
