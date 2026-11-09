//! The sheets over the Machines window (Swift `FixConfirmSheet`, `AddMachineSheet`, `JobSheet`):
//! the Fix sheet shows the exact commands, then the admin password field (or the root warning, or
//! why the step can't run at all), then Cancel and Run; the Add sheet checks a machine before it
//! is saved; the Job sheet edits the sd worker job. Typing reaches the forms through the window's
//! key handler; a click on a field moves the focus there.

use gpui::{div, prelude::*, px, Context, Div, SharedString, Stateful};
use notchling_kit::metric::{RADIUS_CARD, RADIUS_MD};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, field, hsla, label, scrollable, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::forms::{
    AddField, AddForm, JobField, JobForm, RepoField, ADD_INTRO, ALL_REPOS_NOTE, HEALTH_PLACEHOLDER,
};
use super::model::Sheet;
use super::pieces::FixPrompt;
use super::view::{mono_lines, state_mark, text_line, MachinesView};

fn state(enabled: bool) -> PartState {
    if enabled {
        PartState::Rest
    } else {
        PartState::Disabled
    }
}

/// A label on the left of a field.
fn row_label(text: &str, width: f32, a: Appearance) -> Div {
    div()
        .w(px(width))
        .flex_shrink_0()
        .child(label(text, TypeRole::Label, Ink::TextMuted, a))
}

/// A checkbox row.
fn checkbox<V: 'static>(
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
            m.child(label(
                notchling_kit::Icon::Check.glyph(),
                TypeRole::Meta,
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
    /// The sheet on show, over the dimmed window.
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
                // The backdrop takes every click and hover: no card button under it is live.
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

    /// The commands, then the admin password (or the root warning, or the block), then Cancel
    /// and Run. The password lives in the model only while this sheet does (`model_sheet`).
    fn fix_sheet(&self, prompt: &FixPrompt, a: Appearance, cx: &mut Context<Self>) -> Div {
        let mut column = div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(label(prompt.title(), TypeRole::Title, Ink::Text, a))
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
            column = column.child(label(
                &format!("⚠ {warning}"),
                TypeRole::Label,
                Ink::AccentText,
                a,
            ));
        } else if prompt.needs_password() {
            let dots = "•".repeat(self.model.password_len());
            column = column.child(
                div()
                    .id("fix-password")
                    .w_full()
                    .cursor_text()
                    .child(field(
                        &dots,
                        &format!("Admin password for {}@{}", prompt.user, prompt.host),
                        true,
                        a,
                    ))
                    .on_click(cx.listener(|this, _, window, _| window.focus(&this.focus))),
            );
        }
        let can_run = prompt.can_run(self.model.password_len() > 0);
        let cancel = button(
            "fix-cancel",
            "Cancel",
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|this, _, _, cx| this.cancel_sheet(cx)));
        let run = button("fix-run", "Run", ButtonKind::Primary, state(can_run), a)
            .when(can_run, |b| {
                b.on_click(cx.listener(|this, _, _, cx| this.confirm_sheet(cx)))
            });
        column.child(Self::footer(vec![cancel, run], None))
    }

    /// A field row of the Add sheet.
    fn add_field(
        &self,
        form: &AddForm,
        which: AddField,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let focused = form.focus == Some(which);
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(row_label(which.title(), 48.0, a))
            .child(
                div()
                    .id(SharedString::from(format!("add-field-{which:?}")))
                    .flex_1()
                    .cursor_text()
                    .child(field(form.text(which), which.placeholder(), focused, a).w_full())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(Sheet::Add(form)) = this.model.sheet_mut() {
                            form.focus = Some(which);
                        }
                        window.focus(&this.focus);
                        cx.notify();
                    })),
            )
    }

    fn add_sheet(&self, form: &AddForm, a: Appearance, cx: &mut Context<Self>) -> Div {
        let existing = self.model.names();
        let can_add = self.model.can_add();
        let mut column = div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(label("Add Machine", TypeRole::Title, Ink::Text, a))
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
                let (glyph, ink) = state_mark(Some(&line.state));
                result = result.child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(8.0))
                        .child(label(glyph, TypeRole::Body, ink, a))
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
        let checking = form
            .checking
            .then(|| label("Checking…", TypeRole::Meta, Ink::TextMuted, a));
        let cancel = button(
            "add-cancel",
            "Cancel",
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|this, _, _, cx| this.cancel_sheet(cx)));
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
        column.child(Self::footer(vec![cancel, check, add], checking))
    }

    /// A field row of the Job sheet.
    fn job_field(
        &self,
        form: &JobForm,
        which: JobField,
        title: &str,
        placeholder: &str,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let focused = form.focus == Some(which);
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
                    .cursor_text()
                    .child(field(form.text(which), placeholder, focused, a).w_full())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(Sheet::Job(form)) = this.model.sheet_mut() {
                            form.focus = Some(which);
                        }
                        window.focus(&this.focus);
                        cx.notify();
                    })),
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
            .child(label(form.title(), TypeRole::Title, Ink::Text, a))
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
        let cancel = button(
            "job-cancel",
            "Cancel",
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|this, _, _, cx| this.cancel_sheet(cx)));
        let can_save = form.can_save();
        let save = button("job-save", "Save", ButtonKind::Primary, state(can_save), a)
            .when(can_save, |b| {
                b.on_click(cx.listener(|this, _, _, cx| this.save_job(cx)))
            });
        column.child(Self::footer(vec![cancel, save], None))
    }
}
