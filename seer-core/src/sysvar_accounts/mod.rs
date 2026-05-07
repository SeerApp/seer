//! Purpose: parse known sysvar accounts with stable, built-in layouts.

use codama_nodes::{Docs, NumberFormat};
use solana_pubkey::Pubkey;
use std::str::FromStr;

use crate::{
    idl::{
        parsed_arg::{
            ParsedArg, ParsedArgValue, ViewArrayTypeNode, ViewBooleanTypeNode, ViewBytesTypeNode, ViewNumberTypeNode, ViewStructFieldTypeNode, ViewStructTypeNode, collect_parsed_arg_byte_offsets
        },
        types::{ParsedAccount, ProgramIdentifier},
    }, program_manager::types::AccountIdlParseResult,
};

const SYSVAR_RENT_PUBKEY: &str = "SysvarRent111111111111111111111111111111111";
const SYSVAR_RENT_LEN: usize = 8 + 8 + 1;
const SYSVAR_CLOCK_PUBKEY: &str = "SysvarC1ock11111111111111111111111111111111";
const SYSVAR_CLOCK_LEN: usize = 8 + 8 + 8 + 8 + 8;
const SYSVAR_EPOCH_SCHEDULE_PUBKEY: &str = "SysvarEpochSchedu1e111111111111111111111111";
const SYSVAR_EPOCH_SCHEDULE_LEN: usize = 8 + 8 + 1 + 8 + 8;
const SYSVAR_EPOCH_REWARDS_PUBKEY: &str = "SysvarEpochRewards1111111111111111111111111";
const SYSVAR_EPOCH_REWARDS_LEN: usize = 8 + 8 + 32 + 16 + 8 + 8 + 1;
const SYSVAR_LAST_RESTART_SLOT_PUBKEY: &str = "SysvarLastRestartS1ot1111111111111111111111";
const SYSVAR_LAST_RESTART_SLOT_LEN: usize = 8;
const SYSVAR_STAKE_HISTORY_PUBKEY: &str = "SysvarStakeHistory1111111111111111111111111";
const SYSVAR_STAKE_HISTORY_VEC_LEN_PREFIX: usize = 8;
const STAKE_HISTORY_ENTRY_LEN: usize = 8 + 8 + 8 + 8;

pub fn parse_sysvar_account(key: &Pubkey, bytes: &[u8]) -> Option<AccountIdlParseResult> {
    parse_sysvar_rent_account(key, bytes)
        .or_else(|| parse_sysvar_clock_account(key, bytes))
        .or_else(|| parse_sysvar_epoch_schedule_account(key, bytes))
        .or_else(|| parse_sysvar_epoch_rewards_account(key, bytes))
        .or_else(|| parse_sysvar_last_restart_slot_account(key, bytes))
        .or_else(|| parse_sysvar_stake_history_account(key, bytes))
}

fn parse_sysvar_rent_account(key: &Pubkey, bytes: &[u8]) -> Option<AccountIdlParseResult> {
    let rent_key = Pubkey::from_str(SYSVAR_RENT_PUBKEY).expect("valid sysvar rent pubkey");
    if *key != rent_key || bytes.len() < SYSVAR_RENT_LEN {
        return None;
    }

    let lamports_per_byte_year = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
    let exemption_threshold = f64::from_le_bytes(bytes[8..16].try_into().ok()?);
    let burn_percent = bytes[16];

    let parsed = ParsedAccount {
        id: ProgramIdentifier::Default,
        data: ParsedArg {
            name: "rent".to_string(),
            value: ParsedArgValue::Struct(ViewStructTypeNode {
                fields: vec![
                    ViewStructFieldTypeNode {
                        name: "lamports_per_byte_year".to_string(),
                        docs: Docs::default(),
                        byte_offset: Some(0),
                        value: ParsedArgValue::Number(ViewNumberTypeNode {
                            value: lamports_per_byte_year.to_string(),
                            format: NumberFormat::U64,
                        }),
                    },
                    ViewStructFieldTypeNode {
                        name: "exemption_threshold".to_string(),
                        docs: Docs::default(),
                        byte_offset: Some(8),
                        value: ParsedArgValue::Number(ViewNumberTypeNode {
                            value: exemption_threshold.to_string(),
                            format: NumberFormat::F64,
                        }),
                    },
                    ViewStructFieldTypeNode {
                        name: "burn_percent".to_string(),
                        docs: Docs::default(),
                        byte_offset: Some(16),
                        value: ParsedArgValue::Number(ViewNumberTypeNode {
                            value: burn_percent.to_string(),
                            format: NumberFormat::U8,
                        }),
                    },
                ],
            }),
        },
    };

    let parsed_byte_offsets = collect_parsed_arg_byte_offsets(&parsed.data);
    Some(AccountIdlParseResult {
        parsed,
        parsed_byte_offsets,
    })
}

fn parse_sysvar_clock_account(key: &Pubkey, bytes: &[u8]) -> Option<AccountIdlParseResult> {
    let clock_key = Pubkey::from_str(SYSVAR_CLOCK_PUBKEY).expect("valid sysvar clock pubkey");
    if *key != clock_key || bytes.len() < SYSVAR_CLOCK_LEN {
        return None;
    }

    let slot = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
    let epoch_start_timestamp = i64::from_le_bytes(bytes[8..16].try_into().ok()?);
    let epoch = u64::from_le_bytes(bytes[16..24].try_into().ok()?);
    let leader_schedule_epoch = u64::from_le_bytes(bytes[24..32].try_into().ok()?);
    let unix_timestamp = i64::from_le_bytes(bytes[32..40].try_into().ok()?);

    let parsed = ParsedAccount {
        id: ProgramIdentifier::Default,
        data: ParsedArg {
            name: "clock".to_string(),
            value: ParsedArgValue::Struct(ViewStructTypeNode {
                fields: vec![
                    number_field("slot", 0, slot, NumberFormat::U64),
                    number_field(
                        "epoch_start_timestamp",
                        8,
                        epoch_start_timestamp,
                        NumberFormat::I64,
                    ),
                    number_field("epoch", 16, epoch, NumberFormat::U64),
                    number_field(
                        "leader_schedule_epoch",
                        24,
                        leader_schedule_epoch,
                        NumberFormat::U64,
                    ),
                    number_field("unix_timestamp", 32, unix_timestamp, NumberFormat::I64),
                ],
            }),
        },
    };
    parsed_result(parsed)
}

fn parse_sysvar_epoch_schedule_account(key: &Pubkey, bytes: &[u8]) -> Option<AccountIdlParseResult> {
    let epoch_schedule_key =
        Pubkey::from_str(SYSVAR_EPOCH_SCHEDULE_PUBKEY).expect("valid sysvar epoch schedule pubkey");
    if *key != epoch_schedule_key || bytes.len() < SYSVAR_EPOCH_SCHEDULE_LEN {
        return None;
    }

    let slots_per_epoch = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
    let leader_schedule_slot_offset = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
    let warmup = bytes[16] != 0;
    let first_normal_epoch = u64::from_le_bytes(bytes[17..25].try_into().ok()?);
    let first_normal_slot = u64::from_le_bytes(bytes[25..33].try_into().ok()?);

    let parsed = ParsedAccount {
        id: ProgramIdentifier::Default,
        data: ParsedArg {
            name: "epoch_schedule".to_string(),
            value: ParsedArgValue::Struct(ViewStructTypeNode {
                fields: vec![
                    number_field("slots_per_epoch", 0, slots_per_epoch, NumberFormat::U64),
                    number_field(
                        "leader_schedule_slot_offset",
                        8,
                        leader_schedule_slot_offset,
                        NumberFormat::U64,
                    ),
                    ViewStructFieldTypeNode {
                        name: "warmup".to_string(),
                        docs: Docs::default(),
                        byte_offset: Some(16),
                        value: ParsedArgValue::Boolean(ViewBooleanTypeNode { value: warmup }),
                    },
                    number_field(
                        "first_normal_epoch",
                        17,
                        first_normal_epoch,
                        NumberFormat::U64,
                    ),
                    number_field("first_normal_slot", 25, first_normal_slot, NumberFormat::U64),
                ],
            }),
        },
    };
    parsed_result(parsed)
}

fn parse_sysvar_epoch_rewards_account(key: &Pubkey, bytes: &[u8]) -> Option<AccountIdlParseResult> {
    let epoch_rewards_key =
        Pubkey::from_str(SYSVAR_EPOCH_REWARDS_PUBKEY).expect("valid sysvar epoch rewards pubkey");
    if *key != epoch_rewards_key || bytes.len() < SYSVAR_EPOCH_REWARDS_LEN {
        return None;
    }

    let distribution_starting_block_height = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
    let num_partitions = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
    let parent_blockhash = hex::encode(&bytes[16..48]);
    let total_points = u128::from_le_bytes(bytes[48..64].try_into().ok()?);
    let total_rewards = u64::from_le_bytes(bytes[64..72].try_into().ok()?);
    let distributed_rewards = u64::from_le_bytes(bytes[72..80].try_into().ok()?);
    let active = bytes[80] != 0;

    let parsed = ParsedAccount {
        id: ProgramIdentifier::Default,
        data: ParsedArg {
            name: "epoch_rewards".to_string(),
            value: ParsedArgValue::Struct(ViewStructTypeNode {
                fields: vec![
                    number_field(
                        "distribution_starting_block_height",
                        0,
                        distribution_starting_block_height,
                        NumberFormat::U64,
                    ),
                    number_field("num_partitions", 8, num_partitions, NumberFormat::U64),
                    ViewStructFieldTypeNode {
                        name: "parent_blockhash".to_string(),
                        docs: Docs::default(),
                        byte_offset: Some(16),
                        value: ParsedArgValue::Bytes(ViewBytesTypeNode {
                            value: parent_blockhash,
                        }),
                    },
                    number_field("total_points", 48, total_points, NumberFormat::U128),
                    number_field("total_rewards", 64, total_rewards, NumberFormat::U64),
                    number_field(
                        "distributed_rewards",
                        72,
                        distributed_rewards,
                        NumberFormat::U64,
                    ),
                    ViewStructFieldTypeNode {
                        name: "active".to_string(),
                        docs: Docs::default(),
                        byte_offset: Some(80),
                        value: ParsedArgValue::Boolean(ViewBooleanTypeNode { value: active }),
                    },
                ],
            }),
        },
    };
    parsed_result(parsed)
}

fn parse_sysvar_last_restart_slot_account(key: &Pubkey, bytes: &[u8]) -> Option<AccountIdlParseResult> {
    let last_restart_slot_key = Pubkey::from_str(SYSVAR_LAST_RESTART_SLOT_PUBKEY)
        .expect("valid sysvar last restart slot pubkey");
    if *key != last_restart_slot_key || bytes.len() < SYSVAR_LAST_RESTART_SLOT_LEN {
        return None;
    }

    let last_restart_slot = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
    let parsed = ParsedAccount {
        id: ProgramIdentifier::Default,
        data: ParsedArg {
            name: "last_restart_slot".to_string(),
            value: ParsedArgValue::Struct(ViewStructTypeNode {
                fields: vec![number_field(
                    "last_restart_slot",
                    0,
                    last_restart_slot,
                    NumberFormat::U64,
                )],
            }),
        },
    };
    parsed_result(parsed)
}

fn parse_sysvar_stake_history_account(key: &Pubkey, bytes: &[u8]) -> Option<AccountIdlParseResult> {
    let stake_history_key =
        Pubkey::from_str(SYSVAR_STAKE_HISTORY_PUBKEY).expect("valid sysvar stake history pubkey");
    if *key != stake_history_key || bytes.len() < SYSVAR_STAKE_HISTORY_VEC_LEN_PREFIX {
        return None;
    }

    let count = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
    let count: usize = count.try_into().ok()?;
    let needed_len = SYSVAR_STAKE_HISTORY_VEC_LEN_PREFIX
        .checked_add(count.checked_mul(STAKE_HISTORY_ENTRY_LEN)?)?;
    if bytes.len() < needed_len {
        return None;
    }

    let mut values = Vec::with_capacity(count);
    for idx in 0..count {
        let entry_start = SYSVAR_STAKE_HISTORY_VEC_LEN_PREFIX + idx * STAKE_HISTORY_ENTRY_LEN;
        let epoch = u64::from_le_bytes(bytes[entry_start..entry_start + 8].try_into().ok()?);
        let effective =
            u64::from_le_bytes(bytes[entry_start + 8..entry_start + 16].try_into().ok()?);
        let activating =
            u64::from_le_bytes(bytes[entry_start + 16..entry_start + 24].try_into().ok()?);
        let deactivating =
            u64::from_le_bytes(bytes[entry_start + 24..entry_start + 32].try_into().ok()?);

        values.push(ParsedArgValue::Struct(ViewStructTypeNode {
            fields: vec![
                number_field("epoch", entry_start, epoch, NumberFormat::U64),
                number_field("effective", entry_start + 8, effective, NumberFormat::U64),
                number_field(
                    "activating",
                    entry_start + 16,
                    activating,
                    NumberFormat::U64,
                ),
                number_field(
                    "deactivating",
                    entry_start + 24,
                    deactivating,
                    NumberFormat::U64,
                ),
            ],
        }));
    }

    let parsed = ParsedAccount {
        id: ProgramIdentifier::Default,
        data: ParsedArg {
            name: "stake_history".to_string(),
            value: ParsedArgValue::Array(ViewArrayTypeNode { values }),
        },
    };
    parsed_result(parsed)
}

fn number_field<T: ToString>(
    name: &str,
    byte_offset: usize,
    value: T,
    format: NumberFormat,
) -> ViewStructFieldTypeNode {
    ViewStructFieldTypeNode {
        name: name.to_string(),
        docs: Docs::default(),
        byte_offset: Some(byte_offset),
        value: ParsedArgValue::Number(ViewNumberTypeNode {
            value: value.to_string(),
            format,
        }),
    }
}

fn parsed_result(parsed: ParsedAccount) -> Option<AccountIdlParseResult> {
    let parsed_byte_offsets = collect_parsed_arg_byte_offsets(&parsed.data);
    Some(AccountIdlParseResult {
        parsed,
        parsed_byte_offsets,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        parse_sysvar_account, SYSVAR_CLOCK_PUBKEY, SYSVAR_EPOCH_REWARDS_PUBKEY,
        SYSVAR_EPOCH_SCHEDULE_PUBKEY, SYSVAR_LAST_RESTART_SLOT_PUBKEY, SYSVAR_RENT_PUBKEY,
        SYSVAR_STAKE_HISTORY_PUBKEY,
    };
    use solana_pubkey::Pubkey;
    use std::str::FromStr;

    #[test]
    fn parses_sysvar_rent() {
        let key = Pubkey::from_str(SYSVAR_RENT_PUBKEY).expect("sysvar rent pubkey must parse");
        let mut data = vec![];
        data.extend_from_slice(&1234_u64.to_le_bytes());
        data.extend_from_slice(&2.5_f64.to_le_bytes());
        data.push(42_u8);

        let parsed = parse_sysvar_account(&key, &data).expect("sysvar rent should parse");
        assert_eq!(parsed.parsed.data.name, "rent");
        assert_eq!(parsed.parsed_byte_offsets.len(), 3);
    }

    #[test]
    fn parses_sysvar_clock() {
        let key = Pubkey::from_str(SYSVAR_CLOCK_PUBKEY).expect("sysvar clock pubkey must parse");
        let mut data = vec![];
        data.extend_from_slice(&10_u64.to_le_bytes());
        data.extend_from_slice(&20_i64.to_le_bytes());
        data.extend_from_slice(&30_u64.to_le_bytes());
        data.extend_from_slice(&40_u64.to_le_bytes());
        data.extend_from_slice(&50_i64.to_le_bytes());

        let parsed = parse_sysvar_account(&key, &data).expect("sysvar clock should parse");
        assert_eq!(parsed.parsed.data.name, "clock");
        assert_eq!(parsed.parsed_byte_offsets.len(), 5);
    }

    #[test]
    fn parses_sysvar_epoch_schedule() {
        let key = Pubkey::from_str(SYSVAR_EPOCH_SCHEDULE_PUBKEY)
            .expect("sysvar epoch schedule pubkey must parse");
        let mut data = vec![];
        data.extend_from_slice(&100_u64.to_le_bytes());
        data.extend_from_slice(&200_u64.to_le_bytes());
        data.push(1_u8);
        data.extend_from_slice(&300_u64.to_le_bytes());
        data.extend_from_slice(&400_u64.to_le_bytes());

        let parsed = parse_sysvar_account(&key, &data).expect("sysvar epoch schedule should parse");
        assert_eq!(parsed.parsed.data.name, "epoch_schedule");
        assert_eq!(parsed.parsed_byte_offsets.len(), 5);
    }

    #[test]
    fn parses_sysvar_epoch_rewards() {
        let key = Pubkey::from_str(SYSVAR_EPOCH_REWARDS_PUBKEY)
            .expect("sysvar epoch rewards pubkey must parse");
        let mut data = vec![];
        data.extend_from_slice(&11_u64.to_le_bytes());
        data.extend_from_slice(&22_u64.to_le_bytes());
        data.extend_from_slice(&[7_u8; 32]);
        data.extend_from_slice(&33_u128.to_le_bytes());
        data.extend_from_slice(&44_u64.to_le_bytes());
        data.extend_from_slice(&55_u64.to_le_bytes());
        data.push(1_u8);

        let parsed = parse_sysvar_account(&key, &data).expect("sysvar epoch rewards should parse");
        assert_eq!(parsed.parsed.data.name, "epoch_rewards");
        assert_eq!(parsed.parsed_byte_offsets.len(), 7);
    }

    #[test]
    fn parses_sysvar_last_restart_slot() {
        let key = Pubkey::from_str(SYSVAR_LAST_RESTART_SLOT_PUBKEY)
            .expect("sysvar last restart slot pubkey must parse");
        let data = 999_u64.to_le_bytes();

        let parsed = parse_sysvar_account(&key, &data).expect("sysvar last restart slot should parse");
        assert_eq!(parsed.parsed.data.name, "last_restart_slot");
        assert_eq!(parsed.parsed_byte_offsets.len(), 1);
    }

    #[test]
    fn parses_sysvar_stake_history() {
        let key = Pubkey::from_str(SYSVAR_STAKE_HISTORY_PUBKEY)
            .expect("sysvar stake history pubkey must parse");
        let mut data = vec![];
        data.extend_from_slice(&2_u64.to_le_bytes());
        data.extend_from_slice(&1_u64.to_le_bytes());
        data.extend_from_slice(&2_u64.to_le_bytes());
        data.extend_from_slice(&3_u64.to_le_bytes());
        data.extend_from_slice(&4_u64.to_le_bytes());
        data.extend_from_slice(&5_u64.to_le_bytes());
        data.extend_from_slice(&6_u64.to_le_bytes());
        data.extend_from_slice(&7_u64.to_le_bytes());
        data.extend_from_slice(&8_u64.to_le_bytes());

        let parsed = parse_sysvar_account(&key, &data).expect("sysvar stake history should parse");
        assert_eq!(parsed.parsed.data.name, "stake_history");
        assert_eq!(parsed.parsed_byte_offsets.len(), 8);
    }
}
