// src/main.rs
use clap::Parser;


#[derive(Parser, Debug)]
#[command(name = "aex")]
struct Opt {
    #[arg(long, default_value = "aex-server")]
    name: String,

    #[arg(long, default_value = "0.0.0.0")]
    ip: String,

    #[arg(long, default_value_t = 9000)]
    port: u16,

    #[arg(long)]
    data_dir: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Opt::parse();

    Ok(())
}
