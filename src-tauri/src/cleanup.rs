/// Rule-based Fast-mode cleanup of a raw transcript.
pub fn cleanup_fast(text: &str) -> String {
    let collapsed = collapse_whitespace(text);
    if collapsed.is_empty() {
        return String::new();
    }
    let stripped = strip_fillers(&collapsed);
    let collapsed = collapse_whitespace(&stripped);
    if collapsed.is_empty() {
        return String::new();
    }
    let mut out = capitalize_sentences(&collapsed);
    ensure_ending_punctuation(&mut out);
    out
}

fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_space = true;
    for c in text.chars() {
        if c.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

fn is_filler_word(word: &str) -> bool {
    let core: String = word
        .chars()
        .filter(|c| c.is_alphabetic())
        .flat_map(|c| c.to_lowercase())
        .collect();
    if core.is_empty() {
        return false;
    }
    matches!(core.as_str(), "um" | "uh" | "er" | "ah" | "mhm")
        || (core.starts_with("hmm") && core.bytes().all(|b| b == b'h' || b == b'm'))
}

fn strip_fillers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    for c in text.chars() {
        if c.is_alphabetic() {
            word.push(c);
        } else {
            flush_word(&mut word, &mut out);
            out.push(c);
        }
    }
    flush_word(&mut word, &mut out);
    out
}

fn flush_word(word: &mut String, out: &mut String) {
    if word.is_empty() {
        return;
    }
    if !is_filler_word(word) {
        out.push_str(word);
    }
    word.clear();
}

fn capitalize_sentences(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cap_next = true;
    for c in text.chars() {
        if cap_next && c.is_alphabetic() {
            for u in c.to_uppercase() {
                out.push(u);
            }
            cap_next = false;
        } else {
            out.push(c);
            if matches!(c, '.' | '?' | '!') {
                cap_next = true;
            }
        }
    }
    out
}

fn ensure_ending_punctuation(text: &mut String) {
    let has_letter = text.chars().any(|c| c.is_alphabetic());
    if !has_letter {
        return;
    }
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        return;
    }
    let last = trimmed.chars().last();
    if !matches!(last, Some('.' | '?' | '!')) {
        let keep = trimmed.to_string();
        text.clear();
        text.push_str(&keep);
        text.push('.');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_fillers_and_punctuates() {
        let out = cleanup_fast("um hello uh there hmm");
        assert!(out.starts_with('H'));
        assert!(!out.to_lowercase().contains(" um "));
        assert!(out.ends_with('.'));
    }
}
