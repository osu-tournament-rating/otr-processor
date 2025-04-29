use clap::Parser;

#[derive(Parser, Clone)]
#[command(
    display_name = "o!TR Processor",
    author = "osu! Tournament Rating",
    long_about = "Generates ratings for the osu! Tournament Rating platform"
)]
pub struct Args {
    /// Connection string should be formatted like so: postgresql://USER:PASSWORD@HOST:PORT/DATABASE
    /// Example: postgresql://postgres:password@localhost:5432/postgres
    ///
    /// This is true regardless of whether the database is running via docker. If
    /// running postgres through docker, mapping the ports
    #[arg(
        short,
        long,
        env,
        help = "Database connection string",
        long_help = "If running via docker, the connection string should be formatted like so: \
        postgresql://USER:PASSWORD@HOST:PORT/DATABASE"
    )]
    pub connection_string: String,

    /// Ignores database constraints when processing.
    /// Allows those without access to the users table
    /// to modify the tournaments table
    #[arg(short, long, action = clap::ArgAction::SetTrue)]
    pub ignore_constraints: bool,

    /// Log level (trace, debug, info, warn, error)
    #[arg(
        short,
        long,
        env = "RUST_LOG", 
        default_value = "info", 
        value_parser = ["trace", "debug", "info", "warn", "error"],
        help = "Sets the logging verbosity"
    )]
    pub log_level: String
}
