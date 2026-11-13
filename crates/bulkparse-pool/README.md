# bulkparse-pool

Optional native Rust process host for BulkParse. Provides bounded waiting queues,
persistent child workers, end-to-end deadlines, cancellation, crash replacement,
worker recycling and metrics. The portable engine and existing bindings do not
depend on this crate.

Build `bulkparse-worker` from the same source tree and configure its trusted path
through `PoolConfig`. Call `Pool::try_submit` with an immutable absolute input
path, parser options and a deadline duration. Inspect the terminal `JobReport`,
including complete/partial/failed status and conversion diagnostics.

See [usage, lifecycle and limits](../../docs/worker-pool.md) and the `parse` and
`load` examples. Integration tests require `python3` for the test-only fault peer.
No Python runtime is required by the library or worker binary.

Experimental. OS-enforced resource limits, tenant authorization, durable job
storage and retry policy belong to the host. No packages have been published.
