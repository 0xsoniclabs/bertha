# Known Bugs

## Resuming a replay within the epoch before a rules change

Rules updates from `UpdateNetworkRules` events are held in memory until the epoch-sealing block commits them. Before Allegro, Sonic also applies a patched `MinBaseFee` from the block after the event, and Bertha keeps that in memory too. If a replay resumes after such an event but before the epoch-sealing block, both are lost.
