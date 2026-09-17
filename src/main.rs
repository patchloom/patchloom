use std::process::ExitCode;

fn main() -> ExitCode {
    match patchloom::run() {
        Ok(code) => ExitCode::from(code),
        Err(e) => ExitCode::from(patchloom::report_dispatch_error(&e)),
    }
}
