//! The image AI commands: `panoply cutout`, `erase`, `unmark` and `upscale`.
//!
//! They are thin: each one builds the options of its module (`image.remove_background`,
//! `image.erase`, `image.remove_watermark`, `image.upscale`) and runs it through the same
//! registry and capability guard as `panoply run`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::Subcommand;
use panoply_core::{CancelToken, JobEvent, JobSpec};
use serde_json::{Map, Value, json};

/// The image AI commands.
#[derive(Subcommand)]
pub enum VisionCmd {
    /// Cut the subject out: panoply cutout a.jpg [--color white | --blur 40]
    Cutout {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Put the subject on this color instead of transparency, for example white or #f5f0e8
        #[arg(long)]
        color: Option<String>,
        /// Put the subject on a blurred copy of the picture, strength 1 to 100
        #[arg(long)]
        blur: Option<u32>,
        /// auto (default), os (the system's subject finder) or model (IS-Net)
        #[arg(long)]
        engine: Option<String>,
        /// Write results into this folder instead of next to each original
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
    },
    /// Erase an object: panoply erase a.jpg mask.png, or panoply erase a.jpg --rect 100,200,300,80
    Erase {
        /// The picture, then its mask picture (white is erased) unless --rect is given
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Erase this rectangle (x,y,w,h in pixels or percentages) instead of using a mask picture
        #[arg(long)]
        rect: Option<String>,
        /// Grow the mask by this many pixels (default 8)
        #[arg(long)]
        expand: Option<u32>,
        /// Write results into this folder instead of next to each original
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
    },
    /// Remove a watermark, logo or date stamp from your own pictures:
    /// panoply unmark ./photos --auto --yes-i-own-these
    Unmark {
        /// Pictures, or folders of pictures
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Find the marks yourself (the default; kept so scripts read clearly)
        #[arg(long)]
        auto: bool,
        /// Confirm that these pictures are yours or that you may edit them (required)
        #[arg(long = "yes-i-own-these")]
        yes_i_own_these: bool,
        /// Clean this rectangle (x,y,w,h in pixels or percentages) instead of searching
        #[arg(long)]
        rect: Option<String>,
        /// Look only within 18% of an edge
        #[arg(long)]
        edges: bool,
        /// Also write the cleaned area as a picture
        #[arg(long)]
        save_mask: bool,
        /// Write results into this folder instead of next to each original
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
    },
    /// Make a picture 2 or 4 times bigger: panoply upscale a.jpg --scale 4
    Upscale {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// 2 or 4 (default: 4 under 1000 px, else 2)
        #[arg(long)]
        scale: Option<u32>,
        /// Write results into this folder instead of next to each original
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
    },
}

/// Folders become the pictures inside them (not looking deeper), sorted by name.
fn expand(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            let mut inside: Vec<PathBuf> = std::fs::read_dir(&p)
                .map(|rd| {
                    rd.flatten()
                        .map(|e| e.path())
                        .filter(|f| {
                            f.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                                matches!(
                                    e.to_ascii_lowercase().as_str(),
                                    "jpg"
                                        | "jpeg"
                                        | "png"
                                        | "webp"
                                        | "heic"
                                        | "heif"
                                        | "tif"
                                        | "tiff"
                                        | "avif"
                                        | "bmp"
                                )
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            inside.sort();
            out.extend(inside);
        } else {
            out.push(p);
        }
    }
    out
}

fn run_job(
    id: &str,
    files: Vec<PathBuf>,
    options: Map<String, Value>,
    out: Option<PathBuf>,
) -> ExitCode {
    let spec = JobSpec {
        module_id: id.to_string(),
        inputs: files,
        options: Value::Object(options),
    };
    let started = Instant::now();
    let slow = Duration::from_millis(panoply_core::budgets::PROGRESS_AFTER_MS);
    let result = crate::run_builtin(&spec, out.as_deref(), CancelToken::new(), move |event| {
        if let JobEvent::Progress { fraction, label } = event
            && started.elapsed() > slow
        {
            eprintln!("{:>3.0}%  {label}", fraction * 100.0);
        }
    });
    match result {
        Ok(o) => {
            for line in o.notes {
                println!("{line}");
            }
            for path in o.outputs {
                println!("wrote {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{}", e.plain_message());
            eprintln!("{}", e.next_step());
            ExitCode::FAILURE
        }
    }
}

/// Run one of the commands.
pub fn run(cmd: VisionCmd) -> ExitCode {
    let mut o = Map::new();
    match cmd {
        VisionCmd::Cutout {
            files,
            color,
            blur,
            engine,
            out,
        } => {
            if let Some(c) = color {
                o.insert("result".into(), json!("color"));
                o.insert("background".into(), json!(c));
            } else if let Some(b) = blur {
                o.insert("result".into(), json!("blur"));
                o.insert("blur".into(), json!(b));
            }
            if let Some(e) = engine {
                o.insert("engine".into(), json!(e));
            }
            run_job("image.remove_background", expand(files), o, out)
        }
        VisionCmd::Erase {
            files,
            rect,
            expand: grow,
            out,
        } => {
            if let Some(r) = rect {
                o.insert("rect".into(), json!(r));
            }
            if let Some(g) = grow {
                o.insert("expand".into(), json!(g));
            }
            run_job("image.erase", files, o, out)
        }
        VisionCmd::Unmark {
            files,
            auto: _,
            yes_i_own_these,
            rect,
            edges,
            save_mask,
            out,
        } => {
            o.insert("own_content".into(), json!(yes_i_own_these));
            if let Some(r) = rect {
                o.insert("rect".into(), json!(r));
            }
            if edges {
                o.insert("search".into(), json!("edges"));
            }
            if save_mask {
                o.insert("save_mask".into(), json!(true));
            }
            run_job("image.remove_watermark", expand(files), o, out)
        }
        VisionCmd::Upscale { files, scale, out } => {
            if let Some(s) = scale {
                o.insert("scale".into(), json!(s.to_string()));
            }
            run_job("image.upscale", expand(files), o, out)
        }
    }
}
