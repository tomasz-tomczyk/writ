//! The `writ` binary. Everything it does lives in the library beside it,
//! so the tests and the binary run the same code.

use std::process::ExitCode;

fn main() -> ExitCode {
    writ_cli::run()
}
