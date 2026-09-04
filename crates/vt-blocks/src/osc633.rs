//! VS Code OSC 633: `A`/`B`/`C`/`D[;exit]` plus `E;cmdline[;nonce]` and
//! `P;key=value`. The nonce is the only spoofing defence in this family;
//! `cmdline` is untrusted program output unless the nonce matches.
