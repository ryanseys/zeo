# Bundled tests:
#   - file_binread_directory_error
#   - file_class_methods_int_recv
#   - file_directory_predicates
#   - file_read_directory_error
#   - file_write_binary_string
#   - file_write_directory_error

# === file_binread_directory_error ===
require "tmpdir"
ZTMP = Dir.mktmpdir

def t_file_binread_directory_error
  begin
    File.binread(".")
    puts "binread string succeeded"
  rescue => e
    puts e.class
    puts e.message.include?("Is a directory")
  end
  
  begin
    File.binread(".").bytes
    puts "binread bytes succeeded"
  rescue => e
    puts e.class
    puts e.message.include?("Is a directory")
  end
end
t_file_binread_directory_error

# === file_class_methods_int_recv ===
def t_file_class_methods_int_recv
  # `File.readable?` and `File.binwrite` — class-method stubs in
  # the `rcname == "File"` dispatch block, plus `IntArray#pack("C*")`
  # which is the symmetric inverse of `String#bytes`.
  #
  # Surfaced via optcarrot's battery-save dead code path:
  #   `return unless File.readable?(sav)` and
  #   `File.binwrite(sav, @wrk.pack("C*"))`.
  #
  # `readable?` reuses the exist check (close enough on POSIX).
  # `binwrite` reuses `sp_file_write` — fputs-based, so embedded
  # NULs truncate; acceptable for the NUL-free use sites that
  # exist in practice.
  #
  # A cwd-relative path, so ruby and zeo resolve it to the same file.
  
  path = File.join(ZTMP, "probe_file_class_test")
  
  File.binwrite(path, [72, 105, 33].pack("C*"))   # "Hi!"
  puts File.readable?(path)
  puts File.read(path)
  File.delete(path)
  puts File.exist?(path)
end
t_file_class_methods_int_recv

# === file_directory_predicates ===
def t_file_directory_predicates
  path = File.join(ZTMP, "probe_file_predicate_test.txt")
  File.write(path, "x")
  
  puts File.directory?(".")
  puts File.directory?("./")
  puts File.directory?(path)
  puts File.file?(path)
  puts File.file?(".")
  puts File.directory?("probe_missing_predicate_path")
  puts File.file?("probe_missing_predicate_path")
  
  File.delete(path)
end
t_file_directory_predicates

# === file_read_directory_error ===
def t_file_read_directory_error
  begin
    File.read(".")
    puts "read succeeded"
  rescue => e
    puts e.class
    puts e.message.include?("Is a directory")
  end
end
t_file_read_directory_error

# === file_write_binary_string ===
def t_file_write_binary_string
  path = File.join(ZTMP, "probe_file_write_binary_string.bin")
  
  payload = "A" + 0.chr + "B"
  
  File.write(path, payload)
  bytes = File.binread(path).bytes
  
  puts bytes.length
  if bytes.length == 3
    puts bytes[0]
    puts bytes[1]
    puts bytes[2]
  end
  puts payload.inspect.include?("B")
  
  File.delete(path) if File.exist?(path)
end
t_file_write_binary_string

# === file_write_directory_error ===
def t_file_write_directory_error
  begin
    File.write(".", "hello")
    puts "write succeeded"
  rescue => e
    puts e.class
    puts e.message.include?("Is a directory")
  end
end
t_file_write_directory_error

__END__
Errno::EISDIR
true
Errno::EISDIR
true
true
Hi!
false
true
true
false
true
false
false
false
Errno::EISDIR
true
3
65
0
66
true
Errno::EISDIR
true
