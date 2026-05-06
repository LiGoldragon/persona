use persona::schema::{HarnessName, PersonaDocument};

struct SchemaFixture;

impl SchemaFixture {
    fn encoded_example_document() -> String {
        PersonaDocument::example().to_nota().unwrap()
    }
}

#[test]
fn example_document_round_trips_as_nota() {
    let document = PersonaDocument::example();
    let encoded = document.to_nota().unwrap();
    let recovered = PersonaDocument::from_nota(&encoded).unwrap();

    assert_eq!(recovered, document);
    assert!(encoded.starts_with("(PersonaDocument ["));
}

#[test]
fn harness_name_is_a_transparent_nota_value() {
    let name = HarnessName::new("operator");

    assert_eq!(name.as_str(), "operator");
}

#[test]
fn example_document_contains_initial_message_object() {
    let encoded = SchemaFixture::encoded_example_document();

    assert!(encoded.contains("(MessageRecord \"message-1\" \"operator\" \"designer\""));
    assert!(encoded.contains("(AuthorizationRecord \"delivery-1\" \"message-1\" Allow"));
    assert!(encoded.contains("(StateTransitionRecord \"transition-1\" AppendMessage"));
    assert!(encoded.contains("(PersonaStateSnapshot 1 Running"));
}
