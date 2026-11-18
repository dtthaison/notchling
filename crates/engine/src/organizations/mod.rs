pub mod backup;
mod book;
pub mod folders;
pub mod kinds;
mod migrate;
mod record;
mod view;

pub use book::{EditError, Organizations};
pub use folders::FolderMatch;
pub use kinds::{Kind, FIELD, KINDS};
pub use migrate::{MigrateError, Report};
pub use record::{
    normalize_folder, Counts, Document, MachineSettings, Organization, Problem, Shadow,
    DEFAULT_COLOR, DOC, NAME_MAX, PERSONAL_ID, PERSONAL_NAME, VERSION,
};
pub use view::{Gap, Resolved, Source, State, View};

#[cfg(test)]
mod tests_closed;
#[cfg(test)]
mod tests_folders;
#[cfg(test)]
mod tests_migrate;
#[cfg(test)]
mod tests_personal;
#[cfg(test)]
mod tests_record;
#[cfg(test)]
mod tests_review;
#[cfg(test)]
mod tests_shadow;
#[cfg(test)]
mod tests_support;
