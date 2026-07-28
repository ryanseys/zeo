# A bare `require "tempfile"` now COMPILES but dies at load time: tempfile
# requires delegate, whose `Delegator` setup calls `undef_method` inside a
# `class_eval` block on a duplicated Kernel module, and that only works in a
# literal `class ... end` body (see issue_undef_method_dynamic_class_body.rb
# and issue_require_delegate_crashes.rb, which this now shares a cause with).
#
# (This replaced two codegen bugs, both fixed: an undeclared `path` identifier
# -- a keyword argument's value escaped the spliced-file local renaming, so
# rustc reported a collision with the built-in `#[path]` attribute -- and a
# keyword parameter bound without `mut` that the method body reassigned.)
require "tempfile"
p Tempfile.respond_to?(:new)
