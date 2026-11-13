# Contributing

BulkParse is a standalone open-source library. Keep Bulkgrid-specific infrastructure outside the core. Contributions should use the shared model and report unsupported content rather than silently dropping it.

Run formatting and relevant core tests from the README. Changes to the output model must update JSON Schema expectations and both TypeScript declarations. Binding changes need their host smoke tests. Add a small original or redistribution-safe fixture for parsing behavior; do not commit customer documents.

New formats must state their actual extraction scope, dependency/license requirements, resource limits and source provenance. Include malformed-input cases and quality expectations. Performance changes need reproducible before/after measurements at comparable quality.

Do not modify or include the downloaded competitor reference trees in submissions. Do not publish packages or push releases without project-maintainer authorization.
