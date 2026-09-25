# COBOL semantic differential pilot

Run `cargo xtask cobol-differential --cobc /absolute/path/to/cobc --seeds 1..60 --receipt /absolute/external/receipt.json` for a bounded local sweep. `--check` runs the fixed 1..9 smoke budget. The receipt path must be outside the checkout. The command fails if the compiler is missing or unusable; an unrun campaign earns no credit.

The generator explores a small numeric and move grammar, while separate anchor programs replay issues #158, #159, #229, #230, and #231. The receipt records rejected programs, reference compiler identity, issue reachability from generated seeds, and minimized divergences. The process limits are ten seconds and 64 KiB of output per child process. GnuCOBOL is a development reference, not IBM authority; this campaign grants zero licensed conformance credit. Treat new differences as leads for issue triage, not established product defects.
