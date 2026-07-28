# A bare `require "erb"` fails at load time: `ERB::Util.html_escape` is
# undefined. erb/util.rb reaches it through `include ERB::Escape` and then
# `module_function :html_escape`, but zeo's `module_function` only promotes a
# method DEFINED IN THE SAME BODY -- an inherited name silently no-ops instead
# of copying the ancestor's instance method onto the module.
#
# (This replaced an earlier codegen/struct-layout mismatch -- the generated
# Rust referenced a `self.frozen_string` field that was never declared, since
# an ivar written only via multiple assignment was never collected. That is
# fixed; this is the blocker underneath it.)
require "erb"
p ERB.respond_to?(:new)
