# Agent Feedback

- context: Authoring ADS epic task contracts; tasks stayed unpickable despite exact cargo commands in Verification tables.
- friction: v7TextHasExactVerificationProof prefix-matches Check cells against bare command prefixes, so backtick-wrapped commands like `cargo test ...` fail VERIFICATION_PROOF_MISSING with no hint about backticks.
- product-idea: Trim leading backticks/inline-code markers in v7VerificationCheckLooksExact before prefix matching, or extend the warning hint to mention backticks.
- impact: Task authors following normal markdown habits produce permanently unpickable tasks; cost ~15 min of bisection.
- related: ADS-T-0001
