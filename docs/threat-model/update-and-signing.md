# Update and signing threat model

The current repository does not publish an auto-updating binary. Before update functionality is enabled, the release design must prove:

- repository and release identity;
- signed application artifacts;
- signed adapter catalog metadata;
- checksum/signature verification before execution;
- rollback to the previous application and database-compatible version;
- no updater-controlled payment, credential, or arbitrary repository-script behavior;
- explicit separation of stable, preview, experimental, and legal-review channels.

Until those controls exist, updates are manual developer operations and the UI must not claim otherwise.
