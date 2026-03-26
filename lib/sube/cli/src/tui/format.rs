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

/// Format block events JSON into readable detail text, grouped by extrinsic.
pub fn format_events_detail(json: &JsonValue) -> String {
    let events = match json.as_array() {
        Some(arr) => arr,
        None => return "(invalid events)".into(),
    };

    // Group events by phase
    struct EventInfo<'a> {
        pallet: &'a str,
        name: &'a str,
        data: Option<&'a JsonValue>,
    }

    fn parse_event(event: &JsonValue) -> Option<(Phase, EventInfo)> {
        let phase = match event.get("phase") {
            Some(p) if p.get("ApplyExtrinsic").is_some() => {
                Phase::Extrinsic(p["ApplyExtrinsic"].as_u64().unwrap_or(0) as u32)
            }
            Some(p) if p.get("Initialization").is_some() => Phase::Init,
            Some(p) if p.get("Finalization").is_some() => Phase::Finalize,
            _ => return None,
        };
        let obj = event.get("event")?.as_object()?;
        let (pallet, inner) = obj.iter().next()?;
        let (name, data) = match inner.as_object().and_then(|o| o.iter().next()) {
            Some((n, d)) => (n.as_str(), Some(d)),
            None => ("?", None),
        };
        Some((phase, EventInfo { pallet, name, data }))
    }

    #[derive(PartialEq)]
    enum Phase { Init, Extrinsic(u32), Finalize }

    let mut out = String::new();

    // Initialization events
    let init_events: Vec<_> = events.iter().filter_map(|e| {
        let (phase, info) = parse_event(e)?;
        (phase == Phase::Init).then_some(info)
    }).collect();
    if !init_events.is_empty() {
        out.push_str("─── initialization ───\n");
        for e in &init_events {
            format_single_event(&mut out, e.pallet, e.name, e.data);
        }
        out.push('\n');
    }

    // Group by extrinsic index
    let max_ext = events.iter().filter_map(|e| {
        if let Some(n) = e.get("phase").and_then(|p| p.get("ApplyExtrinsic")).and_then(|n| n.as_u64()) {
            Some(n as u32)
        } else { None }
    }).max();

    if let Some(max) = max_ext {
        for ext_idx in 0..=max {
            let ext_events: Vec<_> = events.iter().filter_map(|e| {
                let (phase, info) = parse_event(e)?;
                (phase == Phase::Extrinsic(ext_idx)).then_some(info)
            }).collect();
            if ext_events.is_empty() { continue; }

            // Check if this extrinsic succeeded or failed
            let status = ext_events.iter().find_map(|e| {
                if e.pallet == "System" && e.name == "ExtrinsicSuccess" { Some("✓") }
                else if e.pallet == "System" && e.name == "ExtrinsicFailed" { Some("✗") }
                else { None }
            }).unwrap_or("?");

            // Find the main action (first non-System event)
            let action = ext_events.iter().find(|e| e.pallet != "System" && e.pallet != "TransactionPayment");

            let action_str = action
                .map(|a| format!("{}::{}", a.pallet, a.name))
                .unwrap_or_else(|| "system".into());

            out.push_str(&format!("─── extrinsic #{ext_idx} {status} {action_str} ───\n"));

            for e in &ext_events {
                if e.pallet == "System" && (e.name == "ExtrinsicSuccess" || e.name == "ExtrinsicFailed") {
                    continue; // already shown in header
                }
                format_single_event(&mut out, e.pallet, e.name, e.data);
            }
            out.push('\n');
        }
    }

    // Finalization events
    let fin_events: Vec<_> = events.iter().filter_map(|e| {
        let (phase, info) = parse_event(e)?;
        (phase == Phase::Finalize).then_some(info)
    }).collect();
    if !fin_events.is_empty() {
        out.push_str("─── finalization ───\n");
        for e in &fin_events {
            format_single_event(&mut out, e.pallet, e.name, e.data);
        }
    }

    out
}

fn format_single_event(out: &mut String, pallet: &str, name: &str, data: Option<&JsonValue>) {
    out.push_str(&format!("  {pallet}::{name}\n"));
    if let Some(data) = data {
        if let Some(obj) = data.as_object() {
            for (key, val) in obj {
                out.push_str(&format!("    {key}: {}\n", format_value(val)));
            }
        } else {
            let val_str = format_value(data);
            if val_str != "null" {
                out.push_str(&format!("    {val_str}\n"));
            }
        }
    }
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
