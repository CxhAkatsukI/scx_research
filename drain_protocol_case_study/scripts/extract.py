#!/usr/bin/env python3
"""Generate host oracles from pinned C, not a second handwritten protocol."""
import json
import re
from fetch import ROOT, blob_id


def block_end(text, opening):
    # Braces inside comments/literals do not change the C block nesting.
    tokens = r'/\*[\s\S]*?\*/|//[^\n]*|"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'|[{}]'
    depth = 0
    for token in re.finditer(tokens, text[opening:]):
        if token[0] == "{":
            depth += 1
        elif token[0] == "}":
            depth -= 1
            if depth == 0:
                return opening + token.end()
    raise ValueError("unterminated C block")


def function(text, name):
    matches = list(re.finditer(r"^static inline [^\n]*\b" + re.escape(name) + r"\(", text, re.M))
    if len(matches) != 1:
        raise ValueError(f"ambiguous function: {name}")
    start = matches[0].start()
    end = block_end(text, text.index("{", start))
    return text[start:end]


def main():
    manifest = json.loads((ROOT / "upstream.json").read_text())
    output = ROOT / "build"
    output.mkdir(exist_ok=True)
    for version in ("before", "after"):
        source = ROOT / "dep/evidence" / version / "llc_aware.bpf.h"
        raw = source.read_bytes()
        expected = next(f["blob"] for f in manifest["files"] if f["version"] == version
                        and f["path"].endswith("llc_aware.bpf.h"))
        if blob_id(raw) != expected:
            raise SystemExit(f"untrusted source contents: {source}")
        text = raw.decode()
        names = ("subcell_llc_drain_enable", "subcell_llc_drain_disable",
                 "subcell_llc_nr_queued_inc", "subcell_llc_has_cpus", "account_subcell_llc_enqueue")
        parts = [f'// Generated from {manifest[version]}, blob {expected}.\n',
                 f'#define CASE {version}\n#include "oracle.h"\n']
        parts.extend(function(text, name) + "\n" for name in names)
        needle = "pending = READ_ONCE(candidate_llc_state->nr_queued);"
        start = text.index(needle)
        condition = text.index("if (!pending)", start)
        end = block_end(text, text.index("{", condition))
        branch = text[start:end]
        # Keep the upstream 'continue' unchanged: it exits this one-iteration loop.
        parts.append("static inline bool source_empty_queue_branch(void) {\n"
                     "struct subcell *subcell = &shared;\n"
                     "struct subcell_llc *candidate_llc_state = &shared.llcs[0];\n"
                     "u32 candidate_llc = 0, cell_id = 0, subcell_id = 0, pending;\n"
                     "(void)cell_id; (void)subcell_id;\n"
                     "for (int once = 0; once < 1; once++) {\n" + branch +
                     "\nreturn false;\n}\nreturn true;\n}\n")
        (output / f"{version}.c").write_text("\n".join(parts))
        print(f"extracted {version}: {len(names)} whole functions + unchanged empty branch")


if __name__ == "__main__":
    main()
