# The `@ site` in a SystemCallError message names CRuby's own C FUNCTION,
# not the syscall underneath it. `File.stat` says `rb_file_s_stat`,
# `File.delete` says `apply2files` (the shared helper behind chmod, chown,
# utime and unlink alike), and a two-path operation prints BOTH operands:
# `... @ rb_file_s_rename - (from, to)`.
#
# Two rows are where the mapping stops being mechanical. `Dir.chdir` names
# a DIFFERENT function per form -- `dir_chdir0` with a block, `chdir_path`
# without -- and reading a DIRECTORY says `io_fread`, because CRuby opens
# and then reads, and only the read can answer EISDIR. One Rust call covers
# both steps, so that site is picked from the errno.
#
# `File.readlines(a_directory)` is deliberately absent: CRuby answers
# `@ io_fillbuf - fd:6 <path>`, naming a buffered-read helper AND the raw
# descriptor number, which is not reproducible across runs.

require "tmpdir"
D = Dir.mktmpdir
M = File.join(D, "gone")
E = File.join(D, "here")
File.write(E, "hi")
def show(n)
  yield
rescue Exception => e
  puts "#{n}\t#{e.class}: #{e.message.gsub(D, '<T>')}"
end
show("atime") { File.atime(M) }
show("ctime") { File.ctime(M) }
show("birthtime") { File.birthtime(M) }
show("lstat") { File.lstat(M) }
show("stat") { File.stat(M) }
show("size") { File.size(M) }
show("mtime") { File.mtime(M) }
show("truncate") { File.truncate(M, 0) }
show("realdirpath") { File.realdirpath(M) }
show("link") { File.link(E, E) }
show("symlink") { File.symlink(E, E) }
show("rename") { File.rename(M, E) }
show("chmod") { File.chmod(0o600, M) }
show("chown") { File.chown(nil, nil, M) }
show("utime") { File.utime(Time.now, Time.now, M) }
show("delete") { File.delete(M) }
show("readlink") { File.readlink(M) }
show("expand ~x") { File.expand_path("~xyz_no_user") }
show("Dir.mkdir") { Dir.mkdir(D) }
show("Dir.rmdir") { Dir.rmdir(M) }
show("Dir.entries") { Dir.entries(M) }
show("Dir.open") { Dir.open(M) }
show("Dir.new") { Dir.new(M) }
show("IO.read dir") { IO.read(D) }
show("chdir no block") { Dir.chdir(M) }
show("chdir block") { Dir.chdir(M) { 1 } }
show("File.read dir") { File.read(D) }
show("File.open missing") { File.open(M) }
show("File.write dir") { File.write(D, "x") }
