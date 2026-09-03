# Ruby's backtrace names the C frames a raise passed through. A raise from
# inside a block given to `Kernel#then` shows ruby's
# `from ...:in 'Kernel#then'` line between the block and its caller; zeo's
# native builtin rows push no frame, so that line is missing and the two frames
# around it become adjacent.
#
# The frame is what makes the trace readable -- it is how a reader sees that
# the failure came through `then` rather than from the calling line directly --
# and anything that INDEXES into `caller` counts a different depth than ruby
# does. zeo already pushes frames for compiled methods, so the machinery
# exists; the builtin rows do not use it.
#
# `caller_locations` from a method reached through such a builtin already
# reports ruby's label (the three lines at the end), so only the raise path
# is short a frame here.

def inner = raise("boom")

begin
  1.then { inner }
rescue => e
  puts e.backtrace.map { |l| l.sub(/^.*gaps./, "").sub(/:\d+:/, ":N:") }
end

def who = caller_locations(1, 1).first.label
def direct = who
p direct
p [1].map { who }.first
p 1.then { who }
__END__
compiler/builtins/builtin_frames_in_a_backtrace.rb:N:in 'Object#inner'
compiler/builtins/builtin_frames_in_a_backtrace.rb:N:in 'block in <main>'
compiler/builtins/builtin_frames_in_a_backtrace.rb:N:in 'Kernel#then'
compiler/builtins/builtin_frames_in_a_backtrace.rb:N:in '<main>'
"Object#direct"
"block in <main>"
"block in <main>"
