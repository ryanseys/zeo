# IO.popen is unimplemented -- zeo raises NoMethodError instead of spawning
# the subprocess and yielding a readable/writable IO connected to its
# stdin/stdout.
IO.popen("echo hello") { |io| puts io.read.strip }
