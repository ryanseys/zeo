# FFI :binstr return mode -- binary-safe String from a byte count.
#
# A `:str` FFI return lowers to strlen, truncating at the first embedded NUL,
# which is fatal for binary protocols (WebSocket frames carry 0x00). The
# `:binstr` return mode builds the String from the exact byte count the callee
# published in sp_net_bin_len, so embedded NULs survive. Verified here via
# shell_capture (deterministic on every POSIX target) of a 5-byte payload with
# two embedded NULs -- a plain `:str` would report length 1.
module Net
  ffi_func :sp_net_shell_capture, [:str, :int], :binstr
end

s = Net.sp_net_shell_capture("printf 'a\\0b\\0c'", 64)
puts s.length
puts s.bytesize
puts s.bytes.inspect
