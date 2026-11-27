use std::collections::{HashMap, HashSet};

use gpui::{
    div, prelude::*, px, App, Context, Div, Entity, SharedString, Stateful, Subscription, Window,
};
use notchling_kit::metric::{RADIUS_SM, SPACE};
use notchling_kit::ui::{editor_field, hsla, icon, label, KitStyled, TextEditor};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;
use serde_json::Value;

use super::consent_card::ConsentCard;
use super::consent_family::{
    family_apply, part_also, part_counts, part_field, part_hash, part_title, toggled, Admin, Part,
};
use super::run::Run;
use super::view::InboxPanelView;
use notchling_engine::vault::entry::VaultEntry;

use crate::vault::pick::{local_entries, VaultPick};
use crate::vault::pick_view::{pick_block, PickEvent};

const TICK: f32 = 14.0;

#[derive(Default)]
pub struct FamilyState {
    unticked: HashMap<String, HashSet<String>>,
    editors: HashMap<String, Entity<TextEditor>>,
    vault: HashMap<String, String>,
    picks: HashMap<String, VaultPick>,
    subs: Vec<Subscription>,
}

pub fn only_entry(
    result: Result<Vec<VaultEntry>, String>,
    name: Option<&str>,
) -> Result<Vec<VaultEntry>, String> {
    let mut entries = result?;
    match name {
        Some(name) => {
            entries.retain(|e| notchling_engine::machines::names::same_name(&e.name, name))
        }
        None => entries.clear(),
    }
    Ok(entries)
}

impl FamilyState {
    pub fn unticked(&self, id: &str) -> HashSet<String> {
        self.unticked.get(id).cloned().unwrap_or_default()
    }

    fn typed(&self, id: &str, cx: &App) -> Option<String> {
        self.editors
            .get(id)
            .map(|editor| editor.read(cx).text().to_owned())
            .filter(|text| !text.is_empty())
    }

    pub fn chosen(&self, card: &str) -> Option<&str> {
        self.vault.get(card).map(String::as_str)
    }

    pub fn choose(&mut self, card: &str, name: &str) {
        self.vault.insert(card.to_owned(), name.to_owned());
        self.close_pick(card);
    }

    pub fn clear_choice(&mut self, card: &str) {
        self.vault.remove(card);
        self.close_pick(card);
    }

    pub fn toggle_pick(&mut self, card: &str) -> bool {
        self.picks.entry(card.to_owned()).or_default().toggle()
    }

    fn close_pick(&mut self, card: &str) {
        if let Some(pick) = self.picks.get_mut(card) {
            pick.close();
        }
    }

    pub fn pick_of(&self, card: &str) -> VaultPick {
        self.picks.get(card).cloned().unwrap_or_default()
    }

    pub fn loaded(
        &mut self,
        card: &str,
        only: Option<&str>,
        result: Result<Vec<VaultEntry>, String>,
    ) {
        self.picks
            .entry(card.to_owned())
            .or_default()
            .loaded(only_entry(result, only));
    }

    pub fn admin_ready(&self, id: &str, cx: &App) -> bool {
        self.vault.contains_key(id) || self.typed(id, cx).is_some()
    }

    pub fn admin(&self, id: &str, cx: &App) -> Option<Admin> {
        match self.vault.get(id) {
            Some(name) => Some(Admin::Vault(name.clone())),
            None => self
                .typed(id, cx)
                .map(|text| Admin::Password(notchling_engine::accounts::Secret::new(text))),
        }
    }

    pub fn fill(&self, run: Run, id: &str, cx: &App) -> Run {
        match run {
            Run::Family(apply, parts, _) => Run::Family(apply, parts, self.admin(id, cx)),
            other => other,
        }
    }

    pub fn forget(&mut self, id: &str, cx: &mut Context<InboxPanelView>) {
        self.unticked.remove(id);
        self.vault.remove(id);
        self.picks.remove(id);
        if let Some(editor) = self.editors.remove(id) {
            editor.update(cx, |editor, cx| editor.set_text("", cx));
        }
    }
}

fn tick_box(kept: bool, a: Appearance) -> Div {
    div()
        .size(px(TICK))
        .mt(px(2.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_SM / 2.0))
        .border(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .fill(Ink::Raised, a)
        .children(kept.then(|| icon(Icon::Check, 10.0, Ink::AccentText, a)))
}

fn part_lines(part: &Part, a: Appearance) -> Div {
    let ink = if part.kept { Ink::Text } else { Ink::TextFaint };
    let mut meta = vec![part_counts(part)];
    meta.extend(part_hash(part));
    let mut col = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.0))
        .child(label(&part_title(part), TypeRole::Body, ink, a))
        .child(label(&meta.join(" · "), TypeRole::Meta, Ink::TextMuted, a));
    for field in &part.fields {
        col = col.child(label(&part_field(field), TypeRole::Meta, Ink::TextMuted, a));
    }
    for path in &part.also {
        col = col.child(label(&part_also(path), TypeRole::Meta, Ink::TextMuted, a));
    }
    for note in &part.notes {
        col = col.child(label(note, TypeRole::Meta, Ink::TextFaint, a));
    }
    col
}

impl InboxPanelView {
    pub(super) fn family_prepare(&mut self, view: &Value, cx: &mut Context<Self>) {
        if !family_apply(view).is_some_and(|a| a.admin) {
            return;
        }
        let Some(id) = view.get("id").and_then(Value::as_str) else {
            return;
        };
        if self.family.editors.contains_key(id) {
            return;
        }
        let editor = cx.new(|cx| {
            TextEditor::new(t::consent::machine_password_placeholder(), 1, cx)
                .single_line()
                .masked()
        });
        let sub = cx.subscribe(
            &editor,
            |_, _, _: &notchling_kit::ui::TextEditorEvent, cx| cx.notify(),
        );
        self.family.subs.push(sub);
        self.family.editors.insert(id.to_owned(), editor);
    }

    fn toggle_part(&mut self, card: &str, part: &str, cx: &mut Context<Self>) {
        if self.busy.contains(card) {
            return;
        }
        toggled(
            self.family.unticked.entry(card.to_owned()).or_default(),
            part,
        );
        cx.notify();
    }

    fn part_row(
        &self,
        card: &ConsentCard,
        part: &Part,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (card_id, part_id) = (card.id.clone(), part.id.clone());
        div()
            .id(SharedString::from(format!(
                "consent-part-{card_id}-{part_id}"
            )))
            .cursor_pointer()
            .flex()
            .items_start()
            .gap(px(SPACE[1]))
            .child(tick_box(part.kept, a))
            .child(part_lines(part, a))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_part(&card_id, &part_id, cx)))
    }

    fn admin_vault_event(
        &mut self,
        card: &str,
        only: Option<&str>,
        event: PickEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            PickEvent::Toggle => {
                if self.family.toggle_pick(card) {
                    self.read_admin_entries(card, only, cx);
                }
            }
            PickEvent::Choose(name) => self.family.choose(card, &name),
            PickEvent::Clear => self.family.clear_choice(card),
            PickEvent::New => {}
        }
        cx.notify();
    }

    fn read_admin_entries(&self, card: &str, only: Option<&str>, cx: &mut Context<Self>) {
        let (card, only) = (card.to_owned(), only.map(str::to_owned));
        let vault = cx
            .try_global::<crate::mac::shell::AppVault>()
            .map(|v| v.0.clone());
        let switches = cx
            .try_global::<crate::mac::shell::AppSwitchFeed>()
            .map(|f| f.0.switches());
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    match (vault, switches) {
                        (Some(vault), Some(switches)) => local_entries(&vault, &switches),
                        _ => Err(crate::acting::DETACHED_MESSAGE.to_owned()),
                    }
                })
                .await;
            this.update(cx, |view, cx| {
                view.family.loaded(&card, only.as_deref(), result);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn admin_block(
        &mut self,
        card: &ConsentCard,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let editor = self.family.editors.get(&card.id)?.clone();
        let focused = editor.read(cx).is_focused(window);
        let mut block = div()
            .flex()
            .flex_col()
            .gap(px(SPACE[0]))
            .child(label(
                t::consent::machine_password_label(),
                TypeRole::Label,
                Ink::TextFaint,
                a,
            ))
            .child(editor_field(&editor, focused, a));
        let entry = card
            .family_apply
            .as_ref()
            .and_then(|f| f.admin_entry.clone());
        if entry.is_some() {
            let chosen = self.family.chosen(&card.id).map(str::to_owned);
            let pick = self.family.pick_of(&card.id);
            let id = card.id.clone();
            block = block.child(pick_block(
                &format!("consent-admin-{}", card.id),
                &pick,
                chosen.as_deref(),
                false,
                a,
                cx,
                move |this: &mut InboxPanelView, event, _: &mut Window, cx| {
                    this.admin_vault_event(&id, entry.as_deref(), event, cx)
                },
            ));
        }
        Some(block)
    }

    pub(super) fn family_block(
        &mut self,
        card: &ConsentCard,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let apply = card.family_apply.as_ref()?;
        let mut block = div().flex().flex_col().gap(px(SPACE[1]));
        let mut any = false;
        if apply.family.has_parts() && !card.parts.is_empty() {
            let mut list = div().flex().flex_col().gap(px(SPACE[0]));
            for part in &card.parts {
                list = list.child(self.part_row(card, part, a, cx));
            }
            block = block.child(list);
            any = true;
        }
        if apply.admin {
            block = block.children(self.admin_block(card, window, a, cx));
            any = true;
        }
        any.then_some(block)
    }
}
