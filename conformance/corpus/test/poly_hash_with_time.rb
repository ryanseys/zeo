# Same heap-box pattern as Range applied to Time. sp_Time is wider
# than sp_RbVal's union (8 bytes), so a Time value in a poly slot
# (heterogeneous hash / array) used to fall through
# `box_non_nullable_value_to_poly`'s default and emit
# `sp_box_int(sp_time_at_int(...))` — which C rejected.
#
# The fix introduces SP_BUILTIN_TIME and `sp_box_time` (heap copies
# the stack Time and boxes through SP_TAG_OBJ). The companion
# `sp_Time_inspect` is timezone-dependent in the runtime (UTC) so
# we don't compare the printed Time bodies here — the regression
# is about the boxing path compiling and the int sibling values
# round-tripping cleanly.

H = {
  start: Time.at(0),
  middle: Time.at(1234567890),
  count: 42,
  limit: 100,
}

puts H[:count]                # 42
puts H[:limit]                # 100
puts H.length                 # 4
puts H.has_key?(:start)       # true
puts H.has_key?(:missing)     # false

# Mixed poly array: Time + Int — verify the int slots still
# round-trip and the Time slots don't break the array shape.
arr = [Time.at(0), 100, Time.at(1234567890), 200]
puts arr.length               # 4
puts arr[1]                   # 100
puts arr[3]                   # 200
