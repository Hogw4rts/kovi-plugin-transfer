use base64::Engine;
use image::{AnimationDecoder, ImageDecoder, ImageEncoder};
use std::io::Cursor;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::Duration;

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()
        .expect("failed to create HTTP client")
});

fn extract_media_url(data: &serde_json::Value) -> Option<&str> {
    data.get("url")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty() && (s.starts_with("http://") || s.starts_with("https://")))
        .or_else(|| {
            data.get("file")
                .and_then(|v| v.as_str())
                .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
        })
}

pub(crate) async fn process_segments(segments: &mut Vec<serde_json::Value>) {
    for seg in segments.iter_mut() {
        match seg.get("type").and_then(|v| v.as_str()) {
            Some("image") => {
                if let Some(modified) = process_image_segment(seg).await {
                    *seg = modified;
                } else {
                    if let Some(data) = seg.get("data") {
                        if let Some(url) = data
                            .get("url")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                        {
                            kovi::log::info!(
                                "transfer: image mod failed, falling back to CDN URL"
                            );
                            *seg = serde_json::json!({"type": "image", "data": {"file": url}});
                        }
                    }
                }
            }
            Some("video") => {
                if let Some(modified) = process_video_segment(seg).await {
                    *seg = modified;
                } else {
                    if let Some(data) = seg.get("data") {
                        if let Some(url) = data
                            .get("url")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                        {
                            kovi::log::info!(
                                "transfer: video mod failed, falling back to CDN URL"
                            );
                            *seg = serde_json::json!({"type": "video", "data": {"file": url}});
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

async fn process_image_segment(seg: &serde_json::Value) -> Option<serde_json::Value> {
    let data = seg.get("data")?;
    let url = extract_media_url(data)?;

    let resp = HTTP
        .get(url)
        .send()
        .await
        .map_err(|e| {
            kovi::log::warn!("transfer: image download failed: {e}");
        })
        .ok()?;

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| {
            kovi::log::warn!("transfer: image read failed: {e}");
        })
        .ok()?;

    let is_gif = content_type.contains("gif") || bytes.starts_with(b"GIF");

    let modified = if is_gif {
        kovi::log::info!("transfer: processing GIF image, {} bytes", bytes.len());
        process_gif_image(&bytes)?
    } else {
        let img = image::load_from_memory(&bytes)
            .map_err(|e| {
                kovi::log::warn!("transfer: image decode failed: {e}");
            })
            .ok()?;
        modify_edge_pixels(&img)
    };

    let b64 = base64::engine::general_purpose::STANDARD.encode(&modified);
    Some(serde_json::json!({"type": "image", "data": {"file": format!("base64://{}", b64)}}))
}

fn process_gif_image(bytes: &[u8]) -> Option<Vec<u8>> {
    let cursor = Cursor::new(bytes);
    let decoder = image::codecs::gif::GifDecoder::new(cursor).ok()?;
    let (w, h) = decoder.dimensions();

    let mut frames = Vec::new();
    for frame in decoder.into_frames() {
        let frame = frame.ok()?;
        let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_raw(
            w,
            h,
            frame.buffer().to_vec(),
        )?);
        let buf = modify_edge_pixels(&img);
        let modified_img = image::load_from_memory(&buf).ok()?;
        let delay = frame.delay();
        let delay_ms = delay.numer_denom_ms();
        frames.push(image::Frame::from_parts(
            modified_img.to_rgba8(),
            0,
            0,
            image::Delay::from_numer_denom_ms(delay_ms.0, delay_ms.1),
        ));
    }

    let mut buf = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut buf);
        encoder
            .set_repeat(image::codecs::gif::Repeat::Infinite)
            .ok()?;
        encoder.encode_frames(frames).ok()?;
    }
    Some(buf)
}

fn modify_edge_pixels(img: &image::DynamicImage) -> Vec<u8> {
    let mut rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut rng = rand::thread_rng();

    for x in 0..w {
        randomize_pixel(rgba.get_pixel_mut(x, 0), &mut rng);
        if h > 1 {
            randomize_pixel(rgba.get_pixel_mut(x, h - 1), &mut rng);
        }
    }
    for y in 1..h.saturating_sub(1) {
        randomize_pixel(rgba.get_pixel_mut(0, y), &mut rng);
        if w > 1 {
            randomize_pixel(rgba.get_pixel_mut(w - 1, y), &mut rng);
        }
    }

    let mut buf = Vec::new();
    image::codecs::png::PngEncoder::new(&mut buf)
        .write_image(&rgba, w, h, image::ExtendedColorType::Rgba8)
        .unwrap_or_else(|e| {
            kovi::log::warn!("transfer: PNG encode failed: {e}");
        });
    if buf.is_empty() {
        buf.extend_from_slice(&[
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0,
            1, 8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 10, 73, 68, 65, 84, 120, 156, 98, 0, 0,
            0, 2, 0, 1, 228, 33, 188, 51, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ]);
    }
    buf
}

fn randomize_pixel(pixel: &mut image::Rgba<u8>, rng: &mut impl rand::Rng) {
    pixel.0[0] = pixel.0[0].wrapping_add(rng.gen_range(0u8..20u8));
    pixel.0[1] = pixel.0[1].wrapping_add(rng.gen_range(0u8..20u8));
    pixel.0[2] = pixel.0[2].wrapping_add(rng.gen_range(0u8..20u8));
}

async fn process_video_segment(seg: &serde_json::Value) -> Option<serde_json::Value> {
    let data = seg.get("data")?;
    let url = extract_media_url(data)?;

    kovi::log::info!("transfer: downloading video for edge modification");
    let resp = HTTP
        .get(url)
        .send()
        .await
        .map_err(|e| {
            kovi::log::warn!("transfer: video download failed: {e}");
        })
        .ok()?;
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| {
            kovi::log::warn!("transfer: video read failed: {e}");
        })
        .ok()?;

    let modified = modify_video_edges(&bytes)?;
    kovi::log::info!(
        "transfer: video edge modification done, {} bytes -> {} bytes",
        bytes.len(),
        modified.len()
    );

    let b64 = base64::engine::general_purpose::STANDARD.encode(&modified);

    Some(serde_json::json!({"type": "video", "data": {"file": format!("base64://{}", b64)}}))
}

fn modify_video_edges(input: &[u8]) -> Option<Vec<u8>> {
    let filter = "geq=r='if(eq(X,0)+eq(X,W-1)+eq(Y,0)+eq(Y,H-1),clip(r(X,Y)+random(1)*20-10,0,255),r(X,Y))':g='if(eq(X,0)+eq(X,W-1)+eq(Y,0)+eq(Y,H-1),clip(g(X,Y)+random(1)*20-10,0,255),g(X,Y))':b='if(eq(X,0)+eq(X,W-1)+eq(Y,0)+eq(Y,H-1),clip(b(X,Y)+random(1)*20-10,0,255),b(X,Y))'";

    let mut child = Command::new("ffmpeg")
        .args([
            "-i",
            "pipe:0",
            "-vf",
            filter,
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-c:a",
            "copy",
            "-movflags",
            "frag_keyframe+empty_moov",
            "-f",
            "mp4",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input).ok()?;
    }

    let output = child.wait_with_output().ok()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        kovi::log::info!(
            "transfer: ffmpeg failed: {}",
            stderr.lines().last().unwrap_or("")
        );
        return None;
    }

    Some(output.stdout)
}
