#!/usr/bin/env python3
"""Guard parsed-plan grants and pre-dispatch enterprise SAF enforcement."""

from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[1]


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def execute_body(source: str) -> str:
    start = source.index("    pub fn execute(\n")
    end = source.index("\n    ///", start)
    return source[start:end]


def check(root: Path = ROOT) -> None:
    product = (root / "crates/apps/mainframe-env-server/src/product.rs").read_text()
    require("let plan = self.batch.plan(&bundle)" in product, "job admission does not use the parsed plan")
    require(
        "job_capabilities(self.store.as_ref(), &plan)" in product,
        "job grants are not derived from the verified plan and installed registry",
    )
    require("job_capabilities(&jcl)" not in product, "raw JCL still controls grants")
    for service in ("Db2Service", "ImsService", "MqService"):
        require(
            f"{service}::open_authorized(" in product,
            f"production composition bypasses {service} resource authorization",
        )

    for family in ("db2", "ims", "mq"):
        source = (root / f"crates/providers/mainframe-env-{family}/src/service.rs").read_text()
        body = execute_body(source)
        require("authorizer.authorize(" in body, f"{family} execute omits enterprise authorization")
        require(
            body.index("authorizer.authorize(") < body.index("apply_request("),
            f"{family} authorization occurs after provider dispatch",
        )
        require(
            f"fn {family}_" in source and "_denial_precedes_mutation()" in source,
            f"{family} deny-before-mutation regression is missing",
        )


def main() -> int:
    try:
        check()
    except (OSError, ValueError) as error:
        print(f"enterprise authorization architecture guard: {error}", file=sys.stderr)
        return 1
    print("enterprise authorization architecture guard: pass")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
