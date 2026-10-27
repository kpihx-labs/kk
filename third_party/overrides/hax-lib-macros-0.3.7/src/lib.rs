// Proc-macros must "reside in the root of the crate": whence the use
// of `std::include!` instead of proper module declaration.

#![cfg_attr(hax, feature(macro_metavar_expr_concat))]

#[cfg(hax)]
std::include!("implementation.rs");

#[cfg(not(hax))]
std::include!("dummy.rs");
