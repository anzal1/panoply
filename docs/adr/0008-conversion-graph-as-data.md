# 0008. The conversion graph is generated data

- Status: accepted
- Date: 2026-10-13

## Context

9,214 conversions across 214 formats cannot be maintained by hand, and the claimed number must be honest.

## Decision

conversions/formats.py is the single source of truth. It generates docs/CONVERSIONS.md and crates/panoply-convert/data/graph.json. The router does shortest-path search with at most four plain steps, model steps only as a single direct step, and only sensible family pairs count.

## Consequences

Adding a format or bridge is one line plus regeneration, and every pair gets a round-trip test in CI.
