# Governance

de-shell is maintained by [@P4suta](https://github.com/P4suta). Contributions
are accepted under the Apache-2.0 license through reviewed pull requests.

Technical decisions prioritize, in order:

1. honest behavioral guarantees and explicit residual behavior;
2. deterministic, outside-in tests and minimized regressions;
3. safety at process, filesystem, network, and secret boundaries;
4. compatibility of the Effect IR, schemas, and adapter protocol;
5. portability and maintainability across the declared platform matrix.

Substantial changes should begin as a GitHub Discussion or feature proposal
that states the user-facing contract, guarantee implications, alternatives, and
migration path. The maintainer records the decision in the pull request,
documentation, or an issue linked from the implementation. Security embargoes
use the private advisory process and may defer public rationale until disclosure.

The maintainer may delegate areas to additional reviewers as the contributor
base grows. Changes to ownership, release authority, or this governance model
must be made in a dedicated pull request.
