# JIT-only goldens

Ordinary ruby-oracle goldens that depend on the compiler and the program
sharing one process, which only the JIT backend does. The AOT leg skips this
whole directory.

Each file's own header states what it needs the shared process for.
