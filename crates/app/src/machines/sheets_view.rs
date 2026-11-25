use gpui::{div, prelude::*, px, Context, Div, SharedString, Stateful};
use notchling_kit::metric::{RADIUS_CARD, RADIUS_MD};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, editor_field, hsla, icon, label, scrollable, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::editors::Key;
use super::forms::{
    AddField, AddForm, JobField, JobForm, RepoField, ADD_INTRO, ALL_REPOS_NOTE, HEALTH_PLACEHOLDER,
};
use super::model::Sheet;
use super::pieces::FixPrompt;
use super::view::{busy_line, marked, mono_lines, state_icon, state_mark, text_line, MachinesView};
use crate::sheets::header::{card_title, close_button};
use crate::vault::pick_view::pick_block;

fn state(enabled: bool) -> PartState {
    if enabled {
        PartState::Rest
    } else {
        PartState::Disabled
    }
}

fn row_label(text: &str, width: f32, a: Appearance) -> Div {
    div()
        .w(px(width))
        .flex_shrink_0()
        .child(label(text, TypeRole::Label, Ink::TextMuted, a))
}

pub(crate) fn checkbox<V: 'static>(
    id: SharedString,
    title: &str,
    on_now: bool,
    a: Appearance,
    cx: &mut Context<V>,
    toggle: impl Fn(&mut V, &mut Context<V>) + 'static,
) -> Stateful<Div> {
    let mark = div()
        .size(px(16.0))
        .flex_shrink_0()
        .rounded(px(4.0))
        .border(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .fill(if on_now { Ink::Accent } else { Ink::Raised }, a)
        .flex()
        .items_center()
        .justify_center()
        .when(on_now, |m| {
            m.child(icon(
                notchling_kit::Icon::Check,
                TypeRole::Meta.spec().size,
                Ink::OnAccent,
                a,
            ))
        });
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(8.0))
        .cursor_pointer()
        .child(mark)
        .child(label(title, TypeRole::Body, Ink::Text, a))
        .on_click(cx.listener(move |this, _, _, cx| {
            toggle(this, cx);
            cx.notify();
        }))
}

impl MachinesView {
    pub(super) fn sheet_overlay(&self, a: Appearance, cx: &mut Context<Self>) -> Option<Div> {
        let (width, body) = match self.model.sheet()? {
            Sheet::Fix(prompt) => (600.0, self.fix_sheet(prompt, a, cx)),
            Sheet::Add(form) => (520.0, self.add_sheet(form, a, cx)),
            Sheet::Job(form) => (620.0, self.job_sheet(form, a, cx)),
        };
        Some(
            div()
                .absolute()
                .size_full()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(hsla(Ink::Shadow.color(a)))
                .child(
                    card(a)
                        .w(px(width))
                        .max_h(px(700.0))
                        .rounded(px(RADIUS_CARD))
                        .p(px(20.0))
                        .flex()
                        .flex_col()
                        .gap(px(12.0))
                        .child(body),
                ),
        )
    }

    fn footer(buttons: Vec<Stateful<Div>>, leading: Option<Div>) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .children(leading)
            .child(div().flex_1())
            .children(buttons)
    }

    fn fix_sheet(&self, prompt: &FixPrompt, a: Appearance, cx: &mut Context<Self>) -> Div {
        let mut column = div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(card_title(
                label(prompt.title(), TypeRole::Title, Ink::Text, a),
                close_button("fix-close", a, cx, |this: &mut Self, _, cx| {
                    this.cancel_sheet(cx)
                }),
            ))
            .child(label(&prompt.intro(), TypeRole::Body, Ink::TextMuted, a))
            .child(scrollable(
                mono_lines("fix-commands", &prompt.text(), 300.0, a).min_h(px(80.0)),
            ));
        if let Some(blocked) = prompt.blocked() {
            column = column.child(text_line(
                &format!("⛔ {blocked}"),
                TypeRole::Label,
                Ink::Expired,
                a,
            ));
        } else if let Some(warning) = prompt.warning() {
            column = column.child(marked(
                Icon::Warning,
                &warning,
                TypeRole::Label,
                Ink::AccentText,
                a,
            ));
        } else if prompt.needs_password() {
            let editor = self.editor(
                Key::Password,
                &format!("Admin password for {}@{}", prompt.user, prompt.host),
                true,
                self.model.password_text(),
                cx,
            );
            column = column.child(div().id("fix-password").w_full().child(editor_field(
                &editor,
                self.editors.is_focus(&Key::Password),
                a,
            )));
            column = column.child(pick_block(
                "fix",
                &self.model.vault_pick,
                self.model.admin_entry().as_deref(),
                false,
                a,
                cx,
                MachinesView::vault_event,
            ));
            if let Some(error) = &self.model.vault_error {
                column = column.child(text_line(error, TypeRole::Label, Ink::Expired, a));
            }
        }
        let can_run = prompt.can_run(self.model.password_ready());
        let run = button("fix-run", "Run", ButtonKind::Primary, state(can_run), a)
            .when(can_run, |b| {
                b.on_click(cx.listener(|this, _, _, cx| this.confirm_sheet(cx)))
            });
        column.child(Self::footer(vec![run], None))
    }

    fn add_field(
        &self,
        form: &AddForm,
        which: AddField,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let key = Key::Add(which);
        let editor = self.editor(
            key.clone(),
            which.placeholder(),
            false,
            form.text(which),
            cx,
        );
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(row_label(which.title(), 48.0, a))
            .child(
                div()
                    .id(SharedString::from(format!("add-field-{which:?}")))
                    .flex_1()
                    .child(editor_field(&editor, self.editors.is_focus(&key), a).w_full()),
            )
    }

    fn add_sheet(&self, form: &AddForm, a: Appearance, cx: &mut Context<Self>) -> Div {
        let existing = self.model.names();
        let can_add = self.model.can_add();
        let mut column = div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(card_title(
                label("Add Machine", TypeRole::Title, Ink::Text, a),
                close_button("add-close", a, cx, |this: &mut Self, _, cx| {
                    this.cancel_sheet(cx)
                }),
            ))
            .child(text_line(ADD_INTRO, TypeRole::Body, Ink::TextMuted, a));
        for which in AddField::ALL {
            column = column.child(self.add_field(form, which, a, cx));
        }
        for problem in form.problems(&existing) {
            column = column.child(label(&problem, TypeRole::Meta, Ink::Expired, a));
        }
        if let Some(check) = &form.check {
            let mut result = div().flex().flex_col().gap(px(4.0));
            for line in &check.lines {
                let (mark, ink) = state_mark(Some(&line.state));
                result = result.child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(8.0))
                        .child(state_icon(
                            SharedString::from(format!("add-check-{}", line.title)),
                            mark,
                            ink,
                            TypeRole::Body,
                            a,
                        ))
                        .child(label(&line.title, TypeRole::Body, Ink::Text, a))
                        .child(
                            label(line.state.text(), TypeRole::Meta, Ink::TextMuted, a)
                                .flex_1()
                                .min_w(px(0.0))
                                .line_clamp(3),
                        ),
                );
            }
            if !check.abilities_line.is_empty() {
                result = result.child(label(
                    &check.abilities_line,
                    TypeRole::Meta,
                    Ink::TextMuted,
                    a,
                ));
            }
            column = column.child(result);
        }
        if let Some(error) = &form.error {
            column = column.child(text_line(error, TypeRole::Meta, Ink::Expired, a));
        }
        let checking = form.checking.then(|| {
            busy_line(
                "add-checking",
                "Checking…",
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            )
        });
        let can_check = form.can_check(&existing, can_add);
        let check = button(
            "add-check",
            "Check",
            ButtonKind::Secondary,
            state(can_check),
            a,
        )
        .when(can_check, |b| {
            b.on_click(cx.listener(|this, _, _, cx| this.check_draft(cx)))
        });
        let may_add = form.can_add(&existing, can_add);
        let add = button("add-add", "Add", ButtonKind::Primary, state(may_add), a)
            .when(may_add, |b| {
                b.on_click(cx.listener(|this, _, _, cx| this.add_machine(cx)))
            });
        column.child(Self::footer(vec![check, add], checking))
    }

    fn job_field(
        &self,
        form: &JobForm,
        which: JobField,
        title: &str,
        placeholder: &str,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let key = Key::Job(which);
        let editor = self.editor(key.clone(), placeholder, false, form.text(which), cx);
        let id = match which {
            JobField::Health => "job-field-health".to_owned(),
            JobField::Repo(i, f) => format!("job-field-{i}-{f:?}"),
        };
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(row_label(title, 110.0, a))
            .child(
                div()
                    .id(SharedString::from(id))
                    .flex_1()
                    .child(editor_field(&editor, self.editors.is_focus(&key), a).w_full()),
            )
    }

    fn repo_fields(
        &self,
        form: &JobForm,
        index: usize,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut column = card(a)
            .rounded(px(RADIUS_MD))
            .p(px(12.0))
            .flex()
            .flex_col()
            .gap(px(6.0));
        for f in RepoField::ALL {
            column = column.child(self.job_field(
                form,
                JobField::Repo(index, f),
                f.title(),
                f.placeholder(),
                a,
                cx,
            ));
        }
        let remove = button(
            SharedString::from(format!("remove-repo-{index}")),
            "Remove repository",
            ButtonKind::Ghost,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(Sheet::Job(form)) = this.model.sheet_mut() {
                form.remove_repo(index);
            }
            cx.notify();
        }));
        column.child(div().child(remove))
    }

    fn job_sheet(&self, form: &JobForm, a: Appearance, cx: &mut Context<Self>) -> Div {
        let mut body = div()
            .id("job-sheet-body")
            .min_h(px(200.0))
            .max_h(px(420.0))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(14.0));
        for index in 0..form.draft.repos.len() {
            body = body.child(self.repo_fields(form, index, a, cx));
        }
        let add_repo = button(
            "add-repo",
            "Add repository",
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|this, _, window, cx| {
            if let Some(Sheet::Job(form)) = this.model.sheet_mut() {
                form.add_repo();
            }
            window.focus(&this.focus);
            this.want_sheet_focus();
            cx.notify();
        }));
        body = body.child(div().child(add_repo));
        let mut kinds = div().flex().items_center().gap(px(14.0)).child(label(
            "Kinds",
            TypeRole::Label,
            Ink::TextMuted,
            a,
        ));
        for kind in form.kinds() {
            let k = (*kind).to_owned();
            kinds = kinds.child(checkbox(
                SharedString::from(format!("kind-{kind}")),
                kind,
                form.has_kind(kind),
                a,
                cx,
                move |this, _| {
                    if let Some(Sheet::Job(form)) = this.model.sheet_mut() {
                        form.toggle_kind(&k);
                    }
                },
            ));
        }
        body = body
            .child(kinds)
            .child(self.job_field(
                form,
                JobField::Health,
                "Health URL",
                HEALTH_PLACEHOLDER,
                a,
                cx,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(checkbox(
                        "token-all-repos".into(),
                        "Token may reach all repositories",
                        form.draft.token_all_repos,
                        a,
                        cx,
                        |this, _| {
                            if let Some(Sheet::Job(form)) = this.model.sheet_mut() {
                                form.toggle_all_repos();
                            }
                        },
                    ))
                    .child(text_line(ALL_REPOS_NOTE, TypeRole::Meta, Ink::TextMuted, a)),
            );
        if let Some(day) = &form.draft.token_expires {
            body = body.child(label(
                &format!("Token expires {day} (set by the token check)"),
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            ));
        }
        let mut column = div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(card_title(
                label(form.title(), TypeRole::Title, Ink::Text, a),
                close_button("job-close", a, cx, |this: &mut Self, _, cx| {
                    this.cancel_sheet(cx)
                }),
            ))
            .child(label(
                &format!("on {}", form.draft.machine),
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            ))
            .child(scrollable(body));
        for problem in form.problems() {
            column = column.child(label(&problem, TypeRole::Meta, Ink::Expired, a));
        }
        if let Some(error) = &form.error {
            column = column.child(text_line(error, TypeRole::Meta, Ink::Expired, a));
        }
        let can_save = form.can_save();
        let save = button("job-save", "Save", ButtonKind::Primary, state(can_save), a)
            .when(can_save, |b| {
                b.on_click(cx.listener(|this, _, _, cx| this.save_job(cx)))
            });
        column.child(Self::footer(vec![save], None))
    }
}
