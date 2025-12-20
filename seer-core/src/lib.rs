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
use std::cmp::min;
use std::collections::HashMap;
use std::fmt;
use std::fs::{create_dir_all, File};
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::{collections::VecDeque, env, path::PathBuf};

use crate::collect_inputs::collect_inputs;
use crate::dwarf::types::account_info::AccountInfoRepr;
use crate::dwarf::types::guest_fetch::GuestFetch;
use crate::dwarf::{source_location, DwarfParser, DwarfProgram, VariableInterval};
use crate::logger::{init_seer_logger, seer_logger, SeerLogger, SeerLoggerLevel};

pub struct SeerHook {
    active: bool,
    current_tx: Option<Signature>,
    completed_txns: Vec<Signature>,
    current_instruction: u8,
    program_trace: Vec<Pubkey>,
    depth: u8,
    state: HashMap<String, Value>,
    parser: Option<DwarfParser>,
    unknown_programs: Vec<Pubkey>,
    sequential_instruction_traces: Vec<InstructionTrace>,
    #[allow(dead_code)]
    die_instruction_cache: HashMap<(Pubkey, u64), InstructionTrace>,
}

#[derive(Clone, Serialize)]
struct InstructionTrace {
    instruction: u64,
    trace: Vec<TraceStep>,
}

#[derive(Debug, PartialEq)]
enum SetStatus {
    Subset,
    Superset,
    Neither,
}

impl InstructionTrace {
    pub fn get_set_status_of(&self, other_instruction_trace: &InstructionTrace) -> SetStatus {
        let self_len = self.trace.len();
        let other_len = other_instruction_trace.trace.len();

        for i in 0..min(self_len, other_len) {
            let own_trace = self.trace.get(i).unwrap();
            let other_trace = other_instruction_trace.trace.get(i).unwrap();

            if own_trace != other_trace {
                return SetStatus::Neither;
            }
        }

        if self_len < other_len {
            SetStatus::Superset
        } else {
            SetStatus::Subset
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct TraceStep {
    file: PathBuf,
    line: u64,
    call: bool,
    function: Option<String>,
}

impl TraceStep {
    #[cfg(test)]
    pub fn new_for_test(line: u64) -> Self {
        Self {
            file: PathBuf::new(),
            line,
            call: false,
            function: None,
        }
    }

    pub fn from_log(msg: String) -> Self {
        Self {
            file: PathBuf::new(),
            line: 0,
            call: false,
            function: Some(msg),
        }
    }

    pub fn is_log(&self) -> bool {
        self.file == PathBuf::new()
            && self.line == 0
            && self.call == false
            && self.function.is_some()
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct TraceTree {
    instruction: u64,
    step: TraceStep,
    children: Vec<TraceTree>,
}

impl TraceTree {
    fn get_last_mut(&mut self) -> &mut TraceTree {
        if !self.children.is_empty() {
            let last_index = self.children.len() - 1;
            let last_child = &mut self.children[last_index];
            last_child.get_last_mut()
        } else {
            self
        }
    }

    fn get_root_trace_tree() -> Self {
        Self {
            instruction: 0,
            step: TraceStep {
                file: PathBuf::new(),
                line: 0,
                call: false,
                function: None,
            },
            children: vec![],
        }
    }

    fn grow(&mut self, instruction_trace: &InstructionTrace, counter: usize) {
        if counter < instruction_trace.trace.len() {
            let instruction_trace_step = instruction_trace.trace[counter].clone();
            let last_index = if self.children.is_empty() {
                self.children.push(TraceTree {
                    instruction: instruction_trace.instruction,
                    step: instruction_trace_step,
                    children: vec![],
                });

                0
            } else {
                let last_index = self.children.len() - 1;
                let last_child = &self.children[last_index];

                if last_child.step == instruction_trace_step {
                    last_index
                } else {
                    self.children.push(TraceTree {
                        instruction: instruction_trace.instruction,
                        step: instruction_trace_step,
                        children: vec![],
                    });
                    last_index + 1
                }
            };

            let last_child = &mut self.children[last_index];
            last_child.grow(instruction_trace, counter + 1);
        }
    }

    fn unroot(self) -> Vec<TraceTree> {
        let mut trace_tree = vec![];

        for child in self.children {
            trace_tree.push(child);
        }

        trace_tree
    }
}

impl fmt::Debug for SeerHook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SeerHook")
            .field("program_trace", &self.program_trace)
            .field("current_instruction", &self.current_instruction)
            .field("depth", &self.depth)
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
            state: HashMap::new(),
            parser,
            unknown_programs: Vec::new(),
            sequential_instruction_traces: Vec::new(),
            die_instruction_cache: HashMap::new(),
        }
    }

    fn prune_instruction_trace_subsets(instruction_traces: &mut Vec<InstructionTrace>) {
        let mut i = 1;
        while i < instruction_traces.len() {
            let trace_0 = instruction_traces.get(i - 1).unwrap();
            let trace_1 = instruction_traces.get(i).unwrap();

            let set_status = trace_0.get_set_status_of(trace_1);

            match set_status {
                SetStatus::Superset => {
                    instruction_traces.remove(i - 1);
                }
                SetStatus::Subset => {
                    instruction_traces.remove(i);
                }
                SetStatus::Neither => {
                    i += 1;
                }
            }
        }
    }

    fn build_trace_tree(instruction_traces: &mut Vec<InstructionTrace>) -> Vec<TraceTree> {
        SeerHook::prune_instruction_trace_subsets(instruction_traces);

        let mut root_trace_tree = TraceTree::get_root_trace_tree();

        for it in instruction_traces {
            root_trace_tree.grow(it, 0);
        }

        root_trace_tree.unroot()
    }

    fn push_to_last_leaf(&self, mut roots: Vec<TraceTree>, new_node: TraceTree) -> Vec<TraceTree> {
        if let Some(last_root) = roots.last_mut() {
            let deepest = last_root.get_last_mut();
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

    fn save_to_json(&self, json_string: String, path: &str) -> std::io::Result<()> {
        let mut file = File::create(path)?;
        file.write_all(json_string.as_bytes())?;
        Ok(())
    }

    fn save_trace_to_json(&self, trace_nodes: &Vec<TraceTree>, path: &str) -> std::io::Result<()> {
        let json: String = serde_json::to_string_pretty(trace_nodes).unwrap();
        self.save_to_json(json, path)
    }

    fn save_state_to_json(&self, path: &str) -> std::io::Result<()> {
        let json: String = serde_json::to_string_pretty(&self.state).unwrap();
        self.save_to_json(json, path)
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

    fn get_ordered_insutrction_trace(
        &self,
        project_root: &String,
        current_dwarf_program: &DwarfProgram,
        i: u64,
    ) -> InstructionTrace {
        let mut ordered_instruction_trace: VecDeque<TraceStep> = VecDeque::new();

        if let Some(interval) = current_dwarf_program.interval_tree.search_deepest(&i) {
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

                    let (loc_file, loc_line) = match source_location(&dwarf, &unit, i, project_root)
                    {
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
        }

        InstructionTrace {
            instruction: i,
            trace: Vec::from(ordered_instruction_trace),
        }
    }

    /// Create InstructionTrace with exact step at the end of the previous ordered_instruction_trace
    /// if it exists.
    fn get_extra_line_instruction_trace(
        &self,
        project_root: &String,
        current_dwarf_program: &DwarfProgram,
        mut ordered_instruction_trace: Vec<TraceStep>,
        i: &u64,
    ) -> Option<InstructionTrace> {
        let dwarf = current_dwarf_program.owned_dwarf.dwarf();

        let Some(unit) = find_cu_for_pc(&dwarf, *i).unwrap() else {
            return None;
        };

        let (Some(best_file), Some(best_line)) =
            source_location(&dwarf, &unit, *i, project_root).unwrap()
        else {
            return None;
        };

        let trace_step = TraceStep {
            file: best_file,
            line: best_line,
            call: false,
            function: None,
        };

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
            return Some(InstructionTrace {
                instruction: *i,
                trace: ordered_instruction_trace,
            });
        }

        None
    }

    pub fn log(&mut self, message: &str) {
        if let Some(prev_instruction) = self.sequential_instruction_traces.last_mut() {
            if prev_instruction.trace.last().unwrap().is_log() {
                let mut new_instruction = prev_instruction.clone();
                new_instruction.trace.pop();
                new_instruction
                    .trace
                    .push(TraceStep::from_log(message.to_string()));
                self.sequential_instruction_traces.push(new_instruction);
            } else {
                prev_instruction
                    .trace
                    .push(TraceStep::from_log(message.to_string()));
            }
        }
    }

    fn wrap_steps(&mut self, err: Option<InstructionError>) {
        if self.sequential_instruction_traces.len() > 0 {
            let current_program = self.program_trace.last().unwrap().clone();

            let mut trace_tree: Vec<TraceTree> =
                SeerHook::build_trace_tree(&mut self.sequential_instruction_traces);

            let cwd = env::current_dir()
                .expect("Failed to get current dir!")
                .into_os_string()
                .into_string()
                .expect("Path not valid UTF");

            if let Some(error) = err {
                let error_node = TraceTree {
                    instruction: self
                        .sequential_instruction_traces
                        .last()
                        .unwrap()
                        .instruction,
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
                    self.depth += 1;
                    self.sequential_instruction_traces = Vec::new();
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
                self.depth += 1;
                self.sequential_instruction_traces = Vec::new();
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

                    // let instruction_trace = match self
                    //     .die_instruction_cache
                    //     .get(&(current_program.clone(), pc_lookup))
                    // {
                    //     Some(trace) => trace.clone(),
                    //     None => {
                    //         let trace = self.get_ordered_insutrction_trace(
                    //             project_root,
                    //             current_dwarf_program,
                    //             pc_lookup,
                    //         );

                    //         self.die_instruction_cache
                    //             .insert((*current_program, pc_lookup), trace.clone());

                    //         trace
                    //     }
                    // };

                    let instruction_trace = self.get_ordered_insutrction_trace(
                        project_root,
                        current_dwarf_program,
                        pc_lookup,
                    );

                    let instruction_trace_copy = instruction_trace.trace.clone();

                    if instruction_trace_copy.len() > 0 {
                        self.sequential_instruction_traces.push(instruction_trace);
                        if let Some(trace) = self.get_extra_line_instruction_trace(
                            project_root,
                            current_dwarf_program,
                            instruction_trace_copy,
                            &pc_lookup,
                        ) {
                            self.sequential_instruction_traces.push(trace);
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

#[cfg(test)]
mod tests {
    use crate::{InstructionTrace, SeerHook, SetStatus, TraceStep};

    #[test]
    fn test_get_status_of() {
        let instruction_trace_0 = InstructionTrace {
            instruction: 0,
            trace: vec![
                TraceStep::new_for_test(0),
                TraceStep::new_for_test(1),
                TraceStep::new_for_test(2),
            ],
        };

        let instruction_trace_1 = InstructionTrace {
            instruction: 8,
            trace: vec![
                TraceStep::new_for_test(0),
                TraceStep::new_for_test(1),
                TraceStep::new_for_test(3),
            ],
        };

        let instruction_trace_2 = InstructionTrace {
            instruction: 16,
            trace: vec![TraceStep::new_for_test(0), TraceStep::new_for_test(1)],
        };

        assert_eq!(
            instruction_trace_0.get_set_status_of(&instruction_trace_1),
            SetStatus::Neither
        );
        assert_eq!(
            instruction_trace_0.get_set_status_of(&instruction_trace_2),
            SetStatus::Subset
        );
        assert_eq!(
            instruction_trace_2.get_set_status_of(&instruction_trace_1),
            SetStatus::Superset
        );
    }

    #[test]
    fn test_prune_instruction_trace_subsets() {
        let instruction_trace_0 = InstructionTrace {
            instruction: 0,
            trace: vec![
                TraceStep::new_for_test(0),
                TraceStep::new_for_test(1),
                TraceStep::new_for_test(2),
            ],
        };

        let instruction_trace_1 = InstructionTrace {
            instruction: 8,
            trace: vec![
                TraceStep::new_for_test(0),
                TraceStep::new_for_test(1),
                TraceStep::new_for_test(3),
            ],
        };

        let instruction_trace_2 = InstructionTrace {
            instruction: 16,
            trace: vec![TraceStep::new_for_test(0), TraceStep::new_for_test(1)],
        };

        let instruction_trace_3 = InstructionTrace {
            instruction: 16,
            trace: vec![
                TraceStep::new_for_test(0),
                TraceStep::new_for_test(1),
                TraceStep::new_for_test(3),
                TraceStep::new_for_test(4),
            ],
        };

        let mut traces = vec![
            instruction_trace_0,
            instruction_trace_1,
            instruction_trace_2,
            instruction_trace_3,
        ];

        SeerHook::prune_instruction_trace_subsets(&mut traces);

        assert_eq!(traces.len(), 2);
    }

    #[test]
    fn test_build_trace_tree() {
        let mut traces = vec![
            InstructionTrace {
                instruction: 0,
                trace: vec![
                    TraceStep::new_for_test(0),
                    TraceStep::new_for_test(1),
                    TraceStep::new_for_test(2),
                ],
            },
            InstructionTrace {
                instruction: 8,
                trace: vec![
                    TraceStep::new_for_test(0),
                    TraceStep::new_for_test(1),
                    TraceStep::new_for_test(3),
                ],
            },
            InstructionTrace {
                instruction: 16,
                trace: vec![TraceStep::new_for_test(0), TraceStep::new_for_test(1)],
            },
            InstructionTrace {
                instruction: 16,
                trace: vec![
                    TraceStep::new_for_test(0),
                    TraceStep::new_for_test(1),
                    TraceStep::new_for_test(3),
                    TraceStep::new_for_test(4),
                ],
            },
        ];

        let trace_tree = SeerHook::build_trace_tree(&mut traces);
    }
}
