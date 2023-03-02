use alloc::string::String;

pub fn to_camel(term: &str) -> String {
    let underscore_count = term.chars().filter(|c| *c == '-').count();
    let mut result = String::with_capacity(term.len() - underscore_count);
    let mut at_new_word = true;

    for c in term.chars().skip_while(|&c| c == '-') {
        if c == '-' {
            at_new_word = true;
        } else if at_new_word {
            result.push(c.to_ascii_uppercase());
            at_new_word = false;
        } else {
            result.push(c);
        }
    }
    result
}
