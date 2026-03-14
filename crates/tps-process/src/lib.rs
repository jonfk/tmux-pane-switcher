use anyhow::Result;
use tps_core::{ProcessClass, classify_command};

pub fn classify_foreground_command(command: &str) -> Result<ProcessClass> {
    Ok(classify_command(command))
}
