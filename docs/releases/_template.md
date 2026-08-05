---
title: confval vX.Y.Z
description: One-line summary of the release.
slug: vX.Y.Z
date: 2026-01-01
authors: [ethanhann]
tags: [release]
---

{/*
Copy this file to releases/vX_Y_Z.md for the next release.
Files prefixed with an underscore are ignored by the releases plugin, so this
template never publishes.

Naming:
- Filename: vX_Y_Z.md (underscores).
- slug: vX.Y.Z, which produces the URL /releases/vX.Y.Z.
- date: the release date, which orders the feed.

confval and confval-derive share one version, so a single entry covers both crates.
Follow Keep a Changelog for the section names below and delete any that do not apply.
*/}

One or two sentences describing the theme of this release.

{/* truncate */}

## Highlights

- The headline change, stated plainly.

## Added

## Changed

## Deprecated

## Removed

## Fixed

## Security

## Upgrading

What you need to change when moving from the previous version.

```toml
[dependencies]
confval = { version = "X.Y", features = ["derive", "hcl", "color"] }
```
