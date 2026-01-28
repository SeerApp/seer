use std::collections::HashMap;

use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;

use crate::{
    call_trace_lookup::CallTraceLookup,
    tree::{
        entrypoint::EntrypointHolder,
        invoke::InvokeNode,
        view::{AccountData, InvokeData, ViewNode},
        Tree, TreeContext,
    },
};

pub struct Tracer {
    sender: Pubkey,
    invoke_context: Option<TreeContext<InvokeData, InvokeNode, Option<EntrypointHolder>>>,
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

    pub fn start_program(&mut self, program_address: Pubkey) {
        if let Some(invoke_context) = self.invoke_context.as_mut() {
            invoke_context.start_program(invoke_context.get_last_receiver(), program_address);
        } else {
            self.invoke_context = Some(TreeContext::new_invoke_context(
                self.sender,
                program_address,
            ));
        };
    }

    pub fn end_program(&mut self, err: Option<InstructionError>) {
        self.invoke_context
            .as_mut()
            .expect("Invoke context must exist before end program call")
            .end_program(err);
    }

    pub fn step(
        &mut self,
        lookups: &HashMap<Pubkey, CallTraceLookup>,
        i: u64,
    ) {
        self.invoke_context
            .as_mut()
            .expect("Stepping before invoke context exists")
            .step(lookups, i, &mut self.executed);
    }

    pub fn log(&mut self, message: &str) {
        self.invoke_context
            .as_mut()
            .expect("Logging before invoke context")
            .log(message);
    }

    pub fn account_diff(&mut self, data: AccountData) {
        self.invoke_context
            .as_mut()
            .expect("Account diff before invoke context")
            .account_diff(data);
    }
}

impl From<Tracer> for Option<Tree<ViewNode>> {
    fn from(value: Tracer) -> Self {
        if value.executed {
            let invoke_context = value
                .invoke_context
                .expect("Converting tracer with empty invoke context into view tree");

            invoke_context.into()
        } else {
            None
        }
    }
}
