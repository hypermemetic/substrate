mod executor;
mod activation;
mod types;

pub use executor::BashExecutor;
pub use activation::Bash;
pub use types::{BashError, BashOutput};
