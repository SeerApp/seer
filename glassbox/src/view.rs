use crate::analysis::{
    HideDataLenChildren, HideMode, LoadDefJson, NoiseJson, PathConditionJson, Report,
};

pub struct ViewOpts {
    pub taken_only: bool,
    pub all_load_defs: bool,
    pub skip: usize,
    pub head: usize,
    pub tail: Option<usize>,
    pub start: Option<u64>,
    pub end: Option<u64>,
    pub hide_num_account_children: HideMode,
    pub hide_data_len_children: HideDataLenChildren,
    pub hide_signer: HideMode,
    pub hide_writable: HideMode,
    pub hide_executable: HideMode,
}

pub fn view(mut report: Report, opts: &ViewOpts) -> Report {
    report
        .path_conditions
        .retain(|pc| keep_path(pc, opts));
    report.path_conditions = slice(
        report.path_conditions,
        opts.skip,
        opts.head,
        opts.tail,
    );
    if !opts.all_load_defs {
        report.load_defs.retain(|d| {
            report
                .path_conditions
                .iter()
                .any(|pc| pc.formula.contains(&d.name))
        });
    }
    report.load_defs.retain(|d| !hide_word(d, opts));
    report
}

fn keep_path(pc: &PathConditionJson, opts: &ViewOpts) -> bool {
    if opts.taken_only && !pc.taken {
        return false;
    }
    if let Some(start) = opts.start {
        if pc.order < start {
            return false;
        }
    }
    if let Some(end) = opts.end {
        if pc.order > end {
            return false;
        }
    }
    !hide_constraint(pc, opts)
}

fn hide_constraint(pc: &PathConditionJson, opts: &ViewOpts) -> bool {
    let Some(noise) = pc.noise.as_ref() else {
        return false;
    };
    match noise {
        NoiseJson::NumAccountsChild => opts.hide_num_account_children.hide_constraints(),
        NoiseJson::DataLenChild { account } => opts.hide_data_len_children.includes(*account),
        NoiseJson::Provision {
            signer,
            writable,
            executable,
        } => {
            (!signer || opts.hide_signer.hide_constraints())
                && (!writable || opts.hide_writable.hide_constraints())
                && (!executable || opts.hide_executable.hide_constraints())
        }
    }
}

fn hide_word(def: &LoadDefJson, opts: &ViewOpts) -> bool {
    if opts.hide_num_account_children.hide_words()
        && crate::ancestry::is_num_accounts_child_name(&def.name)
    {
        return true;
    }
    if let Some(i) = crate::ancestry::data_len_child_account(&def.name) {
        return opts.hide_data_len_children.includes(i);
    }
    false
}

fn slice(
    mut values: Vec<PathConditionJson>,
    skip: usize,
    head: usize,
    tail: Option<usize>,
) -> Vec<PathConditionJson> {
    if skip >= values.len() {
        return Vec::new();
    }
    values.drain(..skip);
    if let Some(n) = tail {
        let start = values.len().saturating_sub(n);
        values.drain(..start);
        return values;
    }
    if head != 0 {
        values.truncate(head);
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{Minted, RelJson, Skipped};

    fn pc(order: u64, taken: bool, formula: &str) -> PathConditionJson {
        PathConditionJson {
            order,
            pc: 0,
            disasm: String::new(),
            taken,
            rel: RelJson::Eq,
            lhs: "0x0".into(),
            rhs: "0x0".into(),
            formula: formula.into(),
            noise: None,
            origins: vec![],
        }
    }

    fn empty_report(pcs: Vec<PathConditionJson>) -> Report {
        Report {
            version: 1,
            run: 1,
            ix: 0,
            steps_applied: 0,
            missing_disasm: 0,
            minted: Minted {
                text: 0,
                input: 0,
                load_temps: 0,
                memory_cells: 0,
            },
            skipped: Skipped {
                tautologies: 0,
                text_noise: 0,
            },
            path_conditions: pcs,
            load_defs: vec![],
        }
    }

    fn opts() -> ViewOpts {
        ViewOpts {
            taken_only: false,
            all_load_defs: false,
            skip: 0,
            head: 20,
            tail: None,
            start: None,
            end: None,
            hide_num_account_children: HideMode::Off,
            hide_data_len_children: HideDataLenChildren::Off,
            hide_signer: HideMode::Off,
            hide_writable: HideMode::Off,
            hide_executable: HideMode::Off,
        }
    }

    #[test]
    fn taken_only_and_head() {
        let report = empty_report(vec![
            pc(0, false, "a"),
            pc(1, true, "b"),
            pc(2, true, "c"),
        ]);
        let mut o = opts();
        o.taken_only = true;
        o.head = 1;
        let out = view(report, &o);
        assert_eq!(out.path_conditions.len(), 1);
        assert_eq!(out.path_conditions[0].order, 1);
    }
}
