# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `ProofStamp::lift`, which advances a stamp's anchor along a chain of
  `AnchorLink`s. `MergeStamp` constrains both sides of a merge to one anchor,
  but wallets stamp against whatever anchor was current when they built, so an
  aggregator collecting stamps from different heights had no way to align them.
  Lifting is the "match/update anchors" step of the aggregation process.
- `AnchorLink`, one block's contribution to the anchor chain: a `Stamp` link per
  stamp absorbed, or an `Empty` link for a block absorbing none.
  `AnchorLink::advance` folds a link over an anchor without proving, so a caller
  can pick the links reaching a target anchor before paying for a lift.

  Both are wrappers over the already-registered `StampLift`, `AnchorSeed`,
  `EmptyBlockSeed`, and `AnchorFuse` steps: no circuit, step registration, or
  proof-system change. A lift cannot cross an epoch boundary, since
  `AnchorChain` has no link for one.

## [0.0.0] - 2026-02-16

### Added

- Initial commit.

[unreleased]: https://github.com/tachyon-zcash/tachyon/compare/v0.0.0...HEAD
[0.0.0]: https://github.com/tachyon-zcash/tachyon/releases/tag/v0.0.0