//! Probe: what ImageIO costs for a camera RAW under different requests. Run each variant in its
//! own process under `/usr/bin/time -l` and read "peak memory footprint".
//!
//! Usage: rawprobe FILE VARIANT [N]
//!   full        CGImageSourceCreateImageAtIndex, draw all 8 bit
//!   plain       same, without the SDR decode request
//!   thumb N     CreateThumbnailFromImageAlways with MaxPixelSize N, draw all
//!   thumbif N   CreateThumbnailFromImageIfAbsent (embedded thumbnail when there is one)
//!   sub N       kCGImageSourceSubsampleFactor N on the image request
//!   embedded    list the embedded preview sizes without decoding the RAW
#[cfg(target_os = "macos")]
fn main() {
    use objc2_core_foundation::{CFBoolean, CFData, CFNumber, CFString, CFType};
    use objc2_core_graphics::{CGColorSpace, CGImage, kCGColorSpaceSRGB};
    use objc2_image_io::{
        CGImageSource, kCGImageSourceCreateThumbnailFromImageAlways,
        kCGImageSourceCreateThumbnailFromImageIfAbsent, kCGImageSourceCreateThumbnailWithTransform,
        kCGImageSourceDecodeRequest, kCGImageSourceDecodeToSDR, kCGImageSourceShouldCache,
        kCGImageSourceShouldCacheImmediately, kCGImageSourceSubsampleFactor,
        kCGImageSourceThumbnailMaxPixelSize,
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = &args[0];
    let variant = args[1].as_str();
    let n: i64 = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(0);
    let data = std::fs::read(path).unwrap();
    let cfdata = CFData::from_bytes(&data);
    drop(data);
    let t0 = std::time::Instant::now();
    let f = CFBoolean::new(false);
    let tr = CFBoolean::new(true);
    let num = CFNumber::new_i64(n);
    // SAFETY: these are immutable global CFString constants.
    let (k_cache, k_now, k_req, v_sdr, k_always, k_ifabs, k_max, k_tr, k_sub) = unsafe {
        (
            kCGImageSourceShouldCache,
            kCGImageSourceShouldCacheImmediately,
            kCGImageSourceDecodeRequest,
            kCGImageSourceDecodeToSDR,
            kCGImageSourceCreateThumbnailFromImageAlways,
            kCGImageSourceCreateThumbnailFromImageIfAbsent,
            kCGImageSourceThumbnailMaxPixelSize,
            kCGImageSourceCreateThumbnailWithTransform,
            kCGImageSourceSubsampleFactor,
        )
    };
    let mk = |entries: &[(&CFString, &CFType)]| {
        let keys: Vec<&CFString> = entries.iter().map(|e| e.0).collect();
        let values: Vec<&CFType> = entries.iter().map(|e| e.1).collect();
        objc2_core_foundation::CFDictionary::<CFString, CFType>::from_slices(&keys, &values)
    };
    let base: Vec<(&CFString, &CFType)> = vec![(k_cache, f.as_ref()), (k_now, f.as_ref())];
    let src_opts = mk(&base);
    // SAFETY: live CFData and dictionary.
    let src = unsafe { CGImageSource::with_data(&cfdata, Some(src_opts.as_ref())) }.unwrap();
    let img: Option<objc2_core_foundation::CFRetained<CGImage>> = match variant {
        "full" => {
            let mut e = base.clone();
            e.push((k_req, v_sdr.as_ref()));
            let o = mk(&e);
            unsafe { src.image_at_index(0, Some(o.as_ref())) }
        }
        "plain" => {
            let o = mk(&base);
            unsafe { src.image_at_index(0, Some(o.as_ref())) }
        }
        "thumb" | "thumbif" => {
            let mut e = base.clone();
            e.push((
                if variant == "thumb" {
                    k_always
                } else {
                    k_ifabs
                },
                tr.as_ref(),
            ));
            e.push((k_tr, tr.as_ref()));
            e.push((k_max, num.as_ref()));
            let o = mk(&e);
            unsafe { src.thumbnail_at_index(0, Some(o.as_ref())) }
        }
        "sub" => {
            let mut e = base.clone();
            e.push((k_sub, num.as_ref()));
            let o = mk(&e);
            unsafe { src.image_at_index(0, Some(o.as_ref())) }
        }
        _ => panic!("variant"),
    };
    let Some(img) = img else {
        println!("no image");
        return;
    };
    let (w, h) = (CGImage::width(Some(&img)), CGImage::height(Some(&img)));
    println!(
        "{variant} {n}: image {w}x{h} bpc {} created in {:.0} ms",
        CGImage::bits_per_component(Some(&img)),
        t0.elapsed().as_secs_f64() * 1000.0
    );
    // Draw it in bands of 128 rows into an 8 bit sRGB context, the way the decoder does.
    let cs = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).unwrap();
    let band = 128usize;
    let mut buf = vec![0u8; w * band * 4];
    let mut y = 0;
    let mut sum = 0u64;
    while y < h {
        let rows = band.min(h - y);
        let slice = &mut buf[..rows * w * 4];
        slice.fill(0);
        unsafe {
            let ctx = objc2_core_graphics::CGBitmapContextCreate(
                slice.as_mut_ptr().cast(),
                w,
                rows,
                8,
                w * 4,
                Some(&cs),
                objc2_core_graphics::CGImageAlphaInfo::NoneSkipLast.0,
            )
            .unwrap();
            let rect = objc2_core_foundation::CGRect {
                origin: objc2_core_foundation::CGPoint {
                    x: 0.0,
                    y: -((h - y - rows) as f64),
                },
                size: objc2_core_foundation::CGSize {
                    width: w as f64,
                    height: h as f64,
                },
            };
            objc2_core_graphics::CGContext::draw_image(Some(&ctx), rect, Some(&img));
        }
        sum += slice.iter().step_by(997).map(|&v| v as u64).sum::<u64>();
        if let Some(d) = std::env::var_os("DUMP") {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(d)
                .unwrap();
            f.write_all(slice).unwrap();
        }
        y += rows;
    }
    println!(
        "drawn in {:.0} ms total, checksum {sum}",
        t0.elapsed().as_secs_f64() * 1000.0
    );
    if let Some(s) = std::env::var("HOLD")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        std::thread::sleep(std::time::Duration::from_secs(s));
    }
}
#[cfg(not(target_os = "macos"))]
fn main() {}
