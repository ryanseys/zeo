# Followup to #549. File.write recognized at expression level
# (alongside binwrite). Statement-context calls to File.write
# were already routed through compile_control_call_stmt and
# emitted sp_file_write directly. Tail-position calls in a
# method body fall through to compile_expr, which only
# recognized binwrite; without the alias here, File.write
# warned "cannot resolve call to 'write' on int" and returned
# the literal 0, silently dropping the side effect.

require "tmpdir"
ZTMP = Dir.mktmpdir

def store(path, data)
  File.write(path, data)
end

store(File.join(ZTMP, "probe_file_write_test.txt"), "ok\n")
puts File.read(File.join(ZTMP, "probe_file_write_test.txt"))
File.delete(File.join(ZTMP, "probe_file_write_test.txt"))
__END__
ok
