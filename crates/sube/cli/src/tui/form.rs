use ratatui::prelude::*;
use ratatui::widgets::*;
use sube::scales::{Registry, TypeDef, TypeId};

use super::types;

/// A form field that knows its type and can render accordingly.
pub enum Field {
    /// Plain text input (numbers, hex, strings).
    Text {
        name: String,
        type_desc: String,
        input: String,
        /// If true, plain text is auto-encoded to hex bytes on output.
        is_bytes: bool,
    },
    /// Boolean toggle.
    Bool {
        name: String,
        value: bool,
    },
    /// Enum with selectable variants. Each variant may have sub-fields.
    Enum {
        name: String,
        variants: Vec<String>,
        selected: usize,
        /// Sub-fields for the currently selected variant.
        sub_fields: Vec<Field>,
        /// TypeId of the enum, for resolving variant fields.
        ty_id: TypeId,
    },
    /// Growable list (Vec/Sequence). Each entry has the same field template.
    List {
        name: String,
        item_ty: TypeId,
        items: Vec<Vec<Field>>,
    },
}

impl Field {
    fn name(&self) -> &str {
        match self {
            Field::Text { name, .. }
            | Field::Bool { name, .. }
            | Field::Enum { name, .. }
            | Field::List { name, .. } => name,
        }
    }

    /// Produce the text-format value for sube submission.
    #[allow(clippy::only_used_in_recursion)]
    fn to_value(&self, registry: &Registry) -> String {
        match self {
            Field::Text { input, is_bytes, .. } => {
                if *is_bytes && !input.starts_with("0x") {
                    // Auto-encode plain string to hex bytes
                    format!("0x{}", hex::encode(input.as_bytes()))
                } else {
                    input.clone()
                }
            }
            Field::Bool { value, .. } => {
                if *value { "true".into() } else { "false".into() }
            }
            Field::Enum {
                variants,
                selected,
                sub_fields,
                ..
            } => {
                let variant = &variants[*selected];
                if sub_fields.is_empty() {
                    variant.clone()
                } else if sub_fields.len() == 1 {
                    format!("{}({})", variant, sub_fields[0].to_value(registry))
                } else {
                    let parts: Vec<String> = sub_fields
                        .iter()
                        .map(|f| format!("{}:{}", f.name(), f.to_value(registry)))
                        .collect();
                    format!("{}({})", variant, parts.join(";"))
                }
            }
            Field::List { items, .. } => {
                let parts: Vec<String> = items
                    .iter()
                    .map(|item_fields| {
                        if item_fields.len() == 1 {
                            item_fields[0].to_value(registry)
                        } else {
                            let inner: Vec<String> = item_fields
                                .iter()
                                .map(|f| format!("{}:{}", f.name(), f.to_value(registry)))
                                .collect();
                            format!("({})", inner.join(";"))
                        }
                    })
                    .collect();
                format!("..({})", parts.join(";"))
            }
        }
    }

    /// Count total visible rows for layout.
    fn row_count(&self) -> usize {
        match self {
            Field::Text { .. } | Field::Bool { .. } => 1,
            Field::Enum { sub_fields, .. } => {
                1 + sub_fields.iter().map(|f| f.row_count()).sum::<usize>()
            }
            Field::List { items, .. } => {
                1 + items
                    .iter()
                    .map(|item| item.iter().map(|f| f.row_count()).sum::<usize>())
                    .sum::<usize>()
            }
        }
    }
}

/// Build a field from a type, using the registry to resolve complex types.
pub fn field_from_type(name: &str, ty_id: TypeId, registry: &Registry) -> Field {
    match registry.resolve(ty_id) {
        Some(TypeDef::Bool) => Field::Bool {
            name: name.into(),
            value: false,
        },
        Some(TypeDef::Variant(vdef)) => {
            let variants: Vec<String> = vdef.variants().map(|v| v.name().to_string()).collect();
            let sub_fields = if !variants.is_empty() {
                vdef.variants().next().map(|v| variant_sub_fields(&v, registry)).unwrap_or_default()
            } else {
                vec![]
            };
            Field::Enum {
                name: name.into(),
                variants,
                selected: 0,
                sub_fields,
                ty_id,
            }
        }
        Some(TypeDef::Bytes) => Field::Text {
            name: name.into(),
            type_desc: "Vec<u8> (text auto-encodes to hex)".into(),
            input: String::new(),
            is_bytes: true,
        },
        Some(TypeDef::Sequence(inner)) => {
            // Vec<u8> is Bytes, but Sequence(u8) might also appear
            if matches!(registry.resolve(inner), Some(TypeDef::U8)) {
                Field::Text {
                    name: name.into(),
                    type_desc: "Vec<u8> (text auto-encodes to hex)".into(),
                    input: String::new(),
                    is_bytes: true,
                }
            } else {
                Field::List {
                    name: name.into(),
                    item_ty: inner,
                    items: vec![],
                }
            }
        }
        _ => Field::Text {
            name: name.into(),
            type_desc: types::describe(ty_id, registry),
            input: String::new(),
            is_bytes: false,
        },
    }
}

fn variant_sub_fields(
    variant: &sube::scales::Variant,
    registry: &Registry,
) -> Vec<Field> {
    match variant.fields() {
        sube::scales::Fields::Unit => vec![],
        sube::scales::Fields::NewType(ty_id) => {
            vec![field_from_type("value", ty_id, registry)]
        }
        sube::scales::Fields::Tuple(ids) => ids
            .iter()
            .enumerate()
            .map(|(i, id)| field_from_type(&format!("field{i}"), *id, registry))
            .collect(),
        sube::scales::Fields::Struct(fields) => fields
            .iter()
            .map(|f| field_from_type(f.name, f.ty, registry))
            .collect(),
    }
}

/// Form state with type-aware fields.
pub struct FormState {
    pub fields: Vec<Field>,
    /// Flat cursor into the visible field list.
    pub cursor: usize,
    registry: sube::Rc<sube::Registry>,
}

impl FormState {
    pub fn new(fields: Vec<Field>, registry: sube::Rc<sube::Registry>) -> Self {
        FormState {
            fields,
            cursor: 0,
            registry,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Get the field at the current flat cursor position.
    fn field_at_cursor_mut(&mut self) -> Option<&mut Field> {
        let mut pos = 0;
        for field in &mut self.fields {
            if let Some(f) = find_field_at(field, self.cursor, &mut pos) {
                return Some(f);
            }
        }
        None
    }

    fn total_rows(&self) -> usize {
        self.fields.iter().map(|f| f.row_count()).sum()
    }

    pub fn is_on_last(&self) -> bool {
        self.fields.is_empty() || self.cursor + 1 >= self.total_rows()
    }

    pub fn next_field(&mut self) {
        let total = self.total_rows();
        if self.cursor + 1 < total {
            self.cursor += 1;
        }
    }

    pub fn prev_field(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn push_char(&mut self, c: char) {
        if let Some(Field::Text { input, .. }) = self.field_at_cursor_mut() {
            input.push(c);
        }
    }

    pub fn backspace(&mut self) {
        if let Some(Field::Text { input, .. }) = self.field_at_cursor_mut() {
            input.pop();
        }
    }

    /// Toggle bool or cycle enum variant forward.
    pub fn toggle(&mut self) {
        self.cycle_enum(true);
    }

    /// Toggle enum backward.
    pub fn toggle_back(&mut self) {
        self.cycle_enum(false);
    }

    fn cycle_enum(&mut self, forward: bool) {
        let action = {
            let mut pos = 0;
            let mut result = CycleAction::None;
            for field in &self.fields {
                if let Some(a) = find_cycle_action(field, self.cursor, &mut pos, forward) {
                    result = a;
                    break;
                }
            }
            result
        };

        match action {
            CycleAction::ToggleBool => {
                if let Some(Field::Bool { value, .. }) = self.field_at_cursor_mut() {
                    *value = !*value;
                }
            }
            CycleAction::CycleEnum(ty_id, new_selected) => {
                let new_sub = resolve_variant_sub_fields(ty_id, new_selected, &self.registry);
                if let Some(Field::Enum { selected, sub_fields, .. }) = self.field_at_cursor_mut() {
                    *selected = new_selected;
                    *sub_fields = new_sub;
                }
            }
            CycleAction::None => {}
        }
    }

    /// Add an item to a list field at cursor.
    pub fn add_list_item(&mut self) {
        // Extract item_ty without holding mutable borrow
        let item_ty = {
            let mut pos = 0;
            let mut result = None;
            for field in &self.fields {
                if let Some(f) = find_list_ty(field, self.cursor, &mut pos) {
                    result = Some(f);
                    break;
                }
            }
            result
        };
        if let Some(ty) = item_ty {
            let new_item = vec![field_from_type("item", ty, &self.registry)];
            if let Some(Field::List { items, .. }) = self.field_at_cursor_mut() {
                items.push(new_item);
            }
        }
    }

    /// Remove the last item from a list field at cursor.
    pub fn remove_list_item(&mut self) {
        if let Some(Field::List { items, .. }) = self.field_at_cursor_mut() {
            items.pop();
        }
    }

    /// Produce (field_name, text_value) pairs for submission.
    pub fn values(&self) -> Vec<String> {
        self.fields
            .iter()
            .map(|f| f.to_value(&self.registry))
            .collect()
    }

    pub fn field_names(&self) -> Vec<String> {
        self.fields.iter().map(|f| f.name().to_string()).collect()
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        if self.fields.is_empty() {
            return;
        }

        let mut y = area.y;
        let mut flat_idx = 0;
        for field in &self.fields {
            if y >= area.y + area.height {
                break;
            }
            y = render_field(f, field, area.x, y, area.width, area.y + area.height, &mut flat_idx, self.cursor);
        }
    }
}

fn resolve_variant_sub_fields(
    ty_id: TypeId,
    selected: usize,
    registry: &Registry,
) -> Vec<Field> {
    if let Some(TypeDef::Variant(vdef)) = registry.resolve(ty_id) {
        if let Some(variant) = vdef.variants().nth(selected) {
            return variant_sub_fields(&variant, registry);
        }
    }
    vec![]
}

/// Find the mutable field at a flat cursor position.
fn find_field_at<'a>(field: &'a mut Field, target: usize, pos: &mut usize) -> Option<&'a mut Field> {
    if *pos == target {
        return Some(field);
    }
    *pos += 1;
    match field {
        Field::Enum { sub_fields, .. } => {
            for sf in sub_fields.iter_mut() {
                if let Some(f) = find_field_at(sf, target, pos) {
                    return Some(f);
                }
            }
        }
        Field::List { items, .. } => {
            for item in items.iter_mut() {
                for sf in item.iter_mut() {
                    if let Some(f) = find_field_at(sf, target, pos) {
                        return Some(f);
                    }
                }
            }
        }
        _ => {}
    }
    None
}

/// Determine what toggle/cycle action to take at a cursor position.
enum CycleAction {
    ToggleBool,
    CycleEnum(TypeId, usize),
    None,
}

fn find_cycle_action(field: &Field, target: usize, pos: &mut usize, forward: bool) -> Option<CycleAction> {
    if *pos == target {
        return Some(match field {
            Field::Bool { .. } => CycleAction::ToggleBool,
            Field::Enum { ty_id, selected, variants, .. } if !variants.is_empty() => {
                let new = if forward {
                    (*selected + 1) % variants.len()
                } else if *selected == 0 {
                    variants.len() - 1
                } else {
                    *selected - 1
                };
                CycleAction::CycleEnum(*ty_id, new)
            }
            _ => CycleAction::None,
        });
    }
    *pos += 1;
    match field {
        Field::Enum { sub_fields, .. } => {
            for sf in sub_fields {
                if let Some(a) = find_cycle_action(sf, target, pos, forward) {
                    return Some(a);
                }
            }
        }
        Field::List { items, .. } => {
            for item in items {
                for sf in item {
                    if let Some(a) = find_cycle_action(sf, target, pos, forward) {
                        return Some(a);
                    }
                }
            }
        }
        _ => {}
    }
    None
}

/// Find the item_ty of a List field at cursor.
fn find_list_ty(field: &Field, target: usize, pos: &mut usize) -> Option<TypeId> {
    if *pos == target {
        if let Field::List { item_ty, .. } = field {
            return Some(*item_ty);
        }
    }
    *pos += 1;
    match field {
        Field::Enum { sub_fields, .. } => {
            for sf in sub_fields {
                if let Some(ty) = find_list_ty(sf, target, pos) {
                    return Some(ty);
                }
            }
        }
        Field::List { items, .. } => {
            for item in items {
                for sf in item {
                    if let Some(ty) = find_list_ty(sf, target, pos) {
                        return Some(ty);
                    }
                }
            }
        }
        _ => {}
    }
    None
}

/// Render a field recursively, returning the next y position.
#[allow(clippy::too_many_arguments)]
fn render_field(
    f: &mut Frame,
    field: &Field,
    x: u16,
    y: u16,
    width: u16,
    max_y: u16,
    flat_idx: &mut usize,
    cursor: usize,
) -> u16 {
    if y >= max_y {
        return y;
    }

    let is_focused = *flat_idx == cursor;
    let style = if is_focused {
        Style::default().fg(Color::Yellow).bold()
    } else {
        Style::default()
    };
    let cursor_char = if is_focused { "█" } else { "" };

    let area = Rect::new(x, y, width, 1);

    match field {
        Field::Text {
            name,
            type_desc,
            input,
            ..
        } => {
            let text = format!("{name} ({type_desc}): {input}{cursor_char}");
            f.render_widget(Paragraph::new(text).style(style), area);
            *flat_idx += 1;
            y + 1
        }
        Field::Bool { name, value } => {
            let indicator = if *value { "[✓]" } else { "[ ]" };
            let text = format!("{name}: {indicator}  (Space to toggle)");
            f.render_widget(Paragraph::new(text).style(style), area);
            *flat_idx += 1;
            y + 1
        }
        Field::Enum {
            name,
            variants,
            selected,
            sub_fields,
            ..
        } => {
            let variant_name = variants.get(*selected).map(|s| s.as_str()).unwrap_or("?");
            let text = format!(
                "{name}: ◂ {variant_name} ▸  ({}/{})",
                selected + 1,
                variants.len()
            );
            f.render_widget(Paragraph::new(text).style(style), area);
            *flat_idx += 1;
            let mut next_y = y + 1;
            for sf in sub_fields {
                next_y = render_field(f, sf, x + 2, next_y, width.saturating_sub(2), max_y, flat_idx, cursor);
            }
            next_y
        }
        Field::List {
            name,
            items,
            ..
        } => {
            let hint = if is_focused { "  (+add  -remove)" } else { "" };
            let text = format!("{name}: [{} items]{hint}", items.len());
            f.render_widget(Paragraph::new(text).style(style), area);
            *flat_idx += 1;
            let mut next_y = y + 1;
            for (i, item_fields) in items.iter().enumerate() {
                if next_y >= max_y {
                    break;
                }
                // Item header
                let item_area = Rect::new(x + 1, next_y, width.saturating_sub(1), 1);
                f.render_widget(
                    Paragraph::new(format!("  [{}]", i)).fg(Color::DarkGray),
                    item_area,
                );
                next_y += 1;
                for sf in item_fields {
                    next_y = render_field(f, sf, x + 4, next_y, width.saturating_sub(4), max_y, flat_idx, cursor);
                }
            }
            next_y
        }
    }
}
