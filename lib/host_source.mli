type kind = Embedded | Candidate

type detection = {
  kind : kind;
  interpreter : string;
  locator : string;
  source : string;
}

val detect : path:string -> string -> detection list
