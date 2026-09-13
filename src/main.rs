use std::process::ExitCode;
fn main() -> ExitCode {
    let args = std::env::args_os()
        .skip(1)
        .map(|arg| {
            arg.into_string()
                .map_err(|_| watf::Error::message("arguments must be UTF-8"))
        })
        .collect::<watf::Result<Vec<_>>>();
    match args.and_then(watf::cli::run) {
        Ok(code) => ExitCode::from(code as u8),
        Err(error) => {
            eprintln!("watf: {}", watf::text::compact(&error.to_string(), 1000));
            ExitCode::from(2)
        }
    }
}
