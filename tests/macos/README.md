# macOS-only goldens

Ordinary ruby-oracle goldens, with one extra condition: the program's output
is inherently macOS-specific -- a hardcoded ioctl number, the errno constant
surface, per-platform `dlopen` flag values -- so Linux CRuby would diverge
from the committed macOS-oracle golden exactly as zeo does.

The suite skips this whole directory off macOS. Each file's own header states
which platform fact it depends on.
