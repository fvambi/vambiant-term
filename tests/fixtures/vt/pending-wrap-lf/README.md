# pending-wrap + LF (the only grid divergence found in M0)

`input.vt` = 80 × `A`, `\n`, `B` on an 80-column grid. After the 80th `A` the
cursor is in the pending-wrap state. libghostty-vt 0.2.1 clears that state on
LF (xterm behaviour) and places `B` at column 79 of the next row;
alacritty_terminal 0.26.0 keeps it and wraps `B` to column 0 of the row after
that. With CRLF (any PTY with ONLCR) both engines agree on every M0 corpus.
Captured 2026-09-05 with `term-core-spike dump`.
