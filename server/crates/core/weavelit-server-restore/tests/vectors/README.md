# Vendored C2SP CCTV age test vectors

External known-answer vectors for the age v1 file format, used to validate the
Server's own age v1 X25519 reader against an implementation that is not part of
this repository.

## Provenance

- **Upstream repository:** <https://github.com/C2SP/CCTV>
- **Upstream path:** `age/testdata`
- **Pinned commit:** `1e3d2860d46e94e777e1b17c7a6f2436387e3ecc` (2026-06-05)
- **Retrieved:** 2026-08-10
- **Copyright:** Copyright (c) 2022 The age Authors
- **License:** `0BSD` (upstream offers Zero-Clause BSD, CC0 1.0, or Unlicense at
  the recipient's choice; this repository takes the Zero-Clause BSD option).
  Upstream states that copying the vectors into a project is permitted without
  attribution; the provenance recorded here is a Weavelit requirement, not a
  license requirement.

## Contents

Upstream `age/testdata` holds 143 vector files. The 33 files whose header
carries `armored: yes` are **excluded**: the Weavelit backup format defines no
ASCII armor, so an armored age file is not a valid input to the Server's reader
and those vectors exercise a layer this repository does not implement. The
remaining 110 files are vendored here byte for byte.

`vectors.json` pins every vendored file's byte length and SHA-256 digest, in the
same shape as `tests/fixtures/fixtures.json`, so an edited or replaced vector
fails a test.

## Vector format

Each file is an RFC822-style header, a blank line, then an age file body. The
body is raw unless the header carries `compressed: zlib`. See the upstream
`age/README.md` for the full key list.

## Harness

`src/vectors.rs` (compiled only under `cfg(test)`) loads every file, verifies it
against `vectors.json`, wraps the age body in the fixed Weavelit outer envelope,
and runs the production reader against it. The expected outcome of every vendored
vector is pinned in that module.
