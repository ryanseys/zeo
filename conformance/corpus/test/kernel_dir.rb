# Issue #878: Kernel#__dir__ returns the dirname of the source
# file at compile time. The parser emits SOURCE_FILE <path> near
# the AST head so the codegen can compute dirname without needing
# the source to reference __FILE__.
#
# `caller` is intentionally NOT supported -- it would require per-
# call frame tracking that slows the dispatch hot path.
puts __dir__.is_a?(String)
puts __dir__.length > 0
