# A constant that resolves nowhere raises NameError where it is READ, and both
# the message and the backtrace frame depend on where that is.
module Outer
  module Wrap
    def self.bare = Missing
    def self.scoped = Deep::Missing
  end
end

def failure
  yield
rescue NameError => e
  puts "#{e.message} | name=#{e.name.inspect} | #{e.backtrace.first[/:\d+:in .*/]}"
end

# A bare miss inside a module is reported under that module's path; so is the
# missing HEAD of a scoped path (`Deep`, not `Deep::Missing` -- Ruby never got
# far enough to look for the leaf).
failure { Outer::Wrap.bare }
failure { Outer::Wrap.scoped }
failure { Nowhere }
failure { Nowhere::Leaf }

# A class-body frame is labelled with the class's LEAF name, however the class
# was spelled at its definition site.
module Pkg; end
module Pkg::Inner
  PROBE = -> { Gone }
end
failure { Pkg::Inner::PROBE.call }

# `include` of an undefined module is a call like any other -- it raises only
# when the body actually runs, and nothing above it is disturbed.
puts "reached the include"
include NeverDefined
