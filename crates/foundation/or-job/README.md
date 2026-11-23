# or-job

Cancellation and progress for a job that runs long enough to need both.

`Cancel`, `Progress` and `or_job::run` are the loop `openrage-cli` and the editor run a bulk job through. The first draws progress as a line on stderr and the second as a bar in a frame, so what they share is the counting and the stopping, never the drawing. A failure on one item is counted and the job goes on.

- Layer: foundation, which knows no file format and no game.
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-job`.
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#foundation-crates).
