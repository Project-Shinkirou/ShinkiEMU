use super::*;

#[test]
fn tokenizer_matches_the_observed_engine_space_tab_contract() {
    let tokens = tokenize_operands(b"one, two\t\"three four\" 'five'", 0).unwrap();
    assert_eq!(tokens, vec!["one,", "two", "\"three", "four\"", "'five'"]);
}

#[test]
fn tokenizer_preserves_empty_positional_operands() {
    assert_eq!(
        tokenize_operands(b"100   body", 0).unwrap(),
        vec!["100", "", "", "body"]
    );
    assert_eq!(
        tokenize_operands(b"100\t\tspeaker\tbody", 0).unwrap(),
        vec!["100", "", "speaker", "body"]
    );
    assert_eq!(tokenize_operands(b"", 0).unwrap(), Vec::<String>::new());
}
