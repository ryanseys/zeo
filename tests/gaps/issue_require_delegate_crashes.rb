# A bare `require "delegate"` crashes at load time: Delegator's setup calls
# `undef_method` inside a `class_eval` block on a duplicated Kernel module
# (see issue_undef_method_dynamic_class_body.rb), which zeo doesn't support.
require "delegate"
puts "loaded ok"
