pub mod accounts;
pub mod bridge;
pub mod catalog;
pub mod control;
pub mod cost;
pub mod delegate;
pub mod docfile;
pub mod inbox;
pub mod install;
pub mod lanes;
pub mod limits;
pub mod machines;
pub mod pages;
pub mod providers;
pub mod reply;
pub mod search;
pub mod sessions;
pub mod settings;
pub mod store;
pub mod terminal;
pub mod tools;
pub mod transcript;
pub mod vault;
pub mod viz;

pub fn crate_name() -> &'static str {
    "engine"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_itself() {
        assert_eq!(crate_name(), "engine");
    }
}
