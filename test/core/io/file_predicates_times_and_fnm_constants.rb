# File-type/permission predicates, time accessors, and the FNM_* flag
# constants, exercised on a file the test writes.

p [File::FNM_DOTMATCH, File::FNM_PATHNAME, File::FNM_CASEFOLD, File::FNM_NOESCAPE, File::FNM_EXTGLOB]
path = "/tmp/sp_e2e_file_probe"
File.write(path, "hi"); File.chmod(0644, path)
p File.world_readable?(path)
p [File.pipe?(path), File.socket?(path), File.chardev?(path), File.blockdev?(path)]
p [File.owned?(path), File.setuid?(path), File.sticky?(path)]
p File.identical?(path, path)
p [File.birthtime(path).class, File.atime(path).class, File.ctime(path).class]
link = "/tmp/sp_e2e_file_link"
File.symlink(path, link)
p [File.symlink?(link), File.readlink(link) == path]
File.delete(link); File.delete(path)
__END__
[4, 2, 8, 1, 16]
420
[false, false, false, false]
[true, false, false]
true
[Time, Time, Time]
[true, true]
