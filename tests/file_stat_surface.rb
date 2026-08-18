path = "/tmp/sp_stat_surface_#{Process.pid}"
File.write(path, "hello")
st = File.stat(path)
p st.size == 5
p st.size? == 5
p st.zero?
p(st.nlink >= 1)
p st.pipe?
p st.readable?
p st.writable?
p st.blockdev?
p st.chardev?
p(st.ino > 0)
p(st.blksize > 0)
p(st.uid == Process.uid)
st2 = File::Stat.new(path)
p st2.size == 5
p st2.file?
File.delete(path)
