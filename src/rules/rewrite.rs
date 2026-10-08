//! Propose the rule-grammar spelling of an expression written in SQL style or
//! with an implication arrow, for the one-time move to a single dialect.
//!
//! Only spellings with one meaning are rewritten: `AND`, `OR`, `NOT`,
//! `IS NULL`, `IS NOT NULL`, `<>`, a lone `=`, and `a => b` as `!(a) || (b)`.
//! A proposal is offered only when the result parses in the rule grammar;
//! anything else stays for a person to formalise.

use super::expr;

/// The rule-grammar spelling of `text`, when a mechanical rewrite gets there
/// and changes something.
pub fn propose(text: &str) -> Option<String> {
    let mut out = rewrite_words(text);
    if let Some((left, right)) = split_implication(&out) {
        out = format!("!({}) || ({})", left.trim(), right.trim());
    }
    (out != text && expr::parse(&out).is_ok()).then_some(out)
}

/// Rewrite SQL words and operators outside string literals.
fn rewrite_words(text: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // Copy string literals untouched.
        if c == '\'' || c == '"' {
            let quote = c;
            out.push(c);
            i += 1;
            while i < chars.len() {
                out.push(chars[i]);
                if chars[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if c.is_alphabetic() {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match word.as_str() {
                "AND" => out.push_str("&&"),
                "OR" => out.push_str("||"),
                "IS" => {
                    let (w1, j1) = next_word(&chars, i);
                    if w1 == "NULL" {
                        out.push_str("== null");
                        i = j1;
                    } else if w1 == "NOT" {
                        let save = i;
                        i = j1;
                        let (w2, j2) = next_word(&chars, i);
                        if w2 == "NULL" {
                            out.push_str("!= null");
                            i = j2;
                        } else {
                            i = save;
                            out.push_str(&word);
                        }
                    } else {
                        out.push_str(&word);
                    }
                }
                "NOT" => out.push('!'),
                "NULL" => out.push_str("null"),
                "TRUE" => out.push_str("true"),
                "FALSE" => out.push_str("false"),
                _ => out.push_str(&word),
            }
            continue;
        }
        if c == '<' && chars.get(i + 1) == Some(&'>') {
            out.push_str("!=");
            i += 2;
            continue;
        }
        if c == '=' {
            let prev = if i > 0 { chars[i - 1] } else { ' ' };
            let next = chars.get(i + 1).copied().unwrap_or(' ');
            if !matches!(prev, '=' | '!' | '<' | '>') && !matches!(next, '=' | '>') {
                out.push_str("==");
                i += 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// The word after position `from`, skipping spaces, and the position after it.
fn next_word(chars: &[char], from: usize) -> (String, usize) {
    let mut j = from;
    while j < chars.len() && chars[j] == ' ' {
        j += 1;
    }
    let s = j;
    while j < chars.len() && chars[j].is_alphabetic() {
        j += 1;
    }
    (chars[s..j].iter().collect(), j)
}

/// `a => b` outside string literals, when it appears exactly once.
fn split_implication(text: &str) -> Option<(String, String)> {
    let mut in_str: Option<char> = None;
    let chars: Vec<char> = text.chars().collect();
    let mut at = None;
    for i in 0..chars.len() {
        match (in_str, chars[i]) {
            (None, '\'' | '"') => in_str = Some(chars[i]),
            (Some(q), c) if c == q => in_str = None,
            (None, '=') if chars.get(i + 1) == Some(&'>') && i > 0 && chars[i - 1] != '=' => {
                if at.is_some() {
                    return None;
                }
                at = Some(i);
            }
            _ => {}
        }
    }
    let i = at?;
    Some((chars[..i].iter().collect(), chars[i + 2..].iter().collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_spellings_become_the_rule_grammar() {
        assert_eq!(
            propose("schedule_end IS NULL OR schedule_end >= schedule_date").as_deref(),
            Some("schedule_end == null || schedule_end >= schedule_date")
        );
        assert_eq!(propose("email_from IS NOT NULL AND kind <> 'mail'").as_deref(), Some("email_from != null && kind != 'mail'"));
        assert_eq!(propose("NEW.item_id = OLD.item_id").as_deref(), Some("NEW.item_id == OLD.item_id"));
    }

    #[test]
    fn an_implication_becomes_not_a_or_b() {
        assert_eq!(propose("is_stock_item => uom_id != null").as_deref(), Some("!(is_stock_item) || (uom_id != null)"));
    }

    #[test]
    fn strings_are_left_alone_and_prose_gets_no_proposal() {
        assert_eq!(propose("note = 'a = b AND c'").as_deref(), Some("note == 'a = b AND c'"));
        assert_eq!(propose("a locked purchase order refuses cancel"), None);
        assert_eq!(propose("total_debit == total_credit"), None, "already in the grammar");
    }
}
