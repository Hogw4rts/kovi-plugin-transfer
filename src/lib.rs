mod config;

use config::{Source, Target, TransferRule, load_config, save_config};
use kovi::bot::runtimebot::CanSendApi;
use kovi::log::info;
use kovi::PluginBuilder as plugin;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;
use base64::Engine;
use image::{AnimationDecoder, ImageDecoder, ImageEncoder};

pub(crate) static DATA_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

#[kovi::plugin]
async fn main() {
    let bot = plugin::get_runtime_bot();
    let data_path = DATA_PATH.get_or_init(|| bot.get_data_path()).clone();
    let data_path = Arc::new(data_path);

    let config = Arc::new(tokio::sync::RwLock::new(load_config(&data_path)));

    info!("transfer: plugin loaded, {} rules", config.read().await.rules.len());
    info!("transfer: commands: /transfer add|remove|list|enable|disable");

    plugin::on_msg({
        let bot = bot.clone();
        let config = config.clone();
        let data_path = data_path.clone();

        move |event| {
            let bot = bot.clone();
            let config = config.clone();
            let data_path = data_path.clone();

            async move {
                let text = match event.borrow_text() {
                    Some(t) => t.trim().to_string(),
                    None => return,
                };

                let user_id = event.sender.user_id;

                if text.starts_with("/transfer") {
                    match bot.get_all_admin() {
                        Ok(admins) => {
                            if !admins.contains(&user_id) {
                                return;
                            }
                        }
                        Err(_) => return,
                    }

                    let parts: Vec<&str> = text.split_whitespace().collect();
                    if parts.len() < 2 {
                        event.reply("用法:\n\
                            /transfer add <private|group|group_user> <id> [id] -> <private|group> <id>\n\
                            /transfer remove <index>\n\
                            /transfer list\n\
                            /transfer enable <index>\n\
                            /transfer disable <index>");
                        return;
                    }

                    match parts[1] {
                        "add" => {
                            let mut cfg = config.write().await;
                            if let Some(rule) = parse_add_rule(&parts) {
                                cfg.rules.push(rule);
                                save_config(&cfg, &data_path);
                                event.reply(format!("[OK] 已添加规则 #{}, 共 {} 条规则", cfg.rules.len(), cfg.rules.len()));
                            } else {
                                event.reply("格式: /transfer add <private|group|group_user> <id1> [id2] -> <private|group> <id>");
                            }
                        }
                        "remove" | "del" | "delete" => {
                            let mut cfg = config.write().await;
                            if let Some(idx) = parts.get(2).and_then(|s| s.parse::<usize>().ok()) {
                                if idx < 1 || idx > cfg.rules.len() {
                                    event.reply(format!("索引无效: {}（有效范围 1-{}）", idx, cfg.rules.len()));
                                } else {
                                    let removed = cfg.rules.remove(idx - 1);
                                    save_config(&cfg, &data_path);
                                    event.reply(format!("[OK] 已移除规则 #{} ({})", idx, describe_rule(&removed, idx)));
                                }
                            } else {
                                event.reply("用法: /transfer remove <索引>");
                            }
                        }
                        "list" => {
                            let cfg = config.read().await;
                            if cfg.rules.is_empty() {
                                event.reply("暂无转发规则。");
                                return;
                            }
                            let mut lines = vec!["转发规则列表:".to_string()];
                            for (i, rule) in cfg.rules.iter().enumerate() {
                                lines.push(describe_rule(rule, i + 1));
                            }
                            event.reply(lines.join("\n"));
                        }
                        "enable" | "on" => {
                            let mut cfg = config.write().await;
                            if let Some(idx) = parts.get(2).and_then(|s| s.parse::<usize>().ok()) {
                                if idx < 1 || idx > cfg.rules.len() {
                                    event.reply(format!("索引无效: {}（有效范围 1-{}）", idx, cfg.rules.len()));
                                } else {
                                    cfg.rules[idx - 1].enabled = true;
                                    save_config(&cfg, &data_path);
                                    event.reply(format!("[OK] 已启用规则 #{}", idx));
                                }
                            } else {
                                event.reply("用法: /transfer enable <索引>");
                            }
                        }
                        "disable" | "off" => {
                            let mut cfg = config.write().await;
                            if let Some(idx) = parts.get(2).and_then(|s| s.parse::<usize>().ok()) {
                                if idx < 1 || idx > cfg.rules.len() {
                                    event.reply(format!("索引无效: {}（有效范围 1-{}）", idx, cfg.rules.len()));
                                } else {
                                    cfg.rules[idx - 1].enabled = false;
                                    save_config(&cfg, &data_path);
                                    event.reply(format!("[OK] 已禁用规则 #{}", idx));
                                }
                            } else {
                                event.reply("用法: /transfer disable <索引>");
                            }
                        }
                        _ => event.reply("未知子命令。可用: add, remove, list, enable, disable"),
                    }
                    return;
                }

                if text.starts_with('/') {
                    return;
                }

                let self_id = event.self_id;
                if user_id == self_id {
                    return;
                }

                let cfg = config.read().await;
                forward_message(&bot, &event, &cfg.rules, self_id).await;
            }
        }
    });
}

fn parse_add_rule(parts: &[&str]) -> Option<TransferRule> {
    let arrow_pos = parts.iter().position(|&p| p == "->")?;
    if arrow_pos < 3 || arrow_pos + 3 > parts.len() {
        return None;
    }

    let source = parse_source(&parts[2..arrow_pos])?;
    let target = parse_target(&parts[arrow_pos + 1..])?;

    let name = format!(
        "{} {} -> {} {}",
        parts[2], parts[3..arrow_pos].join(" "),
        parts[arrow_pos + 1], parts[arrow_pos + 2..].join(" ")
    );

    Some(TransferRule {
        enabled: true,
        name,
        source,
        target,
    })
}

fn parse_source(parts: &[&str]) -> Option<Source> {
    match parts[0] {
        "private" => {
            let qq = parts.get(1)?.parse().ok()?;
            Some(Source::Private { qq })
        }
        "group" => {
            let group_id = parts.get(1)?.parse().ok()?;
            Some(Source::Group { group_id })
        }
        "group_user" => {
            let group_id = parts.get(1)?.parse().ok()?;
            let qq = parts.get(2)?.parse().ok()?;
            Some(Source::GroupUser { group_id, qq })
        }
        _ => None,
    }
}

fn parse_target(parts: &[&str]) -> Option<Target> {
    match parts[0] {
        "private" => {
            let qq = parts.get(1)?.parse().ok()?;
            Some(Target::Private { qq })
        }
        "group" => {
            let group_id = parts.get(1)?.parse().ok()?;
            Some(Target::Group { group_id })
        }
        _ => None,
    }
}

fn describe_source(source: &Source) -> String {
    match source {
        Source::Private { qq } => format!("私聊 QQ {}", qq),
        Source::Group { group_id } => format!("群 {} 全部消息", group_id),
        Source::GroupUser { group_id, qq } => format!("群 {} 中 QQ {} 的消息", group_id, qq),
    }
}

fn describe_target(target: &Target) -> String {
    match target {
        Target::Private { qq } => format!("私聊 QQ {}", qq),
        Target::Group { group_id } => format!("群 {}", group_id),
    }
}

fn describe_rule(rule: &TransferRule, index: usize) -> String {
    let status = if rule.enabled { "[ON]" } else { "[OFF]" };
    let label = if rule.name.is_empty() {
        String::new()
    } else {
        format!(" ({})", rule.name)
    };
    format!(
        "  #{}. {} {} -> {}{}",
        index,
        status,
        describe_source(&rule.source),
        describe_target(&rule.target),
        label
    )
}

async fn forward_message(
    bot: &kovi::RuntimeBot,
    event: &kovi::MsgEvent,
    rules: &[TransferRule],
    self_id: i64,
) {
    if rules.is_empty() {
        return;
    }

    let msg_type = if event.group_id.is_some() { "group" } else { "private" };
    let user_id = event.sender.user_id;
    let group_id = event.group_id.unwrap_or(0);
    let sender_name = event.sender.nickname.clone().unwrap_or_default();

    for rule in rules {
        if !rule.enabled {
            continue;
        }

        let matched = match &rule.source {
            Source::Private { qq } => *qq == user_id && msg_type == "private",
            Source::Group { group_id: gid } => *gid == group_id && msg_type == "group",
            Source::GroupUser { group_id: gid, qq } => {
                *gid == group_id && *qq == user_id && msg_type == "group"
            }
        };

        if !matched {
            continue;
        }

        if matches!(&rule.target, Target::Private { qq } if *qq == self_id) {
            continue;
        }

        if matches!(&rule.target, Target::Group { group_id: gid } if *gid == group_id) {
            continue;
        }

        if matches!(&rule.target, Target::Private { qq } if *qq == user_id) {
            continue;
        }

        let label = build_forward_label(&rule.source, &sender_name, user_id);

        send_forward(bot, event, &rule.target, &label).await;
    }
}

fn build_forward_label(source: &Source, sender_name: &str, user_id: i64) -> String {
    match source {
        Source::Private { .. } => {
            format!("[转发自私聊 - {}({})]\n", sender_name, user_id)
        }
        Source::Group { group_id } => {
            format!("[转发自群 {} - {}({})]\n", group_id, sender_name, user_id)
        }
        Source::GroupUser { group_id, .. } => {
            format!("[转发自群 {} - {}({})]\n", group_id, sender_name, user_id)
        }
    }
}

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

async fn send_forward(
    bot: &kovi::RuntimeBot,
    event: &kovi::MsgEvent,
    target: &Target,
    label: &str,
) {
    let mut segments: Vec<serde_json::Value> = match serde_json::to_value(&event.message) {
        Ok(serde_json::Value::Array(arr)) => arr,
        _ => return,
    };

    segments.insert(0, serde_json::json!({
        "type": "text",
        "data": { "text": label }
    }));

    for seg in segments.iter_mut() {
        match seg.get("type").and_then(|v| v.as_str()) {
            Some("image") => {
                if let Some(modified) = process_image_segment(seg).await {
                    *seg = modified;
                } else if let Some(data) = seg.get("data") {
                    if let Some(url) = data.get("url").and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        info!("transfer: image mod failed, falling back to CDN URL");
                        *seg = serde_json::json!({
                            "type": "image",
                            "data": { "file": url }
                        });
                    }
                }
            }
            Some("video") => {
                if let Some(modified) = process_video_segment(seg).await {
                    *seg = modified;
                } else if let Some(data) = seg.get("data") {
                    if let Some(url) = data.get("url").and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        info!("transfer: video mod failed, falling back to CDN URL");
                        *seg = serde_json::json!({
                            "type": "video",
                            "data": { "file": url }
                        });
                    }
                }
            }
            _ => {}
        }
    }

    let (action, params) = match target {
        Target::Private { qq } => (
            "send_private_msg",
            serde_json::json!({ "user_id": qq, "message": segments, "auto_escape": false }),
        ),
        Target::Group { group_id } => (
            "send_group_msg",
            serde_json::json!({ "group_id": group_id, "message": segments, "auto_escape": false }),
        ),
    };

    bot.send_api(action, params);

    match target {
        Target::Private { qq } => {
            info!("transfer: forwarded private {} -> private {}", event.sender.user_id, qq);
        }
        Target::Group { group_id } => {
            info!("transfer: forwarded -> group {}", group_id);
        }
    }
}

async fn process_image_segment(seg: &serde_json::Value) -> Option<serde_json::Value> {
    let data = seg.get("data")?;
    let url = data
        .get("url")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty() && (s.starts_with("http://") || s.starts_with("https://")))
        .or_else(|| {
            data.get("file")
                .and_then(|v| v.as_str())
                .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
        })?;

    let resp = HTTP.get(url).send().await.ok()?;
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp.bytes().await.ok()?;

    let is_gif = content_type.contains("gif") || bytes.starts_with(b"GIF");

    let modified = if is_gif {
        info!("transfer: processing GIF image, {} bytes", bytes.len());
        process_gif_image(&bytes)?
    } else {
        let img = image::load_from_memory(&bytes).ok()?;
        let buf = modify_edge_pixels(&img);
        buf
    };

    let b64 = base64::engine::general_purpose::STANDARD.encode(&modified);

    Some(serde_json::json!({
        "type": "image",
        "data": { "file": format!("base64://{}", b64) }
    }))
}

fn process_gif_image(bytes: &[u8]) -> Option<Vec<u8>> {
    use std::io::Cursor;

    let cursor = Cursor::new(bytes);
    let decoder = image::codecs::gif::GifDecoder::new(cursor).ok()?;
    let (w, h) = decoder.dimensions();

    let mut frames = Vec::new();
    for frame in decoder.into_frames() {
        let frame = frame.ok()?;
        let img = image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(w, h, frame.buffer().to_vec())?,
        );
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
        .unwrap();
    buf
}

fn randomize_pixel(pixel: &mut image::Rgba<u8>, rng: &mut impl rand::Rng) {
    pixel.0[0] = pixel.0[0].wrapping_add(rng.gen_range(0u8..20u8));
    pixel.0[1] = pixel.0[1].wrapping_add(rng.gen_range(0u8..20u8));
    pixel.0[2] = pixel.0[2].wrapping_add(rng.gen_range(0u8..20u8));
}

async fn process_video_segment(seg: &serde_json::Value) -> Option<serde_json::Value> {
    let data = seg.get("data")?;
    let url = data
        .get("url")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty() && (s.starts_with("http://") || s.starts_with("https://")))
        .or_else(|| {
            data.get("file")
                .and_then(|v| v.as_str())
                .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
        })?;

    info!("transfer: downloading video for edge modification");
    let resp = HTTP.get(url).send().await.ok()?;
    let bytes = resp.bytes().await.ok()?;

    let modified = modify_video_edges(&bytes)?;
    info!("transfer: video edge modification done, {} bytes -> {} bytes", bytes.len(), modified.len());

    let b64 = base64::engine::general_purpose::STANDARD.encode(&modified);

    Some(serde_json::json!({
        "type": "video",
        "data": { "file": format!("base64://{}", b64) }
    }))
}

fn modify_video_edges(input: &[u8]) -> Option<Vec<u8>> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let filter = "geq=r='if(eq(X,0)+eq(X,W-1)+eq(Y,0)+eq(Y,H-1),clip(r(X,Y)+random(1)*20-10,0,255),r(X,Y))':g='if(eq(X,0)+eq(X,W-1)+eq(Y,0)+eq(Y,H-1),clip(g(X,Y)+random(1)*20-10,0,255),g(X,Y))':b='if(eq(X,0)+eq(X,W-1)+eq(Y,0)+eq(Y,H-1),clip(b(X,Y)+random(1)*20-10,0,255),b(X,Y))'";

    let mut child = Command::new("ffmpeg")
        .args([
            "-i", "pipe:0",
            "-vf", filter,
            "-c:v", "libx264",
            "-preset", "ultrafast",
            "-c:a", "copy",
            "-movflags", "frag_keyframe+empty_moov",
            "-f", "mp4",
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
        info!("transfer: ffmpeg failed: {}", stderr.lines().last().unwrap_or(""));
        return None;
    }

    Some(output.stdout)
}

