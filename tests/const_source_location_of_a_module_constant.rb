# `Module#const_source_location` answers where a constant was assigned. zeo
# answers `nil` for a constant written in a module or class body -- the ordinary
# case -- so the method is only ever right about constants that do not exist.
#
# zeo records source locations for methods (`Method#source_location` works) but
# a constant write is emitted as a plain `zeo_rt::const_set` carrying no span,
# so `constants.rs` has nothing to report. The information exists at compile
# time: the same `ClassDef`/assignment node the write is emitted from is the one
# `const_added` was taught to announce from.
#
# It is what a reloader or a diagnostic uses to say WHERE a duplicate constant
# came from -- the "already initialized constant" warning quotes it -- so
# answering nil turns a precise message into a vague one.

module NS
  X = 1
  class Inner
    Y = 2
  end
end
Z = 3

p NS.const_source_location(:X)&.last
p NS::Inner.const_source_location(:Y)&.last
p Object.const_source_location(:Z)&.last
p NS.const_source_location(:Nope)
p Object.const_source_location(:NS)&.last

# A constant set at run time reports the caller's line.
NS.const_set(:W, 4)
p NS.const_source_location(:W)&.last
