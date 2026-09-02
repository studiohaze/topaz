# v5.4 pending oracle fixtures

This directory holds desired-shape fixtures that document release-bound gaps
without entering `check-corpus` yet. Each fixture must carry an explicit tracking
marker and a matching checker test or receipt that pins the current behavior.
The executable gate is the pinned checker test; pending fixtures are oracles,
not corpus rows.

As of v5.4 L1 generic nominal 1b, this directory is empty.
