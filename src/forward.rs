use kovi::bot::runtimebot::CanSendApi;
use kovi::log::info;
use crate::config::{Source, Target, TransferRule};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MsgType {
    Private,
    Group,
}

pub(crate) async fn forward_message(
    bot: &kovi::RuntimeBot,
    event: &kovi::MsgEvent,
    rules: &[TransferRule],
    self_id: i64,
) {
    if rules.is_empty() {
        return;
    }

    let msg_type = if event.group_id.is_some() { MsgType::Group } else { MsgType::Private };
    let user_id = event.sender.user_id;
    let group_id = event.group_id.unwrap_or(0);
    let sender_name = event.sender.nickname.clone().unwrap_or_default();

    for rule in rules {
        if !rule.enabled {
            continue;
        }

        let matched = match &rule.source {
            Source::Private { qq } => *qq == user_id && msg_type == MsgType::Private,
            Source::Group { group_id: gid } => *gid == group_id && msg_type == MsgType::Group,
            Source::GroupUser { group_id: gid, qq } => {
                *gid == group_id && *qq == user_id && msg_type == MsgType::Group
            }
        };

        if !matched {
            continue;
        }

        // Anti-loop checks
        if matches!(&rule.target, Target::Private { qq } if *qq == self_id) { continue; }
        if matches!(&rule.target, Target::Group { group_id: gid } if *gid == group_id) { continue; }
        if matches!(&rule.target, Target::Private { qq } if *qq == user_id) { continue; }

        let label = crate::cmd::build_forward_label(&rule.source, &sender_name, user_id);

        send_forward(bot, event, &rule.target, &label).await;
    }
}

async fn send_forward(
    bot: &kovi::RuntimeBot,
    event: &kovi::MsgEvent,
    target: &Target,
    label: &str,
) {
    let mut segments: Vec<serde_json::Value> = match serde_json::to_value(&event.message) {
        Ok(serde_json::Value::Array(arr)) => arr,
        _ => {
            kovi::log::warn!("transfer: failed to serialize message segments");
            return;
        }
    };

    segments.insert(0, serde_json::json!({
        "type": "text",
        "data": { "text": label }
    }));

    crate::media::process_segments(&mut segments).await;

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
