use gpui::Context;

use super::model::ClusterModel;
use super::renders::{vault_names as fixture_names, WORKER};
use super::secrets::{vault_names, SecretsModel};
use super::secrets_view::SecretsEvent;
use super::view::ClusterSheetView;

pub(super) struct Editing {
    pub(super) node: String,
    pub(super) name: String,
    pub(super) model: Option<SecretsModel>,
    pub(super) error: Option<String>,
}

impl ClusterSheetView {
    pub(super) fn open_secrets(&mut self, id: &str, cx: &mut Context<Self>) {
        let snapshot = self.feed.snapshot();
        let Some(row) = snapshot.row(id).filter(|r| r.offers_secrets()) else {
            return;
        };
        self.menu = None;
        self.note = None;
        self.editing = Some(Editing {
            node: row.id.clone(),
            name: row.name.clone(),
            model: None,
            error: None,
        });
        let (feed, local, switches) = (
            self.feed.clone(),
            self.access.local.clone(),
            self.access.switches.switches(),
        );
        let node = row.id.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let remote = feed.remote_vault();
                    vault_names(&feed.snapshot(), &local, &remote, &switches)
                })
                .await;
            this.update(cx, |view, cx| view.vault_read(&node, result, cx))
                .ok();
        })
        .detach();
        cx.notify();
    }

    fn vault_read(
        &mut self,
        node: &str,
        result: Result<Vec<String>, String>,
        cx: &mut Context<Self>,
    ) {
        let allow = self.feed.snapshot().row(node).map(|r| r.secrets.clone());
        let Some(editing) = self.editing.as_mut().filter(|e| e.node == node) else {
            return;
        };
        match (result, allow) {
            (Ok(names), Some(allow)) => editing.model = Some(SecretsModel::new(&names, &allow)),
            (Err(error), _) => editing.error = Some(error),
            (Ok(_), None) => self.editing = None,
        }
        cx.notify();
    }

    pub(super) fn secrets_event(&mut self, event: SecretsEvent, cx: &mut Context<Self>) {
        let model = self.editing.as_mut().and_then(|e| e.model.as_mut());
        match event {
            SecretsEvent::Cancel => self.editing = None,
            SecretsEvent::Toggle(name) => {
                if let Some(model) = model {
                    model.toggle(&name);
                }
            }
            SecretsEvent::Any => {
                if let Some(model) = model {
                    model.set_any(!model.any());
                }
            }
            SecretsEvent::Save => return self.save_secrets(cx),
        }
        cx.notify();
    }

    fn save_secrets(&mut self, cx: &mut Context<Self>) {
        let Some(editing) = self.editing.as_ref() else {
            return;
        };
        let request = editing
            .model
            .as_ref()
            .and_then(|m| self.feed.snapshot().set_secrets(&editing.node, m));
        let Some(request) = request.filter(|_| !self.busy && self.feed.is_live()) else {
            return;
        };
        self.busy = true;
        let feed = self.feed.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let result = feed.send(&request);
                    feed.refresh();
                    result
                })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                match result {
                    Ok(_) => view.editing = None,
                    Err(error) => {
                        if let Some(editing) = view.editing.as_mut() {
                            editing.error = Some(error);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
}

pub(super) fn secrets_fixture(model: &ClusterModel) -> Editing {
    let row = model.row(WORKER);
    Editing {
        node: WORKER.to_owned(),
        name: row.map(|r| r.name.clone()).unwrap_or_default(),
        model: row.map(|r| SecretsModel::new(&fixture_names(), &r.secrets)),
        error: None,
    }
}
