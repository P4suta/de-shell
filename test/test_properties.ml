open Deshell

let argv_generator =
  let open QCheck.Gen in
  let atom = string_size ~gen:printable (int_range 0 24) in
  map2
    (fun executable arguments -> executable :: arguments)
    (map (fun suffix -> "command-" ^ suffix) atom)
    (list_size (int_range 0 7) atom)

let round_trip_property =
  QCheck.Test.make ~count:300
    ~name:"Exec argv survives canonical JSON round-trip"
    (QCheck.make argv_generator) (fun argv ->
      let node =
        Ir.node ~id:"generated"
          ~guarantee:(Ir.Formal { basis = "property" })
          (Ir.Exec (Ir.exec argv))
      in
      let plan =
        Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:node () ]
      in
      match Ir_codec.decode_string (Ir_codec.encode_string plan) with
      | Error _ -> false
      | Ok decoded -> Ir.equal_plan plan decoded)

let runtime_state_generator =
  let open QCheck.Gen in
  let text = string_size ~gen:printable (int_range 0 48) in
  let typed_value =
    oneof
      [
        map (fun value -> (Ir.Text, value)) text;
        map (fun value -> (Ir.Bytes, value)) text;
        map (fun value -> (Ir.Path, value)) text;
        map (fun value -> (Ir.Int, string_of_int value)) int;
        map (fun value -> (Ir.Bool, string_of_bool value)) bool;
        map (fun value -> (Ir.Secret Ir.Text, value)) text;
      ]
  in
  map2
    (fun suffix (value_type, value) ->
      ("state_" ^ string_of_int suffix, value_type, value))
    nat_small typed_value

let runtime_state_round_trip_property =
  QCheck.Test.make ~count:300
    ~name:"typed runtime state survives canonical JSON round-trip"
    (QCheck.make runtime_state_generator) (fun (name, value_type, value) ->
      let node =
        Ir.node ~id:"generated-state"
          ~guarantee:(Ir.Formal { basis = "property" })
          (Ir.Set_variable { name; value_type; value })
      in
      let plan =
        Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:node () ]
      in
      match Ir_codec.decode_string (Ir_codec.encode_string plan) with
      | Error _ -> false
      | Ok decoded -> Ir.equal_plan plan decoded)

let stdout_capture_round_trip_property =
  let open QCheck.Gen in
  let generator =
    map2
      (fun suffix argument -> ("capture_" ^ string_of_int suffix, argument))
      nat_small
      (string_size ~gen:printable (int_range 0 48))
  in
  QCheck.Test.make ~count:200
    ~name:"typed stdout capture survives canonical JSON round-trip"
    (QCheck.make generator) (fun (name, argument) ->
      let body =
        Ir.node ~id:"generated-capture-body"
          ~guarantee:(Ir.Formal { basis = "property" })
          (Ir.Exec (Ir.exec [ "probe"; argument ]))
      in
      let node =
        Ir.node ~id:"generated-capture"
          ~guarantee:(Ir.Formal { basis = "property" })
          (Ir.Capture_stdout { name; value_type = Ir.Text; body })
      in
      let plan =
        Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:node () ]
      in
      match Ir_codec.decode_string (Ir_codec.encode_string plan) with
      | Error _ -> false
      | Ok decoded -> Ir.equal_plan plan decoded)

let () =
  Alcotest.run "IR properties"
    [
      ( "codec",
        [
          QCheck_alcotest.to_alcotest round_trip_property;
          QCheck_alcotest.to_alcotest runtime_state_round_trip_property;
          QCheck_alcotest.to_alcotest stdout_capture_round_trip_property;
        ] );
    ]
