use clap::Parser;

#[derive(Parser, Debug)]
pub(crate) struct BatchArgs {
    #[arg(
        value_name = "JSON",
        allow_hyphen_values = true,
        help = "JSON array of {command, args} objects",
        required_unless_present = "stdin",
        conflicts_with = "stdin"
    )]
    pub commands_json: Option<String>,
    #[arg(
        long,
        help = "Read up to 1 MiB of UTF-8 batch JSON from a pipe or file"
    )]
    pub stdin: bool,
    #[arg(long, help = "Halt the batch on the first failed command")]
    pub stop_on_error: bool,
    #[arg(
        long,
        default_value = "60000",
        help = "Absolute wall-clock budget for the whole non-atomic batch"
    )]
    pub timeout_ms: u64,
}
