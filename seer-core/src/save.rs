use std::{
    fs::{create_dir_all, File},
    io::Write,
    path::PathBuf,
};

use chrono::Local;
use solana_signature::Signature;

use crate::{
    get_cwd,
    tree::{view::ViewNode, Tree},
};

pub fn save_trace_tree(signature: &Signature, instruction: u8, trace_tree: Tree<ViewNode>) {
    let filename = format!("{}_{}.json", signature.to_string(), instruction);

    let output_path = get_output_path(&filename);
    let json = serde_json::to_string_pretty(&trace_tree).unwrap();
    let mut file = File::create(output_path).ok().unwrap();
    file.write_all(json.as_bytes()).ok().unwrap();
}

fn get_output_path(filename: &str) -> PathBuf {
    let mut path = PathBuf::from(get_cwd());
    path.push("seer");
    create_dir_all(&path).unwrap();
    path.push(filename);
    path
}

pub fn save(data: String, filename: String, extension: &str, timestamp: bool) {
    let mut final_filename = filename;

    if timestamp {
        final_filename = add_timestamp(final_filename);
    }

    let output_path = get_output_path(&format!("{}.{}", final_filename, extension));
    let mut file = File::create(output_path).ok().unwrap();
    file.write_all(data.as_bytes()).ok().unwrap();
}

fn add_timestamp(filename: String) -> String {
    let timestamp = Local::now().format("%Y%m%d_%H%M%S");

    format! {"{}_{}", filename, timestamp}
}
