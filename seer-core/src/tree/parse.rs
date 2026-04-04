use crate::{
    idl::IdlTreeParser,
    tree::nodes::{
        EntrypointChildren, FnCallChildren, RootChildren, TreeAccount, TreeEntrypoint, TreeError,
        TreeFnCall, TreeRoot,
    },
};

impl TreeRoot<RootChildren> {
    pub fn parse<T: IdlTreeParser>(&mut self, parser: &T) {
        self.parsed = parser.get_instruction(&self.data);
        for c in &mut self.children {
            match c {
                RootChildren::Entrypoint(e) => {
                    e.parse(parser);
                }
                RootChildren::Account(a) => {
                    a.parse(parser);
                }
                RootChildren::Error(e) => {
                    e.parse(parser);
                }
                _ => continue,
            }
        }
    }
}

impl TreeEntrypoint<EntrypointChildren> {
    pub fn parse<T: IdlTreeParser>(&mut self, parser: &T) {
        for c in &mut self.children {
            match c {
                EntrypointChildren::Entrypoint(e) => {
                    e.parse(parser);
                }
                EntrypointChildren::Account(a) => {
                    a.parse(parser);
                }
                EntrypointChildren::Error(e) => {
                    e.parse(parser);
                }
                EntrypointChildren::FnCall(f) => {
                    f.parse(parser);
                }
                _ => continue,
            }
        }
    }
}

impl TreeFnCall<FnCallChildren> {
    pub fn parse<T: IdlTreeParser>(&mut self, parser: &T) {
        for c in &mut self.children {
            match c {
                FnCallChildren::Entrypoint(e) => {
                    e.parse(parser);
                }
                FnCallChildren::Account(a) => {
                    a.parse(parser);
                }
                FnCallChildren::Error(e) => {
                    e.parse(parser);
                }
                FnCallChildren::FnCall(f) => {
                    f.parse(parser);
                }
                _ => continue,
            }
        }
    }
}

impl TreeAccount {
    pub fn parse<T: IdlTreeParser>(&mut self, parser: &T) {
        self.before
            .set_parsed(parser.get_account(self.before.data()));
        self.after.set_parsed(parser.get_account(self.after.data()));
    }
}

impl TreeError {
    pub fn parse<T: IdlTreeParser>(&mut self, parser: &T) {
        self.message = parser.get_error(self.instruction_error.clone());
    }
}
