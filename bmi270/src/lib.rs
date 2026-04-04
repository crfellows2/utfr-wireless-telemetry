#![feature(generic_arg_infer)]

pub mod bmi270;
pub use bmi270::*;
mod bmi_conf;
pub mod interface;
pub mod registers;
pub mod types;
pub mod units;
