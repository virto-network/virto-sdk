use serde_json::Value as JsonValue;

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

/// Filter out routine system events that appear in every block.
pub fn is_interesting_event(event: &JsonValue) -> bool {
    let event_body = match event.get("event") {
        Some(e) => e,
        None => return false,
    };
    let pallet = match event_body.as_object().and_then(|o| o.keys().next()) {
        Some(p) => p,
        None => return false,
    };
    !matches!(
        pallet.as_str(),
        "System" | "ParachainSystem" | "TransactionPayment" | "MessageQueue" | "CumulusXcm"
    )
}

/// Format block events JSON into readable detail text.
pub fn format_events_detail(json: &JsonValue) -> String {
    let events = match json.as_array() {
        Some(arr) => arr,
        None => return "(invalid events)".into(),
    };

    let mut out = String::new();
    for (i, event) in events.iter().enumerate() {
        let phase = match event.get("phase") {
            Some(p) => {
                if let Some(n) = p.get("ApplyExtrinsic") {
                    format!("extrinsic #{n}")
                } else if p.get("Initialization").is_some() {
                    "initialization".into()
                } else if p.get("Finalization").is_some() {
                    "finalization".into()
                } else {
                    format!("{p}")
                }
            }
            None => "?".into(),
        };

        let (pallet, event_name, fields) = match event.get("event").and_then(|e| e.as_object()) {
            Some(obj) => {
                if let Some((pallet, inner)) = obj.iter().next() {
                    match inner.as_object().and_then(|o| o.iter().next()) {
                        Some((name, data)) => (pallet.as_str(), name.as_str(), Some(data)),
                        None => (pallet.as_str(), "?", None),
                    }
                } else {
                    ("?", "?", None)
                }
            }
            None => ("?", "?", None),
        };

        out.push_str(&format!(
            "{}. {} » {pallet}::{event_name}\n",
            i + 1,
            phase
        ));

        if let Some(data) = fields {
            if let Some(obj) = data.as_object() {
                for (key, val) in obj {
                    out.push_str(&format!("   {key}: {}\n", format_value(val)));
                }
            } else {
                let val_str = format_value(data);
                if val_str != "null" {
                    out.push_str(&format!("   {val_str}\n"));
                }
            }
        }
        out.push('\n');
    }
    out
}

/// Format a JSON value concisely for display.
pub fn format_value(val: &JsonValue) -> String {
    match val {
        JsonValue::String(s) => {
            if s.starts_with("0x") && s.len() > 20 {
                format!("{}…{}", &s[..10], &s[s.len() - 8..])
            } else {
                s.clone()
            }
        }
        JsonValue::Number(n) => n.to_string(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Null => "null".into(),
        JsonValue::Object(obj) => {
            if obj.len() == 1 {
                let (k, v) = obj.iter().next().unwrap();
                let v_str = format_value(v);
                if v_str == "null" {
                    k.clone()
                } else {
                    format!("{k}({v_str})")
                }
            } else if obj.len() <= 3 {
                let parts: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| format!("{k}: {}", format_value(v)))
                    .collect();
                format!("{{ {} }}", parts.join(", "))
            } else {
                format!("{{ {} fields }}", obj.len())
            }
        }
        JsonValue::Array(arr) => {
            if arr.len() <= 3 {
                let parts: Vec<String> = arr.iter().map(format_value).collect();
                format!("[{}]", parts.join(", "))
            } else {
                format!("[{} items]", arr.len())
            }
        }
    }
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
