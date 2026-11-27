pub mod account;
pub mod clock;
pub mod content;
pub mod hook;
mod library;
mod listpage;
mod lock;
pub mod owner;
mod record;
pub mod skill;

pub use library::{find_in, safe_component, safe_relative, PageLibrary, PagesError};
pub use listpage::html as list_page_html;
pub use record::{
    flatten, join_normalized, read_bounded, RecordContext, MAX_FILE_BYTES, MAX_TOTAL_BYTES,
};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_account;
#[cfg(test)]
mod tests_hook;
#[cfg(test)]
mod tests_owner;
