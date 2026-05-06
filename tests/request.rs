use persona::request::{CommandLine, DescribeSchema, PersonaOutput, PersonaRequest};

struct RequestFixture;

impl RequestFixture {
    fn inline_validate_object() -> CommandLine {
        CommandLine::from_arguments([
            "(ValidateObject",
            "(HarnessRecord",
            "operator",
            "Operator",
            "Terminal",
            "\"codex\"))",
        ])
    }
}

#[test]
fn empty_command_line_describes_schema() {
    let request = CommandLine::from_arguments(std::iter::empty::<&str>())
        .decode_request()
        .unwrap();

    assert_eq!(request, PersonaRequest::DescribeSchema(DescribeSchema {}));
}

#[test]
fn inline_nota_request_decodes_after_shell_token_join() {
    let request = RequestFixture::inline_validate_object()
        .decode_request()
        .unwrap();

    match request {
        PersonaRequest::ValidateObject(_) => {}
        other => panic!("expected ValidateObject, got {other:?}"),
    }
}

#[test]
fn describe_schema_outputs_nota() {
    let output = PersonaRequest::DescribeSchema(DescribeSchema {})
        .into_output()
        .to_nota()
        .unwrap();

    assert!(output.starts_with("(SchemaExample"));
    assert!(output.contains("(PersonaDocument ["));
}

#[test]
fn output_round_trips_through_nota() {
    let output = PersonaRequest::DescribeSchema(DescribeSchema {}).into_output();
    let encoded = output.to_nota().unwrap();
    let mut decoder = nota_codec::Decoder::nota(&encoded);
    let recovered = <PersonaOutput as nota_codec::NotaDecode>::decode(&mut decoder).unwrap();

    assert_eq!(recovered, output);
}
