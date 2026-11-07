//! The bridge protocol types (`agentconsole.v1`): the wire types in [`generated`] come from
//! `protocol/agentconsole.v1.schema.json` by `tools/protocol/gen.py`.

pub mod generated;

/// The protocol version the Mac and the phone speak.
pub const PROTOCOL: &str = "agentconsole.v1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_name() {
        assert_eq!(PROTOCOL, "agentconsole.v1");
    }
}
