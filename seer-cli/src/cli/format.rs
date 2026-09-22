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

pub(crate) fn run_card(row: &RunRow) -> String {
    let parent = row
        .parent_id
        .map(|p| p.to_string())
        .unwrap_or_else(|| "—".into());
    format!(
        "run {}\n  parent {parent} · {} · {}\n  status {}\nnext: seer show {}\nnext: seer run --from {} --account <PUBKEY> --lamports 0\nnext: seer ls\n",
        row.id,
        row.source,
        patch_summary(&row.patches),
        status_text(row),
        row.id,
        row.id
    )
}

pub(crate) fn ls_text(runs: &[RunRow], tree: bool) -> String {
    if runs.is_empty() {
        return "no runs yet\n".into();
    }
    if tree {
        return ls_tree(runs);
    }
    let mut out =
        String::from("id   parent  source                    patch                    status\n");
    for r in runs {
        let parent = r
            .parent_id
            .map(|p| p.to_string())
            .unwrap_or_else(|| "—".into());
        out.push_str(&format!(
            "{:<4} {:<7} {:<25} {:<24} {}\n",
            r.id,
            parent,
            trunc(&r.source, 25),
            trunc(&patch_summary(&r.patches), 24),
            status_text(r)
        ));
    }
    out
}

fn ls_tree(runs: &[RunRow]) -> String {
    let ids: std::collections::BTreeSet<i64> = runs.iter().map(|r| r.id).collect();
    let by_id: std::collections::BTreeMap<i64, &RunRow> = runs.iter().map(|r| (r.id, r)).collect();
    let mut kids: std::collections::BTreeMap<Option<i64>, Vec<i64>> =
        std::collections::BTreeMap::new();
    for r in runs {
        let parent = r.parent_id.filter(|p| ids.contains(p));
        kids.entry(parent).or_default().push(r.id);
    }
    let mut out = String::new();
    fn walk(
        id: i64,
        depth: usize,
        kids: &std::collections::BTreeMap<Option<i64>, Vec<i64>>,
        by_id: &std::collections::BTreeMap<i64, &RunRow>,
        out: &mut String,
    ) {
        let Some(row) = by_id.get(&id) else {
            return;
        };
        let label = match patch_summary(&row.patches).as_str() {
            "—" => row.source.clone(),
            other => other.to_string(),
        };
        out.push_str(&format!(
            "{}{}  {}  {}\n",
            "  ".repeat(depth),
            row.id,
            label,
            status_text(row)
        ));
        if let Some(cs) = kids.get(&Some(id)) {
            for c in cs {
                walk(*c, depth.saturating_add(1), kids, by_id, out);
            }
        }
    }
    if let Some(roots) = kids.get(&None) {
        for id in roots {
            walk(*id, 0, &kids, &by_id, &mut out);
        }
    }
    out
}

pub(crate) fn diff_text(a: &RunRow, b: &RunRow) -> String {
    let mut out = format!("{} vs {}\n", a.id, b.id);
    let pa = a
        .parent_id
        .map(|p| p.to_string())
        .unwrap_or_else(|| "—".into());
    let pb = b
        .parent_id
        .map(|p| p.to_string())
        .unwrap_or_else(|| "—".into());
    out.push_str(&format!("parent     {pa}    {pb}\n"));
    out.push_str(&format!("source     {}    {}\n", a.source, b.source));
    out.push_str(&format!(
        "patches    {}    {}\n",
        patch_summary(&a.patches),
        patch_summary(&b.patches)
    ));
    out.push_str(&format!(
        "environment  {}    {}\n",
        a.environment, b.environment
    ));
    out.push_str(&format!(
        "status     {}    {}\n",
        status_text(a),
        status_text(b)
    ));
    out.push_str(&format!(
        "tx         {}\n",
        if a.transaction_blob_hash == b.transaction_blob_hash {
            "same"
        } else {
            "different"
        }
    ));
    out.push_str(&format!(
        "state      {}\n",
        if a.state_blob_hash == b.state_blob_hash {
            "same"
        } else {
            "different"
        }
    ));
    out
}

pub(crate) fn state_listing(id: i64, state: &StateAccounts) -> (String, serde_json::Value) {
    let mut accounts = Vec::new();
    let mut text = format!("accounts {}\n", state.0.len());
    for (pk, acct) in &state.0 {
        accounts.push(serde_json::json!({
            "pubkey": pk.to_string(),
            "lamports": acct.lamports.to_string(),
            "owner": acct.owner.to_string(),
            "executable": acct.executable,
        }));
        text.push_str(&format!(
            "  {pk}  lamports={}  owner={}{}\n",
            acct.lamports,
            acct.owner,
            if acct.executable { "  executable" } else { "" }
        ));
    }
    text.push_str(&format!("next: seer show {id} --account <PUBKEY>\n"));
    (text, serde_json::Value::Array(accounts))
}

pub(crate) fn status_text(row: &RunRow) -> String {
    match (row.status.as_str(), row.error.as_deref()) {
        ("finished", None) => "ok".into(),
        ("finished", Some(e)) => format!("error: {e}"),
        (s, Some(e)) => format!("{s}: {e}"),
        (s, None) => s.into(),
    }
}

fn patch_summary(patches: &str) -> String {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str(patches) else {
        return patches.to_string();
    };
    if items.is_empty() {
        return "—".into();
    }
    items.iter().map(one_patch).collect::<Vec<_>>().join(" ")
}

fn one_patch(v: &serde_json::Value) -> String {
    let acct = v.get("account").and_then(|a| a.as_str()).unwrap_or("?");
    let short = if acct.len() > 8 {
        format!("{}…", &acct[..8])
    } else {
        acct.to_string()
    };
    let mut bits = Vec::new();
    if let Some(n) = v.get("lamports") {
        bits.push(format!("lamports={n}"));
    }
    if let Some(n) = v.get("owner") {
        bits.push(format!("owner={n}"));
    }
    if v.get("data").and_then(|d| d.as_bool()).unwrap_or(false) {
        bits.push("data".into());
    }
    if let Some(n) = v.get("executable") {
        bits.push(format!("executable={n}"));
    }
    if bits.is_empty() {
        short
    } else {
        format!("{short}.{}", bits.join(","))
    }
}

fn trunc(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    format!("{}…", &s[..n.saturating_sub(1)])
}
