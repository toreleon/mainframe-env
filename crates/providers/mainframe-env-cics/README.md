# mainframe-env-cics

The sole 0.1 CICS authority. It owns bounded protocol-neutral terminal sessions,
BMS definitions, EIB/condition state, file/program routes through typed host
services, transaction identity, suspension/resume, and durable session recovery.
The provider never receives dataset/RACF internals or a UI/rendering authority.

The package owns the versioned Apache-2.0 DFHAID and DFHBMSCA behavioral
compatibility source members. Consumers opt into this ABI library explicitly;
catalog presence grants no CICS semantic coverage credit.
