use alloc::string::String;

/// Convert a kebab-case or lowercase string to UpperCamelCase.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_from_hyphenated() {
        assert_eq!(to_camel("para-scheduler"), "ParaScheduler");
    }

    #[test]
    fn camel_already_capitalized() {
        assert_eq!(to_camel("System"), "System");
    }

    #[test]
    fn camel_single_char() {
        assert_eq!(to_camel("a"), "A");
    }

    #[test]
    fn camel_lowercase_word() {
        assert_eq!(to_camel("hello"), "Hello");
    }

    #[test]
    fn camel_multiple_hyphens() {
        assert_eq!(to_camel("a-b-c"), "ABC");
    }
}
