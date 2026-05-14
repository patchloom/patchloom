use std::process::ExitCode;

fn main() -> ExitCode {
    match patchloom::run() {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("patchloom: {e}");
            ExitCode::from(1)
        }
    }
}
