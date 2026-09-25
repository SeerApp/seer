use anyhow::{bail, Result};
use storage::Storage;

use super::{emit, Command};

pub(super) fn cmd(storage: &Storage, cmd: Command, short: bool) -> Result<()> {
    let Command::Glassbox {
        id,
        ix,
        force,
        skip,
        head,
        tail,
        start,
        end,
        taken_only,
        all_load_defs,
        hide_num_account_children,
        hide_data_len_children,
        hide_signer,
        hide_writable,
        hide_executable,
    } = cmd
    else {
        bail!("internal: glassbox");
    };
    let bytes = glassbox::store(storage, id, ix, force)?;
    let report: glassbox::Report = serde_json::from_slice(&bytes)?;
    let missing = report.missing_disasm;
    let report = glassbox::view(
        report,
        &glassbox::ViewOpts {
            taken_only,
            all_load_defs,
            skip,
            head,
            tail,
            start,
            end,
            hide_num_account_children,
            hide_data_len_children,
            hide_signer,
            hide_writable,
            hide_executable,
        },
    );
    let value = serde_json::to_value(&report)?;
    let mut flags = format!("seer glassbox {id} --ix {ix}");
    if taken_only {
        flags.push_str(" --taken-only");
    }
    if all_load_defs {
        flags.push_str(" --all-load-defs");
    }
    let mut footer = Vec::new();
    if head != 0 && tail.is_none() {
        footer.push(format!(
            "{flags} --skip {} --head {head}",
            skip.saturating_add(head)
        ));
    }
    footer.push(format!("seer show {id}"));
    if missing > 0 {
        footer.push("seer program <HASH> --disasm".into());
        footer.push(format!("seer glassbox {id} --ix {ix} --force"));
    }
    emit(value, &footer, short)
}
