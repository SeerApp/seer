use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;

use crate::{
    contexts::invoke::InvokeContext,
    program_manager::types::GlobalProgramContext,
    tree::nodes::{
        account::TreeAccount,
        root::{RootViewChildren, TreeRoot},
    },
};

/// Sender-preserving layer
pub struct Tracer {
    sender: Pubkey,
    invoke_context: Option<InvokeContext>,
}

impl Tracer {
    pub fn new(sender: Pubkey) -> Self {
        Self {
            sender,
            invoke_context: None,
        }
    }

    pub fn get_current_program_address(&self) -> Pubkey {
        self.invoke_context
            .as_ref()
            .expect("Get current program address before invoke context")
            .get_last_receiver()
            .clone()
    }

    /// True when a program is already running (next `start_program` is a CPI).
    pub fn has_invoke_context(&self) -> bool {
        self.invoke_context.is_some()
    }

    pub fn start_program(
        &mut self,
        accounts: Vec<Pubkey>,
        data: Vec<u8>,
        program_address: Pubkey,
        step_order: u64,
    ) {
        if let Some(invoke_context) = self.invoke_context.as_mut() {
            invoke_context.start_program(
                accounts,
                data,
                invoke_context.get_last_receiver(),
                program_address,
                step_order,
            );
        } else {
            let mut invoke_context = InvokeContext::new();
            invoke_context.start_program(accounts, data, self.sender, program_address, step_order);
            self.invoke_context = Some(invoke_context);
        };
    }

    pub fn end_program(&mut self, err: Option<InstructionError>, step_order: u64) {
        self.invoke_context
            .as_mut()
            .expect("Invoke context must exist before end program call")
            .end_program(err, step_order);
    }

    pub fn step(&mut self, global_program_context: &GlobalProgramContext, i: u64, step_order: u64) {
        self.invoke_context
            .as_mut()
            .expect("Stepping before invoke context exists")
            .step(global_program_context, i, step_order);
    }

    pub fn log(&mut self, message: &str, step_order: u64) {
        self.invoke_context
            .as_mut()
            .expect("Logging before invoke context")
            .log(message, step_order);
    }

    pub fn account_diff(&mut self, data: TreeAccount) {
        self.invoke_context
            .as_mut()
            .expect("Account diff before invoke context")
            .account_diff(data);
    }

    pub fn finalize_tree(&mut self, global_program_context: &GlobalProgramContext) {
        let invoke_context = self
            .invoke_context
            .as_mut()
            .expect("Finalizing tree on empty invoke context");

        invoke_context.flatten_account_diffs();

        for tree in invoke_context.trees_iter_mut() {
            if let Some(idl_lookup) = global_program_context.get_idl_lookup(&tree.receiver) {
                idl_lookup.parse_tree(tree);
            }
        }
    }
}

impl From<Tracer> for Option<TreeRoot<RootViewChildren>> {
    fn from(value: Tracer) -> Self {
        let invoke_context = value
            .invoke_context
            .expect("Converting tracer with empty invoke context into view tree");

        Some(invoke_context.into())
    }
}
