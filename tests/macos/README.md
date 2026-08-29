# macOS-only goldens

Ordinary ruby-oracle goldens, with one extra condition: the program's output
is inherently macOS-specific -- a hardcoded ioctl number, the errno constant
surface, per-platform `dlopen` flag values -- so Linux CRuby would diverge
from the committed macOS-oracle golden exactly as zeo does.

The suite skips this whole directory off macOS. Each file's own header states
which platform fact it depends on.

**A program that RUNS on both platforms and merely ANSWERS differently does
not belong here.** Moving it here buys a green run by dropping the Linux
coverage entirely. Leave it in the ordinary suite and give it a second golden
instead -- `<name>.rb.linux.expected` beside `<name>.rb.expected`, each
recorded from the same pinned ruby on its own platform. `sort_by_tie_order`
and the other libc-tie-order goldens are the shape. This directory is for the
programs whose SUBJECT does not exist off macOS.
