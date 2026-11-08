//! The ops a logged-in client can ask for (the rest of Swift `BridgeRouter`). Request fields are
//! read as leniently as the Swift app read them: a field of the wrong type counts as absent, so a
//! default applies where there is one.

use super::backend::AuditEntry;
use super::connection::Connection;
use super::model::{
    Control, PendingPrompt, PhoneOptions, PromptKind, PromptQuestion, PromptResponse, Session,
    ALREADY_CONTROLLABLE,
};
use super::router::{validated_folder, Router, MAX_TEXT_CHARACTERS, MODELS};
use super::uploads::{ImageStore, UploadError, CHUNK_BYTES, MAX_BYTES};
use super::wire::{self, BridgeFailure};
use base64::Engine;
use serde_json::{json, Map, Value};
use std::sync::Arc;

type Body = Map<String, Value>;

fn string<'a>(body: &'a Body, key: &str) -> Option<&'a str> {
    body.get(key).and_then(Value::as_str)
}

fn int(body: &Body, key: &str) -> Option<i64> {
    body.get(key).and_then(Value::as_i64)
}

fn require<'a>(body: &'a Body, key: &str) -> Result<&'a str, BridgeFailure> {
    string(body, key)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| BridgeFailure::invalid(format!("Thiếu \"{key}\".")))
}

fn view_only() -> BridgeFailure {
    BridgeFailure::view_only("Session mở ngoài Console — chỉ xem.")
}

fn busy_sending() -> BridgeFailure {
    BridgeFailure::busy("Đang gửi tin trước đó vào session này.")
}

/// `http(s)://localhost` or `http(s)://127.0.0.1`, any port and path (`WebURLResolver.localURL`).
pub fn local_url(text: &str) -> Option<&str> {
    let rest = text
        .strip_prefix("http://")
        .or_else(|| text.strip_prefix("https://"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // The host is what follows any user info.
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let (host, port) = match host_port.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (host_port, None),
    };
    let port_ok = port.is_none_or(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    (port_ok && (host == "localhost" || host == "127.0.0.1")).then_some(text)
}

impl Router {
    fn session(&self, id: &str) -> Result<Session, BridgeFailure> {
        self.backend
            .sessions()
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| BridgeFailure::not_found("Không có session này."))
    }

    /// Runs `send` while no other send to the same session from this connection is running.
    fn exclusive<T>(
        &self,
        connection: &Connection,
        session_id: &str,
        busy: BridgeFailure,
        send: impl FnOnce() -> Result<T, BridgeFailure>,
    ) -> Result<T, BridgeFailure> {
        if !connection.begin_send(session_id) {
            return Err(busy);
        }
        let result = send();
        connection.end_send(session_id);
        result
    }

    pub(crate) fn chat_get(&self, body: &Body) -> Result<Value, BridgeFailure> {
        let session_id = require(body, "sessionId")?;
        self.session(session_id)?;
        let limit = int(body, "limit").unwrap_or(50).clamp(1, 200) as usize;
        let page = self.backend.chat_page(
            session_id,
            string(body, "subagentId"),
            string(body, "before"),
            string(body, "after"),
            limit,
        )?;
        let mut answer =
            json!({"messages": wire::object(&page.messages), "hasMore": page.has_more});
        if page.reset {
            answer["reset"] = Value::Bool(true);
        }
        Ok(answer)
    }

    /// The JSON answer first, then an `IMG1` binary frame; the returned `{sent}` is the second answer.
    pub(crate) fn chat_image(
        &self,
        body: &Body,
        id: Option<&Value>,
        connection: &Arc<Connection>,
    ) -> Result<Value, BridgeFailure> {
        let session_id = require(body, "sessionId")?;
        let image_id = require(body, "imageId")?;
        let well_formed = image_id.chars().count() <= 80
            && image_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-');
        if !well_formed {
            return Err(BridgeFailure::invalid("imageId không hợp lệ."));
        }
        self.session(session_id)?;
        let width = int(body, "maxWidth").unwrap_or(800).clamp(64, 1600);
        let jpeg = self.backend.chat_image(session_id, image_id, width)?;
        let announce =
            json!({"binary": true, "bytes": jpeg.len(), "format": "jpeg", "imageId": image_id});
        connection.send_text(wire::ok(id, announce));
        connection.send_binary(wire::frame::image(wire::request_number(id), &jpeg));
        Ok(json!({"sent": true}))
    }

    pub(crate) fn viz_get(&self, body: &Body) -> Result<Value, BridgeFailure> {
        let session_id = require(body, "sessionId")?;
        self.session(session_id)?;
        let include_entity = body.get("entity").and_then(Value::as_bool) == Some(true);
        let viz = self.backend.viz(session_id, include_entity)?;
        let mut data = json!({"suggested": viz.suggested_tab});
        if let Some(url) = viz.web_url {
            data["webURL"] = Value::from(url);
        }
        if !viz.web_candidates.is_empty() {
            data["webCandidates"] = Value::from(viz.web_candidates);
        }
        for (key, spec) in &viz.specs {
            data[key.as_str()] = wire::object(spec);
        }
        Ok(data)
    }

    pub(crate) fn web_snapshot(
        &self,
        body: &Body,
        id: Option<&Value>,
        connection: &Arc<Connection>,
    ) -> Result<Value, BridgeFailure> {
        let session_id = require(body, "sessionId")?;
        self.session(session_id)?;
        let width = int(body, "width").unwrap_or(400).clamp(100, 1200);
        let url = match body.get("url").and_then(Value::as_str) {
            Some(text) => local_url(text)
                .ok_or_else(|| {
                    BridgeFailure::invalid(
                        "Chỉ chụp được URL http(s) trên localhost hoặc 127.0.0.1.",
                    )
                })?
                .to_string(),
            None => self
                .backend
                .viz(session_id, false)?
                .web_url
                .filter(|u| local_url(u).is_some())
                .ok_or_else(|| BridgeFailure::not_found("Session này chưa có URL web."))?,
        };
        let jpeg = self.backend.web_snapshot(&url, width)?;
        // The JSON answer goes first, so the client knows a binary frame follows.
        let announce = json!({"binary": true, "bytes": jpeg.len(), "format": "jpeg", "url": url});
        connection.send_text(wire::ok(id, announce));
        connection.send_binary(wire::frame::snapshot(wire::request_number(id), &jpeg));
        Ok(json!({"sent": true}))
    }

    pub(crate) fn session_message(
        &self,
        body: &Body,
        connection: &Arc<Connection>,
    ) -> Result<Value, BridgeFailure> {
        let session_id = require(body, "sessionId")?;
        let text = require(body, "text")?;
        let length = text.chars().count();
        if length > MAX_TEXT_CHARACTERS {
            return Err(BridgeFailure::invalid("Tin nhắn quá dài."));
        }
        let target = self.session(session_id)?;
        if target.control == Control::ViewOnly {
            return Err(view_only());
        }
        self.exclusive(connection, session_id, busy_sending(), || {
            self.backend.send_message(text, &target)
        })?;
        self.audit(connection, "message", Some(session_id), Some(length));
        Ok(json!({"sent": true}))
    }

    /// One Esc interrupts the running turn, then Claude Code runs the queued message by itself.
    /// Only terminals the computer owns.
    pub(crate) fn session_send_now(
        &self,
        body: &Body,
        connection: &Arc<Connection>,
    ) -> Result<Value, BridgeFailure> {
        let session_id = require(body, "sessionId")?;
        let target = self.session(session_id)?;
        if target.control != Control::ConsolePty {
            return Err(BridgeFailure::view_only(
                "Chỉ ngắt được session do Console giữ terminal.",
            ));
        }
        let busy = BridgeFailure::busy("Đang gửi lệnh trước đó vào session này.");
        self.exclusive(connection, session_id, busy, || {
            if !self.allow_send_now(session_id) {
                return Err(BridgeFailure::busy("Vừa ngắt xong, đợi một chút."));
            }
            self.backend.send_now(&target)
        })?;
        self.audit(connection, "sendNow", Some(session_id), Some(0));
        Ok(json!({"sent": true}))
    }

    pub(crate) fn image_begin(
        &self,
        body: &Body,
        connection: &Arc<Connection>,
    ) -> Result<Value, BridgeFailure> {
        // The photo only ever reaches the session as an acting call: while acting is off, none of
        // it is taken in.
        if !self.backend.acting_on() {
            return Err(BridgeFailure::invalid(super::live::ACTING_OFF));
        }
        let session_id = require(body, "sessionId")?;
        let bytes = int(body, "bytes")
            .filter(|b| *b > 0 && *b as usize <= MAX_BYTES)
            .ok_or_else(|| {
                BridgeFailure::invalid(format!("Ảnh phải nhỏ hơn {} MB.", MAX_BYTES / 1_048_576))
            })?;
        let caption = string(body, "caption").unwrap_or_default();
        if caption.chars().count() > MAX_TEXT_CHARACTERS {
            return Err(BridgeFailure::invalid("Chú thích quá dài."));
        }
        if self.session(session_id)?.control == Control::ViewOnly {
            return Err(view_only());
        }
        let upload_id = self.uploads.begin(
            connection.id,
            session_id,
            bytes as usize,
            caption,
            self.now_ms(),
        );
        Ok(json!({"uploadId": upload_id, "chunkBytes": CHUNK_BYTES}))
    }

    pub(crate) fn image_chunk(
        &self,
        body: &Body,
        connection: &Arc<Connection>,
    ) -> Result<Value, BridgeFailure> {
        let upload_id = require(body, "uploadId")?;
        let missing = || BridgeFailure::invalid("Thiếu \"seq\" hoặc \"data\" (base64).");
        let sequence = int(body, "seq").ok_or_else(missing)?;
        let chunk = string(body, "data")
            .and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).ok())
            .ok_or_else(missing)?;
        self.uploads
            .append(upload_id, connection.id, sequence, &chunk, self.now_ms())
            .map_err(upload_failure)?;
        Ok(json!({"received": sequence}))
    }

    pub(crate) fn image_end(
        &self,
        body: &Body,
        connection: &Arc<Connection>,
    ) -> Result<Value, BridgeFailure> {
        let upload_id = require(body, "uploadId")?;
        let upload = self
            .uploads
            .finish(upload_id, connection.id, self.now_ms())
            .map_err(upload_failure)?;
        let ext = ImageStore::detect(&upload.data)
            .ok_or_else(|| BridgeFailure::invalid("Chỉ nhận ảnh JPEG, PNG, GIF hoặc WebP."))?;
        let target = self.session(&upload.session_id)?;
        if target.control == Control::ViewOnly {
            return Err(view_only());
        }
        self.exclusive(connection, &upload.session_id, busy_sending(), || {
            let file = self
                .images
                .save(&upload.data, ext, self.now_ms())
                .map_err(|_| BridgeFailure::internal("Mac không lưu được ảnh."))?;
            let message = ImageStore::message(&upload.caption, &file.to_string_lossy());
            // The path reaches the session only through the backend, which refuses while acting is
            // off; a photo nobody was told about is not kept.
            let sent = self.backend.send_message(&message, &target);
            if sent.is_err() {
                let _ = std::fs::remove_file(&file);
            }
            sent
        })?;
        self.audit(
            connection,
            "image",
            Some(&upload.session_id),
            Some(upload.data.len()),
        );
        Ok(json!({"sent": true}))
    }

    pub(crate) fn commands_list(&self, body: &Body) -> Result<Value, BridgeFailure> {
        let session_id = require(body, "sessionId")?;
        self.session(session_id)?;
        Ok(json!({"commands": wire::object(&self.backend.commands(session_id))}))
    }

    pub(crate) fn prompt_answer(
        &self,
        body: &Body,
        connection: &Arc<Connection>,
    ) -> Result<Value, BridgeFailure> {
        let request_id = require(body, "requestId")?;
        let prompt: PendingPrompt = self
            .backend
            .pending_prompt(request_id)
            .ok_or_else(|| BridgeFailure::not_found("Không còn câu hỏi hoặc permission này."))?;
        let response = match &prompt.kind {
            PromptKind::Permission => match body.get("allow").and_then(Value::as_bool) {
                Some(true) => PromptResponse::Allow,
                Some(false) => PromptResponse::Deny,
                None => {
                    return Err(BridgeFailure::invalid(
                        "Permission cần \"allow\": true hoặc false.",
                    ))
                }
            },
            PromptKind::Question(questions) => {
                if questions.iter().any(|q| q.multi_select) {
                    return Err(BridgeFailure::invalid(
                        "Câu hỏi chọn nhiều đáp án: trả lời trên terminal.",
                    ));
                }
                PromptResponse::Answers(answers(body, questions)?)
            }
        };
        self.backend.respond(&prompt, &response)?;
        self.audit(connection, "answer", Some(&prompt.session_id), None);
        Ok(json!({"answered": true}))
    }

    pub(crate) fn session_new(
        &self,
        body: &Body,
        connection: &Arc<Connection>,
    ) -> Result<Value, BridgeFailure> {
        let cwd = require(body, "cwd")?;
        let model = string(body, "model");
        if model.is_some_and(|m| !MODELS.contains(&m)) {
            return Err(BridgeFailure::invalid("Model không hợp lệ."));
        }
        let folder = validated_folder(cwd, &self.home)?;
        self.backend
            .new_session(&folder, model.filter(|m| *m != "default"))?;
        self.audit(connection, "newSession", None, None);
        Ok(json!({"started": true}))
    }

    /// Only view-only sessions; the backend checks again that the original is gone or stoppable.
    pub(crate) fn session_takeover(
        &self,
        body: &Body,
        connection: &Arc<Connection>,
    ) -> Result<Value, BridgeFailure> {
        let id = require(body, "sessionId")?;
        let stop_original = match body.get("stopOriginal") {
            None => false,
            Some(raw) => raw.as_bool().ok_or_else(|| {
                BridgeFailure::invalid("\"stopOriginal\" phải là true hoặc false.")
            })?,
        };
        let target = self.session(id)?;
        if target.control != Control::ViewOnly {
            return Err(BridgeFailure::invalid(ALREADY_CONTROLLABLE));
        }
        let outcome = self.backend.take_over(&target, stop_original)?;
        self.audit_entry(AuditEntry {
            device: connection.device_name(),
            op: "takeover".into(),
            session: Some(id.to_string()),
            address: connection.audit_address(),
            stopped_pid: outcome.stopped_pid,
            ..AuditEntry::default()
        });
        let mut answer = json!({"sessionId": outcome.session_id, "resumed": true});
        if let Some(pid) = outcome.stopped_pid {
            answer["stoppedPid"] = Value::from(pid);
        }
        Ok(answer)
    }

    pub(crate) fn subscribe(
        &self,
        body: &Body,
        connection: &Arc<Connection>,
    ) -> Result<Value, BridgeFailure> {
        let phone = body
            .get("phone")
            .and_then(Value::as_object)
            .map(|p| PhoneOptions {
                device_window_id: int(p, "deviceWindowId").map(|n| n as u32),
                fps: int(p, "fps").unwrap_or(10).clamp(1, 15),
                width: int(p, "width").unwrap_or(400).clamp(100, 600),
            });
        let session_id = string(body, "sessionId");
        if let Some(id) = session_id {
            self.session(id)?;
        }
        let sessions = self.backend.sessions();
        if body.get("sessions").and_then(Value::as_bool) == Some(true) {
            {
                let mut sub = connection.subscription();
                sub.wants_sessions = true;
                sub.known_prompt_ids = sessions
                    .iter()
                    .filter_map(|s| s.pending.as_ref().map(|p| p.id.clone()))
                    .collect();
                sub.last_sessions_json = Some(super::hub::sessions_json(&sessions));
            }
            connection.send_text(super::hub::sessions_event(&sessions));
        }
        if let Some(id) = session_id {
            let subagent = string(body, "subagentId");
            let current = self.backend.chat_messages(id, subagent).unwrap_or_default();
            // Everything after the phone's newest message is news and gets pushed, so a reply that
            // came while the phone looked elsewhere isn't lost. Unknown or absent: all is known.
            let known_up_to = string(body, "lastMessageId")
                .and_then(|last| current.iter().position(|m| m.id == last))
                .map_or(current.len(), |i| i + 1);
            let mut sub = connection.subscription();
            sub.chat_session_id = Some(id.to_string());
            sub.chat_subagent_id = subagent.map(String::from);
            sub.known_chat_ids = current[..known_up_to]
                .iter()
                .map(|m| m.id.clone())
                .collect();
        }
        if let Some(options) = phone {
            self.backend.start_phone(connection, &options)?;
            connection.subscription().phone_active = true;
        }
        Ok(json!({"subscribed": true}))
    }

    pub(crate) fn unsubscribe(&self, connection: &Arc<Connection>) {
        let streaming = {
            let mut sub = connection.subscription();
            sub.wants_sessions = false;
            sub.chat_session_id = None;
            sub.chat_subagent_id = None;
            std::mem::take(&mut sub.phone_active)
        };
        if streaming {
            self.backend.stop_phone(connection);
        }
    }
}

fn upload_failure(error: UploadError) -> BridgeFailure {
    match error {
        UploadError::OutOfOrder => BridgeFailure::invalid("Mảnh ảnh sai thứ tự."),
        UploadError::TooBig => BridgeFailure::invalid("Mảnh ảnh quá lớn."),
        UploadError::Incomplete => BridgeFailure::invalid("Ảnh chưa gửi đủ."),
        UploadError::Unknown => BridgeFailure::not_found("Không có lượt gửi ảnh này (hết hạn?)."),
    }
}

/// `choice`: a 1-based option number or its label (an array for several questions); `text`: a
/// free answer to a single question.
pub fn answers(body: &Body, questions: &[PromptQuestion]) -> Result<Vec<String>, BridgeFailure> {
    if let Some(text) = string(body, "text") {
        if questions.len() != 1 || text.is_empty() || text.chars().count() > 500 {
            return Err(BridgeFailure::invalid(
                "\"text\" chỉ dùng cho một câu hỏi, tối đa 500 ký tự.",
            ));
        }
        return Ok(vec![text.to_string()]);
    }
    let raw: Vec<&Value> = match body.get("choice") {
        Some(Value::Array(list)) => list.iter().collect(),
        Some(single) => vec![single],
        None => return Err(BridgeFailure::invalid("Cần \"choice\" hoặc \"text\".")),
    };
    if raw.len() != questions.len() {
        return Err(BridgeFailure::invalid(format!(
            "Cần {} câu trả lời.",
            questions.len()
        )));
    }
    raw.iter()
        .zip(questions)
        .map(|(value, question)| {
            if let Some(number) = value.as_i64() {
                return usize::try_from(number)
                    .ok()
                    .filter(|n| (1..=question.options.len()).contains(n))
                    .map(|n| question.options[n - 1].clone())
                    .ok_or_else(|| BridgeFailure::invalid(format!("Lựa chọn {number} không có.")));
            }
            value
                .as_str()
                .filter(|label| question.options.iter().any(|o| o == label))
                .map(String::from)
                .ok_or_else(|| BridgeFailure::invalid("Lựa chọn không khớp đáp án nào."))
        })
        .collect()
}
