use crate::state_accounts::StateAccounts;
use storage::RunRow;

pub(crate) fn run_json(row: &RunRow) -> serde_json::Value {
    serde_json::json!({
        "id": row.id,
        "parent": row.parent_id,
        "source": row.source,
        "patches": serde_json::from_str::<serde_json::Value>(&row.patches).unwrap_or(serde_json::json!([])),
        "environment": serde_json::from_str::<serde_json::Value>(&row.environment).unwrap_or(serde_json::json!({})),
        "status": status_text(row),
        "error": row.error,
        "run_at": row.run_at,
    })
}

pub(crate) fn state_catalog(state: &StateAccounts) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (pk, acct) in &state.0 {
        map.insert(
            pk.to_string(),
            serde_json::json!({
                "lamports": acct.lamports.to_string(),
                "owner": acct.owner.to_string(),
                "executable": acct.executable,
            }),
        );
    }
    serde_json::Value::Object(map)
}

pub(crate) fn ls_select(
    runs: &[RunRow],
    from: Option<i64>,
    tree: bool,
    status: Option<LsStatus>,
) -> Vec<RunRow> {
    let mut out: Vec<RunRow> = match from {
        None => runs.to_vec(),
        Some(id) if tree => {
            let keep = descendant_ids(runs, id);
            runs.iter()
                .filter(|r| keep.contains(&r.id))
                .cloned()
                .collect()
        }
        Some(id) => runs
            .iter()
            .filter(|r| r.parent_id == Some(id))
            .cloned()
            .collect(),
    };
    if let Some(want) = status {
        out.retain(|r| status_matches(r, want));
    }
    out
}

fn descendant_ids(runs: &[RunRow], root: i64) -> std::collections::BTreeSet<i64> {
    let mut keep = std::collections::BTreeSet::from([root]);
    let mut grew = true;
    while grew {
        grew = false;
        for r in runs {
            if let Some(p) = r.parent_id {
                if keep.contains(&p) && keep.insert(r.id) {
                    grew = true;
                }
            }
        }
    }
    keep
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum LsStatus {
    Ok,
    Error,
}

fn status_matches(row: &RunRow, want: LsStatus) -> bool {
    match want {
        LsStatus::Ok => row.error.is_none() && status_text(row) == "ok",
        LsStatus::Error => row.error.is_some(),
    }
}

pub(crate) fn ls_tree_json(runs: &[RunRow]) -> Vec<serde_json::Value> {
    let ids: std::collections::BTreeSet<i64> = runs.iter().map(|r| r.id).collect();
    let by_id: std::collections::BTreeMap<i64, &RunRow> = runs.iter().map(|r| (r.id, r)).collect();
    let mut kids: std::collections::BTreeMap<Option<i64>, Vec<i64>> =
        std::collections::BTreeMap::new();
    for r in runs {
        let parent = r.parent_id.filter(|p| ids.contains(p));
        kids.entry(parent).or_default().push(r.id);
    }
    for sibs in kids.values_mut() {
        sibs.sort_by(|a, b| b.cmp(a));
    }
    let roots = kids.get(&None).cloned().unwrap_or_default();
    roots
        .into_iter()
        .filter_map(|id| node(id, &kids, &by_id))
        .collect()
}

fn node(
    id: i64,
    kids: &std::collections::BTreeMap<Option<i64>, Vec<i64>>,
    by_id: &std::collections::BTreeMap<i64, &RunRow>,
) -> Option<serde_json::Value> {
    let row = by_id.get(&id)?;
    let children = kids
        .get(&Some(id))
        .into_iter()
        .flatten()
        .filter_map(|c| node(*c, kids, by_id))
        .collect::<Vec<_>>();
    let mut value = run_json(row);
    if let serde_json::Value::Object(map) = &mut value {
        map.insert("children".into(), serde_json::Value::Array(children));
    }
    Some(value)
}

/// `head == 0` means no cap.
pub(crate) fn slice_json_array(
    mut values: Vec<serde_json::Value>,
    skip: usize,
    head: usize,
) -> Vec<serde_json::Value> {
    if skip >= values.len() {
        return Vec::new();
    }
    values.drain(..skip);
    if head != 0 {
        values.truncate(head);
    }
    values
}

/// `head == 0` means no cap. `tail` is exclusive with a non-zero head.
pub(crate) fn slice_skip_head_tail<T>(
    mut values: Vec<T>,
    skip: usize,
    head: usize,
    tail: Option<usize>,
) -> Vec<T> {
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

pub(crate) fn status_text(row: &RunRow) -> String {
    match (row.status.as_str(), row.error.as_deref()) {
        ("finished", None) => "ok".into(),
        ("finished", Some(e)) => format!("error: {e}"),
        (s, Some(e)) => format!("{s}: {e}"),
        (s, None) => s.into(),
    }
}
