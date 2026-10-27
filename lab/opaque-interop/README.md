# Independent OPAQUE Interoperability Gate

The two binaries compile the same bounded wire-test adapter against different
dependency graphs. Reference uses the published, unchanged opaque-ke 4.0.1;
restricted uses K&K's explicit Ristretto-only overrides. They do not link into
one process or share a patched reference library.

The parent relays registration/login frames over pipes, compares only hashes of
the resulting session keys and bounds process/frame lifetime. Private setup and
keys are never logged. Passwords are public test fixtures, not operator secrets.
Both client/server directions must succeed; changed password/context must fail.

The historical reference graph intentionally contains old dependencies for the
compatibility comparison and is never linked into the product. The root strict
advisory gate audits the product's lockfile. First use of this separate reference
gate needs its registry packages or an existing cache; the offline product-source
rebuild claim does not imply that this historical reference can fetch offline.
