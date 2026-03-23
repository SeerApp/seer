use std::{
    fs::{create_dir_all, File},
    io::{Read, Write},
    path::PathBuf,
};

use chrono::Local;

use crate::{
    get_cwd,
    meta::TxMetadata,
    seer_debug,
    tree::nodes::{RootViewChildren, TreeRoot},
};

pub fn save_meta(signature: &String, meta: &TxMetadata) {
    let filename = format!("{}_meta.json", signature);

    let output_path = get_output_path(&filename);
    if !output_path.exists() {
        seer_debug!("Creating meta file: {}", output_path.to_string_lossy());
        let json = serde_json::to_string_pretty(&meta).unwrap();
        let mut file = File::create(output_path).expect("Failed to write file");
        file.write_all(json.as_bytes()).ok().unwrap();
    }
}

pub fn save_trace_tree(
    signature: &String,
    instruction: u8,
    trace_tree: TreeRoot<RootViewChildren>,
) {
    let filename = format!("{}_{}.json", signature, instruction);

    let output_path = get_output_path(&filename);
    if !output_path.exists() {
        seer_debug!("Creating new file: {}", output_path.to_string_lossy());
        let json = serde_json::to_string_pretty(&trace_tree).unwrap();
        let mut file = File::create(output_path).expect("Failed to write file");
        file.write_all(json.as_bytes()).ok().unwrap();
    }
}

pub fn load_trace_tree(
    folder: &PathBuf,
    instruction: u8,
    signature: &String,
) -> TreeRoot<RootViewChildren> {
    let filename = format!("{}_{}.json", signature, instruction);

    let mut input_path = folder.clone();
    input_path.push(filename);
    let mut file = File::open(input_path).expect("Failed to open file");

    let mut json = String::new();
    file.read_to_string(&mut json).expect("Failed to read file");

    serde_json::from_str(&json).expect("Failed to deserialize tree")
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

    if !output_path.exists() {
        seer_debug!("Creating new file: {}", output_path.to_string_lossy());
        let mut file = File::create(output_path).expect("Failed to write file");
        file.write_all(data.as_bytes()).ok().unwrap();
    }
}

fn add_timestamp(filename: String) -> String {
    let timestamp = Local::now().format("%Y%m%d_%H%M%S");

    format! {"{}_{}", filename, timestamp}
}
