//! The `zeo` binary. Everything it does is `zeo::cli`, so the api suite can
//! drive the same verbs in process.

fn main() -> std::process::ExitCode {
    zeo::cli::main()
}
