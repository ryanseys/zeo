# A read given an explicit output-buffer argument returns the data but leaves
# the supplied buffer empty. ruby fills that buffer in place and returns the
# same object, so the caller can read the bytes back out of it.
path = "/tmp/sp_outbuf_ignored.txt"
File.write(path, "hello world")
File.open(path) { |f| b = +""; r = f.sysread(3, b); p r; p b; p r.equal?(b) }
File.open(path) { |f| b = +""; r = f.readpartial(3, b); p r; p b; p r.equal?(b) }
File.delete(path)
