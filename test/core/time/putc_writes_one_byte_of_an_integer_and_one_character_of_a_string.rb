# `0x1B4` truncates to its low byte; a BINARY string contributes its
# first BYTE (never a UTF-8 promotion of it).

path = "/tmp/zeo_e2e_binputc_#{Process.pid}"
File.open(path, "wb") { |f| f.putc 0x1b4; f.putc 180.chr + "xx" }
p File.binread(path).bytes
File.delete(path)
__END__
[180, 180]
