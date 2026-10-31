# Checked-in seed input

`initial/` is the fixed source-fact workspace used by the standalone public
seed. It is checked into Git so the bootstrap path does not need Python, Node,
the website, a downloaded archive, or an already-installed Topaz executable.

The workspace contains the canonical compiler source set, the generated
standard-library boundary modules, `request.json`, and
`REQUEST-PROVENANCE.json`. The request is an input artifact, not a claim that
the current generated compiler has already been reproduced.

The request must be regenerated as a coordinated source change when the
canonical source manifest or the CLI/bootstrap contract changes. Do not edit
individual source facts without updating the authoritative source and the
provenance record.
