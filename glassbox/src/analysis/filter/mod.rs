//! Which load defs to print for a given hide filter.

use std::collections::HashSet;

use crate::ancestry::{
    collect_w_names_ast, data_len_child_account, is_acc_flag_only_word, is_num_accounts_child_name,
};
use crate::state::{LoadDef, PathCondition};

use super::options::HideFilters;

fn close_under_load_def_deps(referenced: &mut HashSet<String>, load_defs: &[LoadDef]) {
    let mut changed = true;
    while changed {
        changed = false;
        for def in load_defs {
            if !referenced.contains(&def.name) {
                continue;
            }
            for dep in collect_w_names_ast(&def.expr) {
                if referenced.insert(dep) {
                    changed = true;
                }
            }
        }
    }
}

fn hide_word_name(name: &str, hide: HideFilters, load_defs: &[LoadDef]) -> bool {
    if hide.num_account_children.hide_words() && is_num_accounts_child_name(name) {
        return true;
    }
    if let Some(i) = data_len_child_account(name) {
        return hide.data_len_children.includes(i);
    }
    if hide.any_flag_words() {
        if let Some(def) = load_defs.iter().find(|d| d.name == name) {
            return is_acc_flag_only_word(&def.expr, load_defs, |f| hide.hide_flag_words(f));
        }
    }
    false
}

/// Load defs to print: all minted temps, or those named in path conditions
/// plus any `w_*` they transitively depend on in their definitions.
pub(crate) fn load_defs_to_show<'a>(
    path_conditions: impl IntoIterator<Item = &'a PathCondition>,
    load_defs: &'a [LoadDef],
    all: bool,
    hide: HideFilters,
) -> Vec<&'a LoadDef> {
    let path_conditions: Vec<&'a PathCondition> = path_conditions.into_iter().collect();
    let mut referenced = HashSet::new();
    if all {
        for def in load_defs {
            referenced.insert(def.name.clone());
        }
    } else {
        for pc in &path_conditions {
            referenced.extend(collect_w_names_ast(&pc.formula));
        }
        close_under_load_def_deps(&mut referenced, load_defs);
    }

    let strip_words = hide.num_account_children.hide_words()
        || !hide.data_len_children.is_off()
        || hide.any_flag_words();
    if strip_words {
        let mut keep: HashSet<String> = referenced
            .iter()
            .filter(|n| !hide_word_name(n, hide, load_defs))
            .cloned()
            .collect();
        // Mixed constraints still name data_len children; keep those defs.
        if !hide.data_len_children.is_off() {
            for pc in &path_conditions {
                for n in collect_w_names_ast(&pc.formula) {
                    if data_len_child_account(&n)
                        .is_some_and(|i| hide.data_len_children.includes(i))
                    {
                        keep.insert(n);
                    }
                }
            }
        }
        close_under_load_def_deps(&mut keep, load_defs);
        referenced = keep;
    }

    load_defs
        .iter()
        .filter(|def| referenced.contains(&def.name))
        .collect()
}

#[cfg(test)]
mod tests;
