//! shem: a deterministic PDF rasterizer (docs/adr/0007).

fn main() {
    let args: Option<Vec<String>> = std::env::args_os()
        .skip(1)
        .map(|a| a.into_string().ok())
        .collect();
    let code = match args {
        Some(args) => shem::cli::run(&args, &mut std::io::stdout(), &mut std::io::stderr()),
        None => {
            eprintln!("shem: an argument is not valid Unicode");
            64
        }
    };
    std::process::exit(code);
}
