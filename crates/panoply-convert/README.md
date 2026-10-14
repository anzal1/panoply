# panoply-convert

The conversion graph and the shortest-route planner. It answers "can A become B, and how", for every format pair Panoply knows.

`data/graph.json` is generated from `conversions/formats.py` and embedded at compile time. This crate never
regenerates it. To add a format or a conversion, edit `formats.py`, run `python3 conversions/formats.py` and
commit the regenerated `docs/CONVERSIONS.md` and `graph.json`. CI fails if they drift.

Routing rules are in the crate docs (`cargo doc -p panoply-convert --open`).

Part of [Panoply](../../README.md). Licensed under MIT OR Apache-2.0.
