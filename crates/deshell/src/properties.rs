use crate::config::UnknownInterpreter;
use crate::ir::{SourceBytes, TextExpression, TextPart};
use std::collections::BTreeMap;

struct Generator(u64);

impl Generator {
    fn next(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }

    fn bytes(&mut self, maximum: usize) -> Vec<u8> {
        let length = (self.next() as usize) % (maximum + 1);
        (0..length).map(|_| self.next() as u8).collect()
    }

    fn identifier(&mut self) -> String {
        let length = 1 + (self.next() as usize % 24);
        let mut output = String::with_capacity(length);
        output.push((b'a' + (self.next() % 26) as u8) as char);
        for _ in 1..length {
            let alphabet = b"abcdefghijklmnopqrstuvwxyz0123456789_";
            output.push(alphabet[self.next() as usize % alphabet.len()] as char);
        }
        output
    }
}

#[test]
fn generated_ir_round_trips_and_normalization_is_idempotent() {
    let mut generator = Generator(0x1d0f_cafe_babe_0042);
    for case in 0..512 {
        let directory = generator.identifier();
        let filename = generator.identifier();
        let path = format!("{directory}/{filename}.sh");
        let normalized = crate::ir::normalize_path(&path).unwrap();
        assert_eq!(crate::ir::normalize_path(&normalized).unwrap(), normalized);

        let argument = generator.identifier();
        let source = format!("/usr/bin/printf '%s\\n' '{argument}'\n");
        let plan = crate::frontend::lower(&path, source.as_bytes(), UnknownInterpreter::TraceOnly)
            .unwrap_or_else(|error| panic!("generated case {case} failed to lower: {error}"));
        let first = plan.encode_pretty().unwrap();
        let decoded = crate::ir::Plan::decode(&first).unwrap();
        let second = decoded.encode_pretty().unwrap();
        assert_eq!(first, second, "generated case {case}");
    }
}

#[test]
fn generated_expressions_never_reparse_expanded_dollar_text() {
    let mut generator = Generator(0x6f72_6967_696e_0001);
    for _ in 0..1_024 {
        let variable_name = generator.identifier();
        let argument_name = generator.identifier();
        let literal = format!("${variable_name}:");
        let variable_value = format!("${argument_name}");
        let argument_value = format!("$${variable_name}");
        let expression = TextExpression {
            parts: vec![
                TextPart::Literal {
                    value: literal.clone(),
                },
                TextPart::Variable {
                    name: variable_name.clone(),
                },
                TextPart::Argument {
                    name: argument_name.clone(),
                },
            ],
        };
        let variables = BTreeMap::from([(variable_name, variable_value.clone())]);
        let arguments = BTreeMap::from([(argument_name, argument_value.clone())]);
        assert_eq!(
            expression.evaluate(&variables, &arguments).unwrap(),
            literal + &variable_value + &argument_value
        );
    }
}

#[test]
fn generated_node_ids_are_stable_and_domain_separated() {
    let mut generator = Generator(0x6e6f_6465_2d69_6401);
    for preorder in 0..512_u64 {
        let path = format!("{}/{}.sh", generator.identifier(), generator.identifier());
        let start = generator.next() % 10_000;
        let end = start + generator.next() % 1_000;
        let first = crate::ir::node_id(&path, start, end, "exec", preorder).unwrap();
        let second = crate::ir::node_id(&path, start, end, "exec", preorder).unwrap();
        let other_operation = crate::ir::node_id(&path, start, end, "file_read", preorder).unwrap();
        assert_eq!(first, second);
        assert_ne!(first, other_operation);
        assert_eq!(first.len(), 32);
    }
}

#[test]
fn deterministic_byte_fuzzing_preserves_source_and_never_panics() {
    let mut generator = Generator(0x6675_7a7a_2d76_3101);
    let paths = [
        "fuzz/input.sh",
        "fuzz/input.zsh",
        "fuzz/input.fish",
        "fuzz/input.ps1",
        "fuzz/input.cmd",
        "fuzz/input.nu",
        "fuzz/input.unknown",
    ];
    for case in 0..2_048 {
        let bytes = generator.bytes(256);
        let source = SourceBytes::from_bytes(&bytes);
        assert_eq!(source.to_bytes().unwrap(), bytes, "source case {case}");

        let path = paths[case % paths.len()];
        let plan = crate::frontend::lower(path, &bytes, UnknownInterpreter::TraceOnly)
            .unwrap_or_else(|error| panic!("frontend case {case} returned an error: {error}"));
        plan.validate()
            .unwrap_or_else(|errors| panic!("frontend case {case} made invalid IR: {errors:?}"));
        let encoded = plan.encode_pretty().unwrap();
        assert_eq!(crate::ir::Plan::decode(&encoded).unwrap(), plan);

        let response = crate::protocol::handle_message(crate::protocol::AgentKind::Process, &bytes);
        assert!(response.ends_with(b"\n"), "protocol case {case}");
        assert!(response.len() <= crate::protocol::MAX_MESSAGE_BYTES);
        let _: serde_json::Value = crate::strict_json::parse(&response).unwrap();
    }
}

#[test]
fn generated_duplicate_keys_and_traversal_paths_are_rejected() {
    let mut generator = Generator(0x7365_6375_7269_7479);
    for _ in 0..512 {
        let name = generator.identifier();
        let duplicate = format!("{{\"outer\":{{\"{name}\":1,\"{name}\":2}}}}");
        assert!(crate::strict_json::parse(duplicate.as_bytes()).is_err());

        for path in [
            format!("../{name}"),
            format!("{name}/../escape"),
            format!("/{name}"),
            format!("{name}//file"),
            format!("C:/{name}"),
        ] {
            assert!(crate::ir::normalize_path(&path).is_err(), "accepted {path}");
        }
    }
}
