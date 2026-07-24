# sp_sprintf truncated string-interpolation results to 4095 bytes.
#
# Every Ruby string interpolation ("...#{x}...") compiles to
# sp_sprintf (see compile_interpolated in spinel_codegen.rb), which
# formatted into a fixed `char _sp_tmp[4096]` stack buffer and then
# clamped the result length to sizeof(buf)-1 -- silently dropping
# everything past 4095 bytes. vsnprintf already returns the would-be
# length, so the fix re-renders into a correctly-sized heap buffer for
# the overflow case (the common <4096 path keeps the stack temp +
# memcpy fast path).
#
# Surfaced in roundhouse view emit: a lowering that coalesces
# `io << a; io << b; io << c` append runs into a single
# `io << "...#{}..."` produced interpolations larger than 4096 bytes,
# which truncated rendered HTML pages mid-content.

big = "A" * 5000
out = "[#{big}]"
# Full length must survive, and both ends must be intact (a mid-string
# clamp would keep the head but drop the closing bracket).
puts out.length
puts out[0, 1]
puts out[out.length - 1, 1]
