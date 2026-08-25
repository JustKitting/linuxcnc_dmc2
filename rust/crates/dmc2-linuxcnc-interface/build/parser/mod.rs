mod c_syntax;
mod interpreter_errors;

pub(crate) use c_syntax::{integer_macro, macro_names, named_enum, typedef_enum};
pub(crate) use interpreter_errors::interpreter_error_templates;
