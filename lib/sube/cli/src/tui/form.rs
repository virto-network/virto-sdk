use ratatui::prelude::*;
use ratatui::widgets::*;

/// Dynamic form state for entering key/call parameters.
pub struct FormState {
    /// (field_name, type_description) pairs
    fields: Vec<(String, String)>,
    /// Current input value for each field
    inputs: Vec<String>,
    /// Currently focused field index
    focus: usize,
}

impl FormState {
    pub fn new(fields: Vec<(String, String)>) -> Self {
        let len = fields.len();
        FormState {
            fields,
            inputs: vec![String::new(); len],
            focus: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn is_on_last(&self) -> bool {
        self.fields.is_empty() || self.focus >= self.fields.len() - 1
    }

    pub fn next_field(&mut self) {
        if self.focus + 1 < self.fields.len() {
            self.focus += 1;
        }
    }

    pub fn prev_field(&mut self) {
        if self.focus > 0 {
            self.focus -= 1;
        }
    }

    pub fn push_char(&mut self, c: char) {
        if let Some(input) = self.inputs.get_mut(self.focus) {
            input.push(c);
        }
    }

    pub fn backspace(&mut self) {
        if let Some(input) = self.inputs.get_mut(self.focus) {
            input.pop();
        }
    }

    pub fn values(&self) -> Vec<String> {
        self.inputs.clone()
    }

    pub fn field_names(&self) -> Vec<String> {
        self.fields.iter().map(|(n, _)| n.clone()).collect()
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        if self.fields.is_empty() {
            return;
        }

        let constraints: Vec<Constraint> = self
            .fields
            .iter()
            .map(|_| Constraint::Length(2))
            .collect();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        for (i, ((name, type_desc), input)) in
            self.fields.iter().zip(self.inputs.iter()).enumerate()
        {
            if i >= chunks.len() {
                break;
            }
            let label_style = if i == self.focus {
                Style::default().fg(Color::Yellow).bold()
            } else {
                Style::default()
            };

            let display = if i == self.focus {
                format!("{name} ({type_desc}): {input}█")
            } else {
                format!("{name} ({type_desc}): {input}")
            };

            f.render_widget(Paragraph::new(display).style(label_style), chunks[i]);
        }
    }
}
