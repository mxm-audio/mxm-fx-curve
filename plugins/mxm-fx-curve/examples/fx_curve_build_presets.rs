//! Regenerate the sixty factory curve-stack presets from `src/preset_designs.rs`.

use std::fs;
use std::path::PathBuf;

fn main() {
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("presets");
    fs::create_dir_all(&output).expect("create presets directory");
    for entry in fs::read_dir(&output).expect("read presets directory") {
        let path = entry.expect("preset entry").path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            fs::remove_file(path).expect("remove old generated preset");
        }
    }
    let generated = mxm_fx_curve::preset_designs::generate();
    for (slug, json) in &generated {
        fs::write(output.join(format!("{slug}.json")), json).expect("write generated preset");
    }
    println!("wrote {} factory presets", generated.len());
}
