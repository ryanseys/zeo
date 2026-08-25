# A SystemCallError message's `@ site` names CRuby's own C function, not
# the syscall: `@ rb_sysopen`, `@ io_fread`, `@ dir_chdir0`,
# `@ rb_check_realpath_internal`, `@ rb_file_s_stat`, `@ apply2files`, and
# `@ rb_file_s_rename - (a, b)` with BOTH paths. zeo names the syscall
# (`@ read`, `@ chdir`, ...). `File.expand_path("~nouser")` also words its
# ArgumentError differently. (Found by the 2026-08-24 probe sweep.)
def show
  yield
rescue SystemCallError, ArgumentError => e
  puts "#{e.class}: #{e.message}"
end
show { File.read("/tmp") }
show { Dir.chdir("/nonexistent_zeo_probe_xyz") { } }
show { File.realpath("/nonexistent_zeo_probe_xyz") }
show { File.stat("/nonexistent_zeo_probe_xyz") }
show { File.delete("/nonexistent_zeo_probe_xyz") }
show { File.rename("/nonexistent_zeo_probe_xyz", "/tmp/zzz") }
show { File.expand_path("~xyz_no_such_user_probe") }
