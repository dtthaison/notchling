pub mod backup;
mod book;
pub mod folders;
pub mod kinds;
mod migrate;
mod preview;
mod record;
mod view;

pub use book::{EditError, Organizations};
pub use folders::FolderMatch;
pub use kinds::{Kind, FIELD, KINDS};
pub use migrate::{MigrateError, Report};
pub use preview::{FolderPreview, Moved, Overlap, Relation};
pub use record::{
    known_color, normalize_folder, Counts, Document, MachineSettings, Organization, Problem,
    Shadow, COLORS, DEFAULT_COLOR, DOC, NAME_MAX, PERSONAL_ID, PERSONAL_NAME, VERSION,
};
pub use view::{Gap, Resolved, Source, State, View};

#[cfg(test)]
mod tests_closed;
#[cfg(test)]
mod tests_colors;
#[cfg(test)]
mod tests_folders;
#[cfg(test)]
mod tests_migrate;
#[cfg(test)]
mod tests_personal;
#[cfg(test)]
mod tests_preview;
#[cfg(test)]
mod tests_record;
#[cfg(test)]
mod tests_review;
#[cfg(test)]
mod tests_shadow;
#[cfg(test)]
mod tests_support;
