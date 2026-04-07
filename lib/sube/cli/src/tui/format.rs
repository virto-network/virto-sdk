pub fn fuzzy_match(query: &str, target: &str) -> bool {
    let target_lower = target.to_lowercase();
    let mut chars = target_lower.chars();
    for qc in query.chars() {
        if chars.by_ref().find(|&c| c == qc).is_none() {
            return false;
        }
    }
    true
}

pub fn format_response(resp: sube::Response) -> String {
    match resp {
        sube::Response::None => "(none)".into(),
        sube::Response::Value(entry, meta) => entry
            .to_text(&meta.registry)
            .unwrap_or_else(|e| format!("error: {e}")),
        sube::Response::ValueSet(items, meta) => {
            let mut out = String::new();
            for (keys, value) in items {
                let key_strs: Vec<String> = keys
                    .iter()
                    .filter_map(|k| k.to_text(&meta.registry).ok())
                    .collect();
                let key_display = key_strs.join(", ");
                match value {
                    Some(v) => {
                        let text = v
                            .to_text(&meta.registry)
                            .unwrap_or_else(|e| format!("error: {e}"));
                        out.push_str(&format!("[{key_display}] {text}\n"));
                    }
                    None => out.push_str(&format!("[{key_display}] (none)\n")),
                }
            }
            out
        }
        sube::Response::Void => "(submitted)".into(),
        sube::Response::Meta(_) => "(metadata)".into(),
    }
}
