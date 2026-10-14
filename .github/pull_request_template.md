## What this changes

<!-- One or two plain sentences. Link the issue it closes, for example "Closes #12". -->

## Why

<!-- The user problem, or the number that moved. -->

## How it was checked

<!-- Commands you ran, files you tried, benchmark result for any module change. -->

## Checklist

- [ ] `cargo fmt --all --check` passes
- [ ] `cargo clippy --workspace --exclude gpui-base --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace --exclude gpui-base` passes
- [ ] Meets the budgets in docs/FLOWS.md (or the spec states the exception and why)
- [ ] Quality bar result included for any module change
- [ ] OS engine first; any model is the smallest clean-licensed option, downloaded on first use
- [ ] No GPL, LGPL-linked or AGPL dependencies, no copied or decompiled code (clean room)
- [ ] `cargo deny check` passes if dependencies changed, and `THIRD-PARTY-NOTICES.md` is regenerated
- [ ] `docs/CONVERSIONS.md` and `graph.json` regenerated if formats changed
