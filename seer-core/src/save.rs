use std::{
    fs::{create_dir_all, File},
    io::{Read, Write},
    path::PathBuf,
};

use chrono::Local;

use crate::{
    get_cwd,
    meta::TxMetadata,
    register_trace::RegisterTraceChunk,
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

pub fn save_register_trace_chunk(
    signature: &str,
    instruction: u8,
    tree_uid: u64,
    min_order: u64,
    max_order: u64,
    trace_chunk: &RegisterTraceChunk,
) {
    let filename = format!(
        "{}_{}_{}_{}_{}.json",
        signature, instruction, min_order, max_order, tree_uid
    );

    save_json_file(get_output_path(&filename), trace_chunk, false);
}

/// Writes `<folder>/<signature>_<instruction>.json`, creating `folder` as needed (overwrites).
pub fn save_trace_tree_to_dir(
    folder: &PathBuf,
    signature: &String,
    instruction: u8,
    trace_tree: TreeRoot<RootViewChildren>,
) {
    let filename = format!("{}_{}.json", signature, instruction);
    let mut output_path = folder.clone();
    output_path.push(filename);
    create_dir_all(folder).expect("create trace tree output dir");
    let json = serde_json::to_string_pretty(&trace_tree).expect("serialize trace tree");
    let mut file = File::create(output_path).expect("Failed to write trace tree file");
    file.write_all(json.as_bytes()).expect("write trace tree bytes");
}

pub fn save_register_trace_chunk_to_dir(
    folder: &PathBuf,
    signature: &str,
    instruction: u8,
    tree_uid: u64,
    min_order: u64,
    max_order: u64,
    trace_chunk: &RegisterTraceChunk,
) {
    let filename = format!(
        "{}_{}_{}_{}_{}.json",
        signature, instruction, min_order, max_order, tree_uid
    );
    let mut output_path = folder.clone();
    output_path.push(filename);
    create_dir_all(folder).expect("create register trace output dir");
    save_json_file(output_path, trace_chunk, true);
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

fn save_json_file<T: serde::Serialize>(output_path: PathBuf, value: &T, overwrite: bool) {
    if overwrite || !output_path.exists() {
        seer_debug!("Creating new file: {}", output_path.to_string_lossy());
        let json = serde_json::to_string_pretty(value).expect("serialize json file");
        let mut file = File::create(output_path).expect("Failed to write file");
        file.write_all(json.as_bytes()).ok().unwrap();
    }
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

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{
        init_seer_logger,
        register_trace::{RegisterSnapshot, RegisterTraceChunk},
        save::save_register_trace_chunk_to_dir,
        SeerLogger,
    };

    fn temp_output_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("seer-register-trace-{unique}"));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    fn init_test_logger() {
        init_seer_logger(SeerLogger::from_env());
    }

    #[test]
    fn saves_register_trace_with_order_range_filename() {
        init_test_logger();
        let folder = temp_output_dir();
        let mut reg = BTreeMap::new();
        reg.insert(0, 7);
        reg.insert(2, 9);
        let chunk = RegisterTraceChunk {
            snapshot: RegisterSnapshot { reg },
            trace: vec![],
        };

        save_register_trace_chunk_to_dir(&folder, "sig", 3, 7, 10, 42, &chunk);

        let output_path = folder.join("sig_3_10_42_7.json");
        assert!(output_path.is_file());

        let content = fs::read_to_string(&output_path).expect("read register trace");
        assert!(content.contains("\"snapshot\""));
        assert!(content.contains("\"trace\""));
        assert!(content.contains("\"0\": 7"));
        assert!(!content.contains("\"11\""));

        fs::remove_dir_all(folder).expect("remove temp dir");
    }
}
