use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;

use crate::{
    contexts::invoke::InvokeContext,
    program_manager::program_manager::ProgramManager,
    tree::nodes::{RootViewChildren, TreeAccount, TreeRoot},
};

/// Sender-preserving layer
pub struct Tracer {
    sender: Pubkey,
    invoke_context: Option<InvokeContext>,
    executed: bool,
}

impl Tracer {
    pub fn new(sender: Pubkey) -> Self {
        Self {
            sender,
            invoke_context: None,
            executed: false,
        }
    }

    pub fn start_program(&mut self, accounts: Vec<Pubkey>, data: Vec<u8>, program_address: Pubkey) {
        if let Some(invoke_context) = self.invoke_context.as_mut() {
            invoke_context.start_program(
                accounts,
                data,
                invoke_context.get_last_receiver(),
                program_address,
            );
        } else {
            let mut invoke_context = InvokeContext::new();
            invoke_context.start_program(accounts, data, self.sender, program_address);
            self.invoke_context = Some(invoke_context);
        };
    }

    pub fn end_program(&mut self, err: Option<InstructionError>) {
        self.invoke_context
            .as_mut()
            .expect("Invoke context must exist before end program call")
            .end_program(err);
    }

    pub fn step(&mut self, program_manager: &ProgramManager, i: u64) {
        self.invoke_context
            .as_mut()
            .expect("Stepping before invoke context exists")
            .step(program_manager, i, &mut self.executed);
    }

    pub fn log(&mut self, message: &str) {
        self.invoke_context
            .as_mut()
            .expect("Logging before invoke context")
            .log(message);
    }

    pub fn account_diff(&mut self, data: TreeAccount) {
        self.invoke_context
            .as_mut()
            .expect("Account diff before invoke context")
            .account_diff(data);
    }

    pub fn executed(&self) -> bool {
        self.executed
    }

    pub fn finalize_tree(&mut self, program_manager: &ProgramManager) {
        let invoke_context = self
            .invoke_context
            .as_mut()
            .expect("Finalizing tree on empty invoke context");

        invoke_context.flatten_account_diffs();

        for tree in invoke_context.trees_iter_mut() {
            if let Some(idl_lookup) = program_manager.get_idl_lookup(&tree.receiver) {
                idl_lookup.parse_tree(tree);
            }
        }
    }
}

impl From<Tracer> for Option<TreeRoot<RootViewChildren>> {
    fn from(value: Tracer) -> Self {
        if value.executed {
            let invoke_context = value
                .invoke_context
                .expect("Converting tracer with empty invoke context into view tree");

            Some(invoke_context.into())
        } else {
            None
        }
    }
}
