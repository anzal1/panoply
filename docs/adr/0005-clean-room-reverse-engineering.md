# 0005. Clean-room reverse engineering with REA

- Status: accepted
- Date: 2026-10-13

## Context

Some quality bars are set by closed tools that are hard to match (Acrobat text editing, Everything search speed, remove.bg edges, Wispr Flow latency).

## Decision

Before lowering a bar, study how the benchmark tool achieves it with REA (github.com/morluto/rea): behavior first, binaries only when needed, findings recorded with evidence. Implementation is clean-room: whoever writes the code works from the technique note, never from decompiled code.

## Consequences

We learn techniques without copying code, which keeps Panoply original and safe to publish. Technique notes live in docs/research/.
