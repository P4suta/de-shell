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

let () =
  Alcotest.run "IR properties"
    [ ("codec", [ QCheck_alcotest.to_alcotest round_trip_property ]) ]
