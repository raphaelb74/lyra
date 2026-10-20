use std::process::ExitCode;

fn main() -> ExitCode {
    match lyra_desktop_lib::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Lyra failed to start: {error}");
            ExitCode::FAILURE
        }
    }
}
