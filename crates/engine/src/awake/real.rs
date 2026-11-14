use super::decide::Kind;
use super::hold::Power;
use notchling_platform::awake::AwakeGuard;

pub const REASON: &str = "Notchling: keeping the Mac awake for running agents";

pub struct RealPower;

impl Power for RealPower {
    fn hold(&mut self, kind: Kind) -> Result<Box<dyn Send>, String> {
        let guard =
            AwakeGuard::acquire(REASON, kind == Kind::Display).map_err(|e| e.to_string())?;
        Ok(Box::new(guard))
    }
}
