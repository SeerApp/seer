pub mod collect_inputs;
pub mod dwarf;
pub mod logger;

use gimli::Reader;
use seer_interface::GuestMemory;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use solana_signature::Signature;
use std::collections::HashMap;
use std::fmt;
use std::fs::{create_dir_all, File};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::{collections::VecDeque, env, path::PathBuf};

use crate::collect_inputs::collect_inputs;
use crate::dwarf::types::account_info::AccountInfoRepr;
use crate::dwarf::types::guest_fetch::GuestFetch;
use crate::dwarf::{source_location, DwarfParser, DwarfProgram, VariableInterval};
use crate::logger::{SeerLogger, SeerLoggerLevel, seer_logger, init_seer_logger};

pub struct SeerHook {
    active: bool,
    current_tx: Option<Signature>,
    completed_txns: Vec<Signature>,
    current_instruction: u8,
    program_trace: Vec<Pubkey>,
    depth: u8,
    steps: Vec<u64>,
    steps_lines: HashMap<u64, TraceStep>,
    steps_logs: Vec<(u64, String)>,
    state: HashMap<String, Value>,
    parser: Option<DwarfParser>,
    unknown_programs: Vec<Pubkey>,
}

#[derive(Clone)]
struct InstructionTrace {
    instruction: u64,
    trace: Vec<TraceStep>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct TraceStep {
    file: PathBuf,
    line: u64,
    call: bool,
    function: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TraceNode {
    instruction: u64,
    step: TraceStep,
    children: Vec<TraceNode>,
}

impl TraceNode {
    fn find_or_create_child<'a>(
        children: &'a mut Vec<TraceNode>,
        step: &TraceStep,
        instruction: u64,
    ) -> &'a mut TraceNode {
        if let Some(pos) = children.iter().position(|c| c.step == *step) {
            return &mut children[pos];
        }

        children.push(TraceNode {
            step: step.clone(),
            instruction,
            children: vec![],
        });

        let len = children.len();
        &mut children[len - 1]
    }

    pub fn _clean_trace_nodes(project_root: &str, trace_nodes: &mut Vec<TraceNode>) {
        let root = Path::new(project_root);

        for node in trace_nodes.iter_mut() {
            if let Ok(stripped) = node.step.file.strip_prefix(root) {
                node.step.file = stripped.to_path_buf();
            }

            Self::_clean_trace_nodes(project_root, &mut node.children);
        }
    }
}

impl fmt::Debug for SeerHook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SeerHook")
            .field("program_trace", &self.program_trace)
            .field("current_instruction", &self.current_instruction)
            .field("depth", &self.depth)
            .field("steps", &self.steps)
            .field("parser", &self.parser.as_ref().map(|_| "<parser>"))
            .finish()
    }
}

pub fn find_cu_for_pc<R: Reader>(
    dwarf: &gimli::Dwarf<R>,
    pc: u64,
) -> anyhow::Result<Option<gimli::Unit<R>>> {
    let mut hdrs = dwarf.debug_aranges.headers();
    while let Some(h) = hdrs.next()? {
        let mut ents = h.entries();
        while let Some(e) = ents.next()? {
            let r = e.range();
            if pc >= r.begin && pc <= r.end {
                let dio = h.debug_info_offset();
                let uheader = dwarf.debug_info.header_from_offset(dio)?;
                return Ok(dwarf.unit(uheader).map(Some)?);
            }
        }
    }

    let mut units = dwarf.units();
    while let Some(uheader) = units.next()? {
        let unit = dwarf.unit(uheader)?;
        let mut ranges = dwarf.unit_ranges(&unit)?;
        while let Some(r) = ranges.next()? {
            if pc >= r.begin && pc <= r.end {
                return Ok(Some(unit));
            }
        }
    }

    Ok(None)
}

impl SeerHook {
    pub fn new(project_root: PathBuf, dwarf_sources: HashMap<Pubkey, PathBuf>) -> Self {
        let parser = Some(DwarfParser::new(
            project_root.to_string_lossy().to_string(),
            dwarf_sources,
        ));

        Self {
            active: false,
            current_tx: None,
            completed_txns: Vec::new(),
            current_instruction: 0,
            program_trace: Vec::new(),
            depth: 0,
            steps: Vec::new(),
            steps_lines: HashMap::new(),
            steps_logs: Vec::new(),
            state: HashMap::new(),
            parser: parser,
            unknown_programs: Vec::new(),
        }
    }

    fn build_trace_tree(
        &self,
        sequential_instruction_traces: Vec<InstructionTrace>,
    ) -> Vec<TraceNode> {
        let mut roots: Vec<TraceNode> = vec![];

        for instr_trace in sequential_instruction_traces {
            let mut current_level = &mut roots;

            for step in &instr_trace.trace {
                let node =
                    TraceNode::find_or_create_child(current_level, step, instr_trace.instruction);
                current_level = &mut node.children;
            }
        }

        roots
    }

    fn push_to_last_leaf(&self, mut roots: Vec<TraceNode>, new_node: TraceNode) -> Vec<TraceNode> {
        fn get_last_mut(node: &mut TraceNode) -> &mut TraceNode {
            if !node.children.is_empty() {
                let last_index = node.children.len() - 1;
                let last_child = &mut node.children[last_index];
                get_last_mut(last_child)
            } else {
                node
            }
        }

        if let Some(last_root) = roots.last_mut() {
            let deepest = get_last_mut(last_root);
            if deepest.step.line > 0 {
                deepest.children.push(new_node);
            } else {
                roots.push(new_node);
            }
        } else {
            roots.push(new_node);
        }

        roots
    }

    fn save_trace_to_json(&self, trace_nodes: &Vec<TraceNode>, path: &str) -> std::io::Result<()> {
        let json: String = serde_json::to_string_pretty(trace_nodes).unwrap();
        let mut file = File::create(path)?;
        file.write_all(json.as_bytes())?;
        Ok(())
    }

    fn save_state_to_json(&self, path: &str) -> std::io::Result<()> {
        let json: String = serde_json::to_string_pretty(&self.state).unwrap();
        let mut file = File::create(path)?;
        file.write_all(json.as_bytes())?;
        Ok(())
    }

    fn get_output_path(&self, project_root: &str, filename: &str) -> PathBuf {
        let mut path = PathBuf::from(project_root);
        path.push("seer");
        create_dir_all(&path).unwrap();
        path.push(filename);
        path
    }

    fn get_current_parser<'a>(
        dwarf_parser: &'a DwarfParser,
        current_program: &Pubkey,
    ) -> (&'a String, &'a DwarfProgram) {
        (
            &dwarf_parser.project_root,
            dwarf_parser
                .dwarf_programs
                .get(current_program)
                .expect("Current program not in dwarf_programs!"),
        )
    }

    fn interpolate_logs(&self, mut trace: Vec<TraceNode>) -> Vec<TraceNode> {
        fn find_best_path(
            nodes: &Vec<TraceNode>,
            instr: u64,
            depth: usize,
            path_prefix: Vec<usize>,
        ) -> Option<(Vec<usize>, i64, usize)> {
            let mut best: Option<(Vec<usize>, i64, usize)> = None;

            for (i, node) in nodes.iter().enumerate() {
                let node_instr = node.instruction;

                if node_instr == instr {
                    let mut p = path_prefix.clone();
                    p.push(i);
                    return Some((p, 0, depth));
                } else if node_instr == instr + 8 {
                    let mut p = path_prefix.clone();
                    p.push(i);
                    return Some((p, 1, depth));
                } else if node_instr <= instr {
                    let diff = (instr as i64 - node_instr as i64).abs();
                    let replace = match best {
                        None => true,
                        Some((_, best_diff, best_depth)) => {
                            diff < best_diff || (diff == best_diff && depth > best_depth)
                        }
                    };
                    if replace {
                        let mut p = path_prefix.clone();
                        p.push(i);
                        best = Some((p, diff, depth));
                    }
                }

                let mut child_path = path_prefix.clone();
                child_path.push(i);
                if let Some((child_best_path, child_diff, child_depth)) =
                    find_best_path(&node.children, instr, depth + 1, child_path)
                {
                    match &best {
                        None => best = Some((child_best_path, child_diff, child_depth)),
                        Some((_, best_diff, best_depth)) => {
                            if child_diff < *best_diff
                                || (child_diff == *best_diff && child_depth > *best_depth)
                            {
                                best = Some((child_best_path, child_diff, child_depth));
                            }
                        }
                    }
                }
            }

            best
        }

        for (instr, log_str) in &self.steps_logs {
            if let Some((path, _, _)) = find_best_path(&trace, *instr, 0, vec![]) {
                // Traverse down the path
                let mut current: &mut Vec<TraceNode> = &mut trace;
                for (depth, &idx) in path.iter().enumerate() {
                    if depth == path.len() - 1 {
                        if let Some(target_node) = current.get(idx) {
                            if target_node.step.line == 0 {
                                current.push(TraceNode {
                                    instruction: *instr,
                                    step: TraceStep {
                                        file: PathBuf::new(),
                                        line: 0,
                                        call: false,
                                        function: Some(log_str.clone()),
                                    },
                                    children: vec![],
                                });
                                continue;
                            }
                        }

                        let target_children = &mut current[idx].children;
                        target_children.push(TraceNode {
                            instruction: *instr,
                            step: TraceStep {
                                file: PathBuf::new(),
                                line: 0,
                                call: false,
                                function: Some(log_str.clone()),
                            },
                            children: vec![],
                        });
                    } else {
                        current = &mut current[idx].children;
                    }
                }
            }
        }

        trace
    }

    fn wrap_steps(&mut self, err: Option<InstructionError>) {
        if self.steps.len() > 0 {
            let current_program = self.program_trace.last().unwrap();
            let (project_root, current_dwarf_program) =
                SeerHook::get_current_parser(&self.parser.as_ref().unwrap(), current_program);
            let mut sequential_instruction_traces = Vec::new();

            for i in &self.steps {
                if let Some(interval) = current_dwarf_program.interval_tree.search_deepest(i) {
                    let mut ordered_instruction_trace: VecDeque<TraceStep> = VecDeque::new();
                    let mut tracing = true;
                    let mut current_offset = interval.die_offset;

                    while tracing {
                        let trace_die_node = current_dwarf_program
                            .significant_instruction_map
                            .get(&current_offset)
                            .unwrap();

                        ordered_instruction_trace.push_front(TraceStep {
                            file: trace_die_node.decl_mapping.file.clone(),
                            line: trace_die_node.decl_mapping.line,
                            call: false,
                            function: Some(trace_die_node.function_signature.clone()),
                        });

                        if let Some(call_mapping) = trace_die_node.call_mapping.clone() {
                            ordered_instruction_trace.push_front(TraceStep {
                                file: call_mapping.file,
                                line: call_mapping.line,
                                call: true,
                                function: Some(trace_die_node.function_signature.clone()),
                            });
                        }

                        if trace_die_node.parent_offset == current_offset {
                            let uheader = current_dwarf_program
                                .root_instruction_unit
                                .get(&current_offset)
                                .expect("CU header not found for root offset!");

                            let dwarf = current_dwarf_program.owned_dwarf.dwarf();
                            let unit = dwarf
                                .unit(*uheader)
                                .expect("Did not find CU for CU header!");

                            let (loc_file, loc_line) =
                                match source_location(&dwarf, &unit, *i, project_root) {
                                    Ok(v) => v,
                                    Err(_) => (None, None),
                                };

                            if let Some(file) = loc_file {
                                if let Some(line) = loc_line {
                                    let last_trace_step = ordered_instruction_trace.back();
                                    if let Some(lti) = last_trace_step {
                                        if lti.file != file || lti.line != line {
                                            ordered_instruction_trace.push_back(TraceStep {
                                                file: file,
                                                line: line,
                                                call: false,
                                                function: None,
                                            });
                                        }
                                    }
                                }
                            }

                            tracing = false;
                        } else {
                            current_offset = trace_die_node.parent_offset;
                        }
                    }

                    loop {
                        let first_instruction_trace = ordered_instruction_trace.front();
                        if let Some(first) = first_instruction_trace {
                            if !first
                                .file
                                .to_string_lossy()
                                .to_string()
                                .contains(project_root)
                            {
                                ordered_instruction_trace.pop_front();
                                continue;
                            }
                        }
                        break;
                    }

                    sequential_instruction_traces.push(InstructionTrace {
                        instruction: *i,
                        trace: Vec::from(ordered_instruction_trace),
                    });
                }

                if self.steps_lines.contains_key(i) {
                    if !sequential_instruction_traces.is_empty() {
                        let trace_step = self.steps_lines.get(i).unwrap().clone();

                        let mut ordered_instruction_trace =
                            sequential_instruction_traces.last().unwrap().trace.clone();
                        let mut j = ordered_instruction_trace.len();

                        while j > 0 {
                            j -= 1;

                            let current_trace_step = &ordered_instruction_trace[j];

                            if current_trace_step.function.is_some()
                                && current_trace_step.call == false
                                && current_trace_step.file == trace_step.file
                            {
                                ordered_instruction_trace.push(trace_step);
                                break;
                            } else {
                                ordered_instruction_trace.pop();
                            }

                            if ordered_instruction_trace.is_empty() {
                                break;
                            }
                        }

                        if !ordered_instruction_trace.is_empty() {
                            sequential_instruction_traces.push(InstructionTrace {
                                instruction: *i,
                                trace: ordered_instruction_trace,
                            })
                        }
                    }
                }
            }

            let mut trace_tree: Vec<TraceNode> =
                self.build_trace_tree(sequential_instruction_traces);
            trace_tree = self.interpolate_logs(trace_tree);

            // TraceNode::clean_trace_nodes(project_root, &mut trace_tree);

            let cwd = env::current_dir()
                .expect("Failed to get current dir!")
                .into_os_string()
                .into_string()
                .expect("Path not valid UTF");

            if let Some(error) = err {
                let error_node = TraceNode {
                    instruction: *self.steps.last().unwrap(),
                    step: TraceStep {
                        file: PathBuf::new(),
                        line: 0,
                        call: true,
                        function: Some(error.to_string()),
                    },
                    children: Vec::new(),
                };
                trace_tree = self.push_to_last_leaf(trace_tree, error_node);

                let filename = format!(
                    "{}_{}_{}_error.json",
                    self.current_tx.unwrap().to_string(),
                    self.current_instruction,
                    current_program.to_string(),
                );

                let output_path = self.get_output_path(&cwd, &filename);

                let _ = self.save_state_to_json(output_path.to_str().unwrap());
            }

            let filename = format!(
                "{}_{}_{}_{}.json",
                self.current_tx.unwrap().to_string(),
                self.current_instruction,
                current_program.to_string(),
                self.depth,
            );

            let output_path = self.get_output_path(&cwd, &filename);

            let _ = self.save_trace_to_json(&trace_tree, output_path.to_str().unwrap());
        }
    }

    pub fn activated(&self) -> bool {
        self.active
    }

    pub fn activate(&mut self) {
        self.active = true;
        seer_trace!("Activated");
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        seer_trace!("Deactivated");
    }

    pub fn set_current_tx(&mut self, tx: Signature) {
        if self.active {
            seer_trace!("New tx: {:?}", tx);
            if self.completed_txns.contains(&tx) {
                seer_debug!("Tx already complete");
                self.deactivate();
            } else {
                self.current_tx = Some(tx);
            }
        }
    }

    pub fn unset_current_tx(&mut self) {
        if self.active {
            seer_trace!("Tx unset: {:?}", self.current_tx);
            self.completed_txns.push(self.current_tx.unwrap().clone());
            self.current_tx = None;
        }
    }

    pub fn start_instruction(&mut self, instruction: u8) {
        if self.active {
            seer_trace!("New instruction: {:?}", instruction);
            self.current_tx
                .is_none()
                .then(|| panic!("current_tx is not defined by start_instruction call!"));
            self.current_instruction = instruction;
        }
    }

    pub fn end_instruction(&mut self) {
        if self.active {
            seer_trace!("Instruction unset: {:?}", self.current_instruction);
            self.current_instruction = 0;
        }
    }

    pub fn start_program(&mut self, program: Pubkey) {
        if self.active {
            if self
                .parser
                .as_ref()
                .expect("Parser not set at start_program call!")
                .dwarf_programs
                .contains_key(&program)
            {
                seer_trace!("Starting program: {:?}", program);
                self.current_tx
                    .is_none()
                    .then(|| panic!("current_tx is not defined by start_program call!"));

                if !self.program_trace.is_empty() {
                    self.wrap_steps(None);
                    self.steps = Vec::new();
                    self.steps_logs = Vec::new();
                    self.depth += 1;
                }
            } else {
                self.unknown_programs.push(program.clone());
            }
            self.program_trace.push(program);
        }
    }

    pub fn end_program(&mut self, program: Pubkey, err: Option<InstructionError>) {
        if self.active {
            if !self.unknown_programs.contains(&program) {
                seer_trace!("Ending program: {:?}", program);

                self.current_tx
                    .is_none()
                    .then(|| panic!("current_tx is not defined by end_program call!"));
                self.program_trace
                    .is_empty()
                    .then(|| panic!("program_trace empty by end_program call!"));

                self.wrap_steps(err);
                self.steps = Vec::new();
                self.steps_logs = Vec::new();
                self.depth += 1;
            }
            self.program_trace.pop();
        }
    }

    pub fn step<M: GuestMemory>(&mut self, pc: &u64, mem: &mut M, reg: &[u64; 12]) {
        if self.active {
            if let Some(current_program) = self.program_trace.last() {
                if !self.unknown_programs.contains(&current_program) {
                    self.current_tx
                        .is_none()
                        .then(|| panic!("current_tx is not defined by step call!"));

                    let current_program = self.program_trace.last().unwrap();
                    let (project_root, current_dwarf_program) = SeerHook::get_current_parser(
                        &self.parser.as_ref().unwrap(),
                        current_program,
                    );

                    let pc_lookup = pc.clone();

                    self.state.extend(self.parse_local_variables(
                        &pc_lookup,
                        current_dwarf_program,
                        mem,
                        reg,
                    ));

                    if current_dwarf_program
                        .interval_tree
                        .search_first(&pc_lookup)
                        .is_some()
                        || self.steps_lines.contains_key(&pc_lookup)
                    {
                        self.steps.push(pc_lookup);
                    } else {
                        let dwarf = current_dwarf_program.owned_dwarf.dwarf();
                        if let Some(unit) = find_cu_for_pc(&dwarf, pc_lookup).unwrap() {
                            let loc =
                                source_location(&dwarf, &unit, pc_lookup, project_root).unwrap();
                            if let Some(best_file) = loc.0 {
                                if let Some(best_line) = loc.1 {
                                    if best_file.starts_with(project_root) {
                                        self.steps.push(pc_lookup);
                                        self.steps_lines.insert(
                                            pc_lookup,
                                            TraceStep {
                                                file: best_file,
                                                line: best_line,
                                                call: false,
                                                function: None,
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                panic!("program_trace empty by step call!");
            }
        }
    }

    // Illustration of a specific, ungeneralised case of parsing a complex structure
    fn parse_local_variables<M: GuestMemory>(
        &self,
        pc_lookup: &u64,
        dwarf_program: &DwarfProgram,
        mem: &mut M,
        reg: &[u64; 12],
    ) -> HashMap<String, Value> {
        let mut step_variables: HashMap<String, Value> = HashMap::new();

        if let Some(variable_interval_tree) = &dwarf_program.maybe_variable_interval_tree {
            let mut results: Vec<&VariableInterval> = vec![];
            variable_interval_tree.search(pc_lookup, &mut results);
            if results.len() > 0 {
                for result in results {
                    if result.type_signature == "&solana_account_info::AccountInfo" {
                        let account_flat =
                            AccountInfoRepr::fetch(mem, reg[result.register as usize]);
                        step_variables.insert(
                            result.name.clone(),
                            serde_json::to_value(account_flat).unwrap(),
                        );
                    }
                }
            }
        }

        return step_variables;
    }

    pub fn log(&mut self, message: &str) {
        let prev_step = self.steps.last();
        if let Some(ps) = prev_step {
            self.steps_logs.push((*ps, message.to_string()));
        }
    }
}

static SEER: OnceLock<Mutex<SeerHook>> = OnceLock::new();

/// `maybe_source_project_root` is the root of the native Solana/Anchor project with files to which we will map.
/// `maybe_deploy_folder_root` is the root of the built programs, debug data, and addresses.
/// This function must be called exactly once in the lifetime of a program, such as at the start of
/// the individual user's Seer RPC.
pub fn init(maybe_source_project_root: Option<PathBuf>, maybe_deploy_folder_root: Option<PathBuf>) {
    let (source_project_root, dwarf_sources) =
        collect_inputs(maybe_source_project_root, maybe_deploy_folder_root);

    init_seer_logger(SeerLogger::from_env());

    SEER.set(Mutex::new(SeerHook::new(
        source_project_root,
        dwarf_sources,
    )))
    .expect("Failed to init SeerHook!");
}

pub fn get<'a>() -> std::sync::MutexGuard<'static, SeerHook> {
    let seer: &Mutex<SeerHook> = SEER.get().expect("SEER not initialized!");
    if seer.lock().unwrap().parser.is_none() {
        panic!("Tried accessing SEER singleton before initializing dwarf sources!");
    }
    seer.lock().expect("SeerHook poisoned!")
}
