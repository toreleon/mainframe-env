# mainframe-env-encoding

Ownership: bounded mainframe byte conversion and collation primitives.
Non-goals: compiler orchestration, datasets, or ambient locale discovery. It
uses only the standard library. Public surface: CCSID selection, CP037
conversion, EBCDIC collation, packed/zoned decimal, and checked binary values.

Invariants: conversion never silently substitutes unmappable data; decimal
precision and buffers are bounded. Verify with
`cargo test -p mainframe-env-encoding`.
