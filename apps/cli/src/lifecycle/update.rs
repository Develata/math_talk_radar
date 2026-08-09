//! Self-update (§34). Full implementation lands in Todo 2/3.
use crate::runtime::CliError;

pub async fn check() -> Result<String, CliError> {
    Err(CliError::not_implemented("update --check"))
}

pub async fn run() -> Result<String, CliError> {
    Err(CliError::not_implemented("update"))
}
