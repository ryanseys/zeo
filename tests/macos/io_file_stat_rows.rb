# The `IO`, `File` and `File::Stat` rows that answer facts the runtime already
# holds: the timestamp trio off one fstat, the device-number split, the
# real-uid access predicates, and the raw ioctl/syswrite pair.

require "tmpdir"

Dir.mktmpdir do |dir|
  path = File.join(dir, "sample.txt")
  File.write(path, "hello\n")

  File.open(path) do |f|
    puts "atime: #{f.atime.class}"
    puts "ctime: #{f.ctime.class}"
    puts "birthtime: #{f.birthtime.class}"
    puts "mtime == stat.mtime: #{f.mtime == f.stat.mtime}"
    puts "size: #{f.size}"
    puts "timeout default: #{f.timeout.inspect}"
    f.timeout = 5
    puts "timeout set: #{f.timeout.inspect}"
  end

  st = File.stat(path)
  puts "dev split: #{st.dev_major.is_a?(Integer) && st.dev_minor.is_a?(Integer)}"
  puts "rdev on a regular file: #{[st.rdev_major, st.rdev_minor].inspect}"
  puts "readable_real?: #{st.readable_real?}"
  puts "writable_real?: #{st.writable_real?}"
  puts "executable_real?: #{st.executable_real?}"

  File.chmod(0o755, path)
  puts "executable_real? after chmod: #{File.stat(path).executable_real?}"

  # `chown` to the ids the file already has is a no-op that still reports the
  # count, so it works without privileges.
  puts "chown: #{File.chown(st.uid, st.gid, path)}"
  puts "lchown: #{File.lchown(nil, nil, path)}"
  puts "lutime: #{File.lutime(Time.at(1_000_000), Time.at(1_000_000), path)}"
  puts "mtime after lutime: #{File.stat(path).mtime.to_i}"

  # `syswrite` answers the byte count.
  File.open(path, "w") { |f| puts "syswrite: #{f.syswrite("abcd")}" }
  puts "content: #{File.read(path).inspect}"

  # A leading BOM names the stream's external encoding; without one, nothing
  # changes and the answer is nil.
  bom = File.join(dir, "bom.txt")
  File.binwrite(bom, "\xEF\xBB\xBFhi")
  File.open(bom, "rb") do |f|
    puts "binmode?: #{f.binmode?}"
    puts "bom: #{f.set_encoding_by_bom.inspect}"
    puts "after bom: #{f.read.inspect}"
  end
  File.open(path, "rb") do |f|
    puts "no bom: #{f.set_encoding_by_bom.inspect}"
  end
end

# `IO#puts`/`#print` are public methods of IO -- not Kernel's private pair.
puts "IO#puts listed: #{IO.instance_methods(false).include?(:puts)}"
puts "IO#print listed: #{IO.instance_methods(false).include?(:print)}"
puts "Kernel#puts hidden: #{Kernel.instance_methods(false).include?(:puts)}"
puts "Kernel#puts private: #{Kernel.private_instance_methods(false).include?(:puts)}"
STDOUT.print "print to a stream\n"
STDOUT.puts "puts to a stream"

# `ioctl` reaches the kernel: FIONREAD on a pipe reports the bytes waiting.
r, w = IO.pipe
w.write("1234")
w.flush
buf = [0].pack("l!")
r.ioctl(0x4004667F, buf)
puts "fionread: #{buf.unpack1("l!")}"
r.close
w.close
