use super::id::NodeId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invite {
    pub node: NodeId,
    pub name: String,
    pub can_be_master: bool,
    pub acting: bool,
}
