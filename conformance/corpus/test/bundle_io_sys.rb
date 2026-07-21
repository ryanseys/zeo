# Bundled tests:
#   - file_binread
#   - fileio
#   - fileopen
#   - fileops
#   - system
#   - system_expr
#   - process_clock_gettime
#   - time

# === file_binread ===
def t_file_binread
  # `File.binread(path).bytes` — sp_str_bytes uses null-termination
  # and stops at the first 0x00 byte, so for binary data (e.g. .nes
  # ROM files where 0x00 appears mid-file) we need a dedicated
  # helper that reads with the actual file size.
  #
  # `File.binread(path)` standalone is aliased to File.read.
  #
  # Uses a cwd-relative path so MSYS2 and the native-Windows-built
  # spinel binary resolve to the same file (the harness runs each test
  # from the project root). `/tmp/...` would land in different places
  # on the two sides — see test/fileio.rb for the same workaround.
  
  # Set up a binary file with embedded NULs via a shell command.
  # spinel's File.write uses fputs and would stop at the first NUL.
  path = "spinel_binread_test.bin"
  `printf 'AB\\000CD\\000EF' > #{path}`
  
  # Pattern-matched: emits sp_file_binread_bytes(path) which reads
  # the file by its actual byte count, NOT through sp_str_bytes.
  arr = File.binread(path).bytes
  puts arr.length               # 8
  puts arr[0]                   # 65 (A)
  puts arr[1]                   # 66 (B)
  puts arr[2]                   # 0  (NUL)
  puts arr[3]                   # 67 (C)
  puts arr[4]                   # 68 (D)
  puts arr[5]                   # 0  (NUL)
  puts arr[6]                   # 69 (E)
  puts arr[7]                   # 70 (F)
  
  # `File.binread` standalone aliases `File.read`. spinel's strings
  # are null-terminated so any NUL in the result is a hard stop —
  # this branch is just verifying the alias resolves and returns
  # something string-shaped.
  puts File.binread(path)[0, 2] # AB
  
  File.delete(path) if File.exist?(path)
end
t_file_binread

# === fileio ===
def t_fileio
  # Test basic File I/O.
  # Uses cwd-relative paths so the harness (which runs each test from
  # the project root) and the CRuby reference both write to the same
  # place on every platform — `/tmp/...` doesn't resolve uniformly
  # across MSYS2 mingw64 ruby and native-Windows-built spinel binaries.
  
  # Write a file
  File.write("spinel_test.txt", "Hello from Spinel!\nLine 2\n")
  
  # Read the file
  content = File.read("spinel_test.txt")
  puts content
  
  # File.exist?
  puts File.exist?("spinel_test.txt")  # true
  puts File.exist?("spinel_nonexistent.txt")  # false
  
  # Clean up
  File.delete("spinel_test.txt")
  puts File.exist?("spinel_test.txt")  # false
  
  puts "done"
end
t_fileio

# === fileopen ===
def t_fileopen
  # Test File.open with block.
  # Uses a cwd-relative path for the same reason as test/fileio.rb.
  
  # Write with block
  File.open("spinel_fopen_test.txt", "w") do |f|
    f.puts "line 1"
    f.puts "line 2"
    f.puts "line 3"
  end
  
  # Read with block
  File.open("spinel_fopen_test.txt", "r") do |f|
    f.each_line do |line|
      puts line
    end
  end
  
  # File.open without block (returns file object)
  # Skip — needs explicit close, less common
  
  # Cleanup
  File.delete("spinel_fopen_test.txt")
  puts "done"
end
t_fileopen

# === fileops ===
def t_fileops
  puts File.join("home", "user")      # home/user
  puts File.basename("/path/to/file") # file
  puts "".empty?                      # true
  puts "hi".empty?                    # false
  puts 5.clamp(1, 10)                 # 5
  puts 15.clamp(1, 10)                # 10
  puts "hello".to_sym                 # hello
  puts "a".ord                        # 97
  puts "42".to_i + 1                  # 43
  puts "done"
end
t_fileops

# === system ===
def t_system
  # Test system features needed for ccm
  
  # system()
  system("echo hello_from_system")  # hello_from_system
  
  # ENV
  puts ENV['HOME'] != nil  # true
  
  # Dir.home
  home = Dir.home
  puts home.length > 0  # true
  
  # backtick
  result = `echo backtick_test`.strip
  puts result  # backtick_test
  
  # trap (just register, don't trigger)
  trap('INT') { }
  puts "trap set"
  
  # $stdin — skip interactive test
  # File.readlink — skip (needs symlink)
  
  puts "done"
end
t_system

# === system_expr ===
def t_system_expr
  ok = system("echo hello_from_expr")
  puts ok
  puts($? == 0)
  ok = system("false")
  puts ok
  puts($? == 0)
end
t_system_expr

# === process_clock_gettime ===
def t_process_clock_gettime
  # Process.clock_gettime(Process::CLOCK_MONOTONIC) returns a Float.
  # We can't assert an exact value, but we can verify the call lowers
  # correctly (returns a float, monotonic non-decreasing across two
  # samples).
  
  t1 = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  t2 = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  
  # t1 and t2 must be Float-typed in spinel; arithmetic should work.
  diff = t2 - t1
  if diff >= 0
    puts "monotonic"
  end
end
t_process_clock_gettime

# === time ===
def t_time
  t = Time.now
  puts t.to_f - t.to_i > 0.0
  puts t.to_i > 1_000_000_000
  puts t.to_i < 2_000_000_000
  puts t.to_f > 1_000_000_000.0
  puts t.to_i == t.to_f.to_i || t.to_i + 1 == t.to_f.to_i
  
  puts Time.at(1234567890).to_i == 1234567890
  puts Time.at(0).to_i == 0
  puts Time.at(-1).to_i == -1
  
  t2 = Time.at(1234567890.5)
  puts t2.to_i == 1234567890
  puts (t2.to_f - 1234567890.5).abs < 0.001
  
  a = Time.at(1000.5)
  b = Time.at(500.25)
  puts ((a - b) - 500.25).abs < 0.0001
  
  puts (Time.at(0) - Time.at(0)).abs < 0.0001
  
  puts ((Time.at(100) - Time.at(200)) + 100.0).abs < 0.0001
  
  # guards sp_time_at_float's frac < 0 normalization
  puts Time.at(-0.5).to_i == -1
  
  puts Time.at(0.0).to_i == 0
  
  # d78149b: tv_nsec must reach Time#to_f for ms precision
  puts Time.now.to_f * 1000 > 1_000_000_000_000.0
  
  puts Time.at(1).to_i + Time.at(2).to_i == 3
  
  puts (Time.at(1.5).to_f * 1000 - 1500.0).abs < 1.0
  
  puts (Time.at(2000) - Time.at(1000)).to_i == 1000
  
  puts "done"
end
t_time

