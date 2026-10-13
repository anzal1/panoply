//! The conversion graph and shortest-route planner.
//!
//! `data/graph.json` is generated from `conversions/formats.py` and embedded at compile
//! time. This crate never regenerates it.
//!
//! # Routing rules
//!
//! These mirror `count_pairs` in `conversions/formats.py`, so the Rust planner and the
//! published count of conversions agree.
//!
//! 1. Plain steps (`edges`) chain, up to [`MAX_HOPS`] steps, shortest route first.
//! 2. A plain route is allowed only when the source and target families are allowed:
//!    the same family, a family pair listed in `composite_families`, or a family pair joined
//!    by a direct plain bridge. This stops silly chains such as 3D to image to video to audio.
//!    Only the end points are gated. The steps in between can pass through any family.
//! 3. Model steps (`model_edges`, for example transcribe or OCR) are used only as a single
//!    direct step and are never chained, and a plain route wins when both exist.
//!
//! # Ambiguous extensions
//!
//! An extension can name more than one format (the generator allows it, for example `ts`
//! could be MPEG-TS video or TypeScript code). [`Graph::format_by_ext`] picks by
//! [`FAMILY_PRIORITY`], and [`Graph::route`] tries every candidate pair and keeps the shortest
//! route, with ties going to the higher-priority families. At the moment every extension in
//! the graph is unique, so this is a guard for future formats rather than a daily case.
//!
//! # Tie breaks inside a route
//!
//! Several routes can have the same length (a Word file reaches JPG through PDF or through
//! HTML). The planner prefers common hub formats ([`HUBS`]) as the middle step, so Word to JPG
//! goes through PDF.

mod graph;

pub use graph::{FAMILY_PRIORITY, Format, Graph, GraphError, HUBS, MAX_HOPS, Route, Step};
