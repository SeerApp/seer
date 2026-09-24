mod cursor;
mod framework_error;
mod parsed_arg;

use std::collections::HashMap;

use anchor_lang_idl_spec::{Idl, IdlInstructionAccountItem};
use solana_instruction_error::InstructionError;

use crate::{
    anchor::parsed_arg::{get_idl_type_def_ty, get_parsed_arg_value},
    cursor::Cursor,
    display_error_name, IdlTreeParser,
};
use trace::tree::parsed::{ParsedAccount, ParsedArg, ParsedInstruction, ProgramIdentifier};

pub struct AnchorIdlLookup {
    idl: Idl,
}

impl IdlTreeParser for AnchorIdlLookup {
    fn get_instruction(&self, data: &[u8]) -> Option<ParsedInstruction> {
        let mut cursor = Cursor::new(data);

        for ix in &self.idl.instructions {
            if cursor.match_discriminator(&ix.discriminator) {
                let mut parsed_args = vec![];
                let account_names = self.get_account_names(&ix.accounts);
                for arg in &ix.args {
                    if let Some(parsed_arg) =
                        get_parsed_arg_value(&self.idl.types, arg, &mut cursor)
                    {
                        parsed_args.push(ParsedArg {
                            name: arg.name.clone(),
                            value: parsed_arg,
                        });
                    } else {
                        break;
                    }
                }

                return Some(ParsedInstruction {
                    id: ProgramIdentifier::Default,
                    name: ix.name.clone(),
                    account_names,
                    args: parsed_args,
                });
            }
        }

        None
    }

    fn get_account(&self, data: &[u8]) -> Option<ParsedAccount> {
        for acc in &self.idl.accounts {
            let mut c = Cursor::new(data);
            let disc_ok = if acc.discriminator.is_empty() {
                self.idl.accounts.len() == 1
            } else {
                c.match_discriminator(&acc.discriminator)
            };
            if !disc_ok {
                continue;
            }

            let Some(ty_def) = self.idl.types.iter().find(|t| t.name == acc.name) else {
                continue;
            };

            if let Some(value) =
                get_idl_type_def_ty(&self.idl.types, &mut c, &HashMap::new(), &ty_def.ty)
            {
                return Some(ParsedAccount {
                    id: ProgramIdentifier::Default,
                    data: ParsedArg {
                        name: acc.name.clone(),
                        value,
                    },
                });
            } else {
                continue;
            }
        }

        None
    }

    fn get_error(&self, error: InstructionError) -> String {
        match error {
            InstructionError::Custom(code) => {
                framework_error::try_format_anchor_framework_error(code)
                    .or_else(|| {
                        self.idl
                            .errors
                            .iter()
                            .find(|e| e.code == code)
                            .map(|e| format!("{}: {:#?}", display_error_name(&e.name), e.msg))
                    })
                    .unwrap_or_else(|| InstructionError::Custom(code).to_string())
            }
            _ => error.to_string(),
        }
    }
}

impl AnchorIdlLookup {
    pub fn from_json_str(idl_json: &str) -> Result<Self, serde_json::Error> {
        let idl: Idl = serde_json::from_str(idl_json)?;
        Ok(Self { idl })
    }

    fn get_account_names(&self, accounts: &[IdlInstructionAccountItem]) -> Vec<String> {
        let mut account_names = vec![];

        for ax in accounts {
            match ax {
                IdlInstructionAccountItem::Composite(acc) => {
                    account_names.extend(self.get_account_names(&acc.accounts));
                }
                IdlInstructionAccountItem::Single(acc) => {
                    account_names.push(acc.name.clone());
                }
            }
        }

        account_names
    }
}
