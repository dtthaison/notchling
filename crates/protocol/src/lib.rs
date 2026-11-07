//! The bridge protocol types (`agentconsole.v1`). Another task fills this in.

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
