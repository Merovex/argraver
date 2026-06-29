# Contributing to Argraver

Thanks for your interest. Patches and fixes are welcome.

## Working on a change

- Build: `cargo build` · Test: `cargo test` · Lint: `cargo clippy`
- Keep changes focused; match the surrounding style. See `CLAUDE.md` for the
  module map and the EPUB-first / PDF architecture.
- The PDF path shells out to `typst` (must be on your `PATH`); the EPUB path is
  self-contained.

Please make sure `cargo test` and `cargo clippy` are clean before opening a pull
request, and include a `Signed-off-by` line (`git commit -s`) to certify the
[Developer Certificate of Origin](https://developercertificate.org/).

## Contribution terms

By submitting a contribution (a pull request, patch, or any change) to this
project, you agree that:

1. You wrote it, or otherwise have the right to submit it under these terms.
2. You grant B. Wilson (the Licensor) a perpetual, worldwide, irrevocable,
   royalty-free, sublicensable license to use, reproduce, modify, sublicense,
   and distribute your contribution, in whole or in part, under this project's
   license **or any other terms, including commercial terms**.
3. You retain copyright in your contribution; this grant is non-exclusive, so
   you remain free to use your own work however you like.

This lets the Licensor keep the project's non-commercial license for the public
(see [`LICENSE`](LICENSE)) while preserving the right to relicense the project,
including commercially, without tracking down every contributor.
