pub mod generated;

pub const PROTOCOL: &str = "agentconsole.v1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_name() {
        assert_eq!(PROTOCOL, "agentconsole.v1");
    }
}
