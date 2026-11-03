//! Talk to the companion from your phone: a Telegram bot (your own, made with @BotFather) relays text messages to the
//! same brain, memory and tools, and sends the answer back. Only paired chats are served: Settings shows a one-time
//! code, and sending `/pair CODE` to the bot adds that chat. Approval cards also appear in the chat as Allow / No
//! buttons, so remote requests can still be confirmed.
//!
//! Long polling (`getUpdates`), so nothing has to be reachable from the internet.

use serde_json::{json, Value};
use std::{
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};

use crate::{brain, log, settings::TelegramChat, Core, LockExt};

const API: &str = "https://api.telegram.org";
const PAIR_FOR: Duration = Duration::from_secs(600);
const LIMIT: usize = 4000; // Telegram allows 4096 characters per message

/// A new six-digit pairing code, valid for ten minutes.
pub fn new_pair_code(core: &Core) -> String {
    use rand::Rng;
    let code = format!("{:06}", rand::rng().random_range(0..1_000_000));
    *core.pair_code.lk() = Some((code.clone(), Instant::now()));
    code
}

async fn call(core: &Core, token: &str, method: &str, body: Value, wait: Duration) -> Result<Value, String> {
    let r = core
        .http
        .post(format!("{API}/bot{token}/{method}"))
        .timeout(wait)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?; // the URL holds the token: keep it out of messages and logs
    let v: Value = r.json().await.map_err(|e| e.without_url().to_string())?;
    if v["ok"].as_bool() == Some(true) {
        Ok(v["result"].clone())
    } else {
        Err(v["description"].as_str().unwrap_or("Telegram refused the request").to_string())
    }
}

/// Split a long reply into Telegram-sized pieces, preferring paragraph and line breaks.
fn chunks(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut rest = text.trim();
    while rest.chars().count() > LIMIT {
        let cut = rest.char_indices().nth(LIMIT).map_or(rest.len(), |(i, _)| i);
        let head = &rest[..cut];
        let at = head.rfind("\n\n").or_else(|| head.rfind('\n')).or_else(|| head.rfind(". ").map(|i| i + 1)).filter(|&i| i > LIMIT / 2).unwrap_or(cut);
        out.push(rest[..at].trim().to_string());
        rest = rest[at..].trim_start();
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}

async fn send(core: &Core, token: &str, chat: i64, text: &str) {
    for part in chunks(text) {
        if let Err(e) = call(core, token, "sendMessage", json!({"chat_id": chat, "text": part}), Duration::from_secs(20)).await {
            log::warn(&format!("telegram: couldn't send a message: {e}"));
        }
    }
}

fn settings(core: &Core) -> (bool, String, Vec<TelegramChat>) {
    let s = core.settings.lk();
    (s.telegram_enabled, s.telegram_token.trim().to_string(), s.telegram_chats.clone())
}

/// The bot loop: starts, stops and follows token changes by itself.
pub fn spawn(core: Arc<Core>) {
    tauri::async_runtime::spawn(async move {
        let mut offset: i64 = 0;
        let mut last_token = String::new();
        let mut failures = 0u32;
        loop {
            let (on, token, _) = settings(&core);
            if !on || token.is_empty() {
                tokio::select! { _ = core.remote_restart.notified() => {}, _ = tokio::time::sleep(Duration::from_secs(5)) => {} }
                continue;
            }
            if token != last_token {
                offset = 0;
                last_token = token.clone();
                match call(&core, &token, "getMe", json!({}), Duration::from_secs(15)).await {
                    Ok(me) => {
                        let name = me["username"].as_str().unwrap_or("your bot").to_string();
                        log::info(&format!("telegram: connected as @{name}"));
                        *core.telegram_bot.lk() = Some(name);
                        core.emit("remote", json!(null));
                    }
                    Err(e) => {
                        log::warn(&format!("telegram: {e}"));
                        *core.telegram_bot.lk() = None;
                        core.status(format!("Telegram didn't accept the bot token ({e}). Check it in Settings -> Remote."));
                        core.emit("remote", json!(null));
                        last_token.clear();
                        tokio::select! { _ = core.remote_restart.notified() => {}, _ = tokio::time::sleep(Duration::from_secs(60)) => {} }
                        continue;
                    }
                }
            }
            let body = json!({"offset": offset, "timeout": 25, "allowed_updates": ["message", "callback_query"]});
            let updates = tokio::select! {
                r = call(&core, &token, "getUpdates", body, Duration::from_secs(40)) => r,
                _ = core.remote_restart.notified() => continue,
            };
            match updates {
                Ok(list) => {
                    failures = 0;
                    for u in list.as_array().into_iter().flatten() {
                        offset = offset.max(u["update_id"].as_i64().unwrap_or(0) + 1);
                        handle(&core, &token, u).await;
                    }
                }
                Err(e) => {
                    failures += 1;
                    log::warn(&format!("telegram: polling failed: {e}"));
                    tokio::time::sleep(Duration::from_secs((2u64 << failures.min(5)).min(60))).await;
                }
            }
        }
    });
}

async fn handle(core: &Arc<Core>, token: &str, u: &Value) {
    if let Some(q) = u.get("callback_query") {
        return answer_button(core, token, q).await;
    }
    let Some(m) = u.get("message") else { return };
    let chat = m["chat"]["id"].as_i64().unwrap_or(0);
    let who = m["from"]["first_name"].as_str().or(m["chat"]["title"].as_str()).unwrap_or("someone").to_string();
    let text = m["text"].as_str().unwrap_or("").trim().to_string();
    let (_, _, chats) = settings(core);
    let paired = chats.iter().any(|c| c.id == chat);

    if let Some(code) = text.strip_prefix("/pair") {
        let ok = {
            let mut pc = core.pair_code.lk();
            let valid = pc.as_ref().is_some_and(|(c, at)| c == code.trim() && at.elapsed() < PAIR_FOR);
            if valid {
                *pc = None; // one use only
            }
            valid
        };
        if !ok {
            return send(core, token, chat, "That code isn't right or has expired. Make a new one in Settings -> Remote on the computer.").await;
        }
        let saved = {
            let mut s = core.settings.lk().clone();
            if !s.telegram_chats.iter().any(|c| c.id == chat) {
                s.telegram_chats.push(TelegramChat { id: chat, name: who.clone() });
            }
            core.save_settings_now(s)
        };
        return match saved {
            Ok(()) => {
                core.emit("remote", json!(null));
                core.status(format!("{who} can now talk to me on Telegram."));
                send(core, token, chat, &format!("Paired! Hi {who}, it's {}. Message me any time.", core.settings.lk().name)).await
            }
            Err(e) => send(core, token, chat, &format!("I couldn't save that ({e}).")).await,
        };
    }
    if !paired {
        return send(core, token, chat, "Hi! I'm a private desktop companion. To talk to me, open Settings -> Remote on the computer, make a pairing code and send /pair CODE here.").await;
    }
    if text == "/start" || text == "/help" {
        return send(core, token, chat, "Just write to me like a chat. I can use my tools for you; anything risky shows Allow / No buttons here. /stop stops whatever I'm doing.").await;
    }
    if text == "/stop" {
        core.stop_everything();
        return send(core, token, chat, "Stopped.").await;
    }
    if text.is_empty() {
        return send(core, token, chat, "I can only read text messages here for now.").await;
    }
    let text: String = text.chars().take(2000).collect();
    let (core, token) = (core.clone(), token.to_string());
    // Answer on its own task so button presses (approvals) keep being read while I think.
    tokio::spawn(async move {
        let typing = {
            let (c, t) = (core.clone(), token.clone());
            tokio::spawn(async move {
                loop {
                    let _ = call(&c, &t, "sendChatAction", json!({"chat_id": chat, "action": "typing"}), Duration::from_secs(10)).await;
                    tokio::time::sleep(Duration::from_secs(4)).await;
                }
            })
        };
        let reply = brain::respond_remote(core.clone(), text, brain::Origin::Telegram(chat)).await;
        typing.abort();
        let reply = reply.unwrap_or_else(|e| e);
        send(&core, &token, chat, if reply.trim().is_empty() { "(done)" } else { &reply }).await;
    });
}

/// Show an approval card in the chat that is talking to me right now.
pub fn offer_approval(core: &Arc<Core>, what: &str) {
    let Some(brain::Origin::Telegram(chat)) = *core.remote.lk() else { return };
    let (on, token, _) = settings(core);
    if !on || token.is_empty() {
        return;
    }
    let (c, what) = (core.clone(), what.to_string());
    tokio::spawn(async move {
        let body = json!({"chat_id": chat, "text": format!("May I? {what}"),
            "reply_markup": {"inline_keyboard": [[{"text": "Allow", "callback_data": "approve:yes"}, {"text": "No", "callback_data": "approve:no"}]]}});
        if let Ok(m) = call(&c, &token, "sendMessage", body, Duration::from_secs(15)).await {
            *c.tg_approval.lk() = m["message_id"].as_i64().map(|id| (chat, id));
        }
    });
}

/// The approval was answered (anywhere) or timed out: take the buttons off the card.
pub fn approval_done(core: &Arc<Core>, yes: bool) {
    let Some((chat, id)) = core.tg_approval.lk().take() else { return };
    let (_, token, _) = settings(core);
    let c = core.clone();
    tokio::spawn(async move {
        let text = if yes { "Allowed." } else { "Not allowed." };
        let _ = call(&c, &token, "editMessageText", json!({"chat_id": chat, "message_id": id, "text": text}), Duration::from_secs(15)).await;
    });
}

async fn answer_button(core: &Arc<Core>, token: &str, q: &Value) {
    let from = q["message"]["chat"]["id"].as_i64().unwrap_or(0);
    let (_, _, chats) = settings(core);
    let reply = if !chats.iter().any(|c| c.id == from) {
        "Not paired."
    } else {
        match q["data"].as_str() {
            Some("approve:yes") => {
                core.answer_approval(true);
                "Allowed"
            }
            Some("approve:no") => {
                core.answer_approval(false);
                "Not allowed"
            }
            _ => "",
        }
    };
    let _ = call(core, token, "answerCallbackQuery", json!({"callback_query_id": q["id"], "text": reply}), Duration::from_secs(10)).await;
}

/// For the settings window: is the bot connected, and as whom.
pub fn status(core: &Core) -> Value {
    let (on, token, chats) = settings(core);
    json!({
        "enabled": on && !token.is_empty(),
        "bot": *core.telegram_bot.lk(),
        "chats": chats,
        "ready": core.ready.load(Ordering::Relaxed),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_replies_are_split_at_breaks() {
        assert_eq!(chunks("hello"), vec!["hello"]);
        let para = "x".repeat(3000);
        let text = format!("{para}\n\n{para}");
        let parts = chunks(&text);
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|p| p.chars().count() <= LIMIT && p.chars().all(|c| c == 'x')));
        let unbroken = "y".repeat(9000);
        let parts = chunks(&unbroken);
        assert_eq!(parts.concat(), unbroken, "nothing is lost when there's no break to use");
    }
}
