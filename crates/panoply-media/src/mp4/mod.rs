//! Own MP4 code: the box layer, the movie index and a writer that rebuilds an index.
//!
//! Used where the helper engine cannot do the job: the report on where the index sits (Info),
//! lossless cuts that re-encode only the edge frames and splice them into the copied ones
//! (Trim), and rebuilding the index of a file whose end is missing (Repair).

pub mod boxes;
pub mod cut;
pub mod movie;
pub mod poc;
pub mod repair;
pub mod source;
pub mod write;

use movie::Movie;
use write::{OutSample, OutTrack};

impl Movie {
    /// Every track as it is, reading its samples from source `src`: rewriting a file with the
    /// index at the front (faststart) is `write` of these tracks.
    pub fn out_tracks(&self, src: usize) -> Vec<OutTrack> {
        self.tracks
            .iter()
            .map(|t| OutTrack {
                id: t.id,
                handler: t.handler,
                timescale: t.timescale,
                tkhd: t.tkhd.clone(),
                mdhd: t.mdhd.clone(),
                hdlr: t.hdlr.clone(),
                minf_head: t.minf_head.clone(),
                mdia_extra: t.mdia_extra.clone(),
                trak_extra: t.trak_extra.clone(),
                stsd: t.stsd.clone(),
                samples: t
                    .samples
                    .iter()
                    .map(|s| OutSample::from_source(src, s))
                    .collect(),
                edits: t.edits.clone(),
            })
            .collect()
    }
}
