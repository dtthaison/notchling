#[macro_use]
mod field_macros;
#[macro_use]
mod kind_macros;

pub mod audit_check;
pub mod audit_line;
pub mod doc;
pub mod emit;
pub mod envelope;
pub mod event;
pub mod field;
pub mod field_ref;
pub mod ids;
pub mod kinds;
pub mod registry;
pub mod salt;
pub mod scope;
pub mod spec;
pub mod value;

pub use emit::*;
pub use envelope::*;
pub use event::*;
pub use field::*;
pub use field_ref::*;
pub use spec::*;
pub use value::Fields;

#[cfg(test)]
mod test_kinds;

#[cfg(test)]
mod audit_check_tests;
#[cfg(test)]
mod audit_line_tests;
#[cfg(test)]
mod emit_tests;
#[cfg(test)]
mod field_tests;
#[cfg(test)]
mod registry_tests;
#[cfg(test)]
mod salt_tests;
#[cfg(test)]
mod scope_tests;
#[cfg(test)]
mod value_tests;
