use std::path::PathBuf;

use shinki_emu_metadata::metadata_json_schemas;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let (Some(output), None) = (args.next(), args.next()) else {
        return Err("usage: export_schemas <output-directory>".into());
    };
    let output = PathBuf::from(output);
    std::fs::create_dir_all(&output)?;
    for (name, schema) in metadata_json_schemas() {
        let mut text = serde_json::to_string_pretty(&schema)?;
        text.push('\n');
        std::fs::write(output.join(name), text)?;
    }
    Ok(())
}
