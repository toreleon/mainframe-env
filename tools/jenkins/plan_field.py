#!/usr/bin/env python3
"""Print one validated boolean selector from a CI assurance plan."""
from __future__ import annotations

import argparse
import json
from pathlib import Path

DIRECT_FIELDS = frozenset({
    'build', 'msrv', 'store', 'architecture', 'evidence', 'mutation', 'full',
})
GATE_FIELDS = frozenset({'targets', 'documentation'})


def selected(plan: dict, name: str) -> bool:
    if name in DIRECT_FIELDS:
        value = plan.get(name)
        if not isinstance(value, bool):
            raise ValueError(f'plan field {name} is not boolean')
        return value
    if name in GATE_FIELDS:
        gates = plan.get('primary_gates')
        if (not isinstance(gates, list)
                or any(not isinstance(gate, str) for gate in gates)
                or len(gates) != len(set(gates))):
            raise ValueError('plan primary_gates is not a unique string array')
        return name in gates
    raise ValueError(f'unsupported plan selector {name}')


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--plan', type=Path, required=True)
    parser.add_argument('--field', choices=sorted(DIRECT_FIELDS | GATE_FIELDS), required=True)
    args = parser.parse_args()
    plan = json.loads(args.plan.read_text())
    if not isinstance(plan, dict):
        raise ValueError('CI assurance plan is not an object')
    print(str(selected(plan, args.field)).lower())
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, ValueError, json.JSONDecodeError) as error:
        raise SystemExit(f'Jenkins plan selector failed closed: {error}')
