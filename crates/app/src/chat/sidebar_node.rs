use std::collections::BTreeMap;

use notchling_engine::cluster::Qualified;
use notchling_locale as t;

use super::sidebar::SidebarModel;
use crate::acting::Control;
use crate::cluster::model::ClusterModel;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Nodes {
    known: BTreeMap<String, (String, bool)>,
}

impl Nodes {
    pub fn from_cluster(model: &ClusterModel) -> Nodes {
        Nodes {
            known: model
                .rows()
                .into_iter()
                .filter(|r| !r.removed)
                .map(|r| (r.id, (r.name, r.online)))
                .collect(),
        }
    }

    pub fn with(mut self, id: &str, name: &str, online: bool) -> Nodes {
        self.known.insert(id.to_owned(), (name.to_owned(), online));
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeLabel {
    pub id: String,
    pub name: String,
    pub online: bool,
}

impl NodeLabel {
    pub fn reply_note(&self) -> Option<String> {
        (!self.online).then(|| t::cluster::node_offline(&self.name))
    }
}

pub fn node_label(row_id: &str, nodes: &Nodes) -> Option<NodeLabel> {
    let qualified = Qualified::parse(row_id)?;
    let id = qualified.node.to_string();
    Some(match nodes.known.get(&id) {
        Some((name, online)) => NodeLabel {
            name: name.clone(),
            online: *online,
            id,
        },
        None => NodeLabel {
            name: id.clone(),
            online: false,
            id,
        },
    })
}

impl SidebarModel {
    pub fn with_nodes(mut self, nodes: &Nodes) -> SidebarModel {
        for section in &mut self.sections {
            for row in &mut section.rows {
                row.node = node_label(&row.id, nodes);
            }
        }
        self
    }
}

pub fn reply_note(row_id: &str, nodes: &Nodes) -> Option<String> {
    node_label(row_id, nodes)?.reply_note()
}

pub fn reply_control(row_id: &str, nodes: &Nodes) -> Option<Control> {
    reply_note(row_id, nodes).map(|_| Control {
        enabled: false,
        tooltip: None,
    })
}
