# One `def` in a module, included into classes that make it emit DIFFERENTLY.
# `codegen::share` serves a widely-inherited body from a single emitted
# function, so each shape here is one it must decline -- and `codegen::
# class_query` is what lets it know, by recording the question the emission
# asked about its receiver and replaying it against the other members.
#
# Real gems turn out never to hit these: every sharing group in prism, uri,
# minitest and rubygems renders identically for all its members. That makes the
# safety net untestable in the field, so the shapes are built here on purpose --
# one per question the trace records.

# --- ShadowsKernel: is a bare name the Kernel free function, or a real method?

module Report
  def render = "[#{format('%04d', 7)}]"
end

class PlainReport
  include Report
end

class CustomReport
  include Report
  # Shadows Kernel#format, so the very same body must NOT fold to the free
  # function here.
  def format(*args) = "custom#{args.length}"
end

p PlainReport.new.render
p CustomReport.new.render

# (The third shape the trace records, `ValueSuper`, cannot be exercised here:
# a module mixed into a value subclass whose method calls `super` crashes the
# generated program outright, with or without sharing. See
# tests/gaps/module_super_into_a_value_builtin.rb.)

# --- IvarSlot: where does the module's own ivar sit?

class WithOne
  def initialize = @first = 1
end

module Tracked
  def track
    @seen = (@seen || 0) + 1
    "#{@seen}/#{@seen.class}/#{@seen + 1}/#{@seen * 3}/#{@seen.abs}/#{@seen.to_s}"
  end
end

class TrackedSub < WithOne
  include Tracked
end

class TrackedBare
  include Tracked
end

s = TrackedSub.new
b = TrackedBare.new
p [s.track.split("/").first, s.track.split("/").first]
p b.track.split("/").first
p [s.instance_variables, b.instance_variables]
__END__
"[0007]"
"[custom2]"
["1", "2"]
"1"
[[:@first, :@seen], [:@seen]]
