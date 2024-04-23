use std::io::Result;
use prost_build::Config;

fn main() -> Result<()> {
    Config::new().out_dir("src").compile_protos(&["src/message.proto"], &["src"])?;
    Ok(())
}