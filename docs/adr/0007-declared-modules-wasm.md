# 0007. Modules declare their UI; community modules run in a WASM sandbox

- Status: accepted
- Date: 2026-10-13

## Context

The project grows through community modules. Arbitrary UI code would break the Apple-grade consistency, and arbitrary native code touching user files is unsafe.

## Decision

A module is a module.toml manifest plus code. Built-in modules are native Rust. Community modules compile to WebAssembly and run sandboxed with explicit capabilities (chosen files only, no network by default), calling host engines for heavy work. Every platform renders modules from the manifest with one native component set.

## Consequences

Adding a module is small and safe, and every module looks first-party. Some exotic UIs are not possible inside a module; they become built-ins if needed.
