# Differential probe: MESSAGE FIDELITY.
#
# Raises on purpose across the surfaces whose text is assembled at the raise
# site, and prints the exception class and message per row. Run through
# `cargo xtask probe messages`.
#
# Message text drifts one site at a time, because each site writes its own.
# Ruby 3.4 moved the quoting from ``x'` to `'x'`, CRuby names its own C
# function in an Errno message, and a comparison failure names both operand
# classes -- each of which zeo got right in some places and not others.
# This probe is how the rest of that family gets found.

require "tmpdir"

ROWS = {}

def probe(name, &blk) = ROWS[name] = blk

# Rows zeo does not answer yet, each naming the gap that tracks it. A carved
# row is left OUT of the output rather than recorded wrong. When a gap is
# promoted, delete its entry here and re-bless; the row comes back.
SKIP = {
  "exec missing" => "tests/gaps/a_failed_exec_names_the_program.rb",
}.freeze

DIR = Dir.mktmpdir("zeo-probe")
MISSING = File.join(DIR, "no-such-file")
EXISTING = File.join(DIR, "a-file")
File.write(EXISTING, "hi")

# --- Errno: the site names CRuby's own C function -------------------------
probe("File.read(missing)") { File.read(MISSING) }
probe("File.open(missing)") { File.open(MISSING) }
probe("File.read(directory)") { File.read(DIR) }
probe("File.stat(missing)") { File.stat(MISSING) }
probe("File.lstat(missing)") { File.lstat(MISSING) }
probe("File.size(missing)") { File.size(MISSING) }
probe("File.mtime(missing)") { File.mtime(MISSING) }
probe("File.delete(missing)") { File.delete(MISSING) }
probe("File.unlink(missing)") { File.unlink(MISSING) }
probe("File.chmod(missing)") { File.chmod(0o600, MISSING) }
probe("File.utime(missing)") { File.utime(Time.now, Time.now, MISSING) }
probe("File.realpath(missing)") { File.realpath(MISSING) }
probe("File.rename(missing)") { File.rename(MISSING, EXISTING) }
probe("File.symlink(exists)") { File.symlink(EXISTING, EXISTING) }
probe("File.link(exists)") { File.link(EXISTING, EXISTING) }
probe("Dir.chdir(missing)") { Dir.chdir(MISSING) }
probe("Dir.chdir(missing){}") { Dir.chdir(MISSING) { 1 } }
probe("Dir.mkdir(exists)") { Dir.mkdir(DIR) }
probe("Dir.entries(missing)") { Dir.entries(MISSING) }
probe("IO.read(missing)") { IO.read(MISSING) }

# --- Comparison failures name both operands -------------------------------
probe("sort mixed") { [1, "a"].sort }
probe("sort_by mixed") { [1, "a"].sort_by { |x| x } }
probe("min_by mixed") { [1.0, Float::NAN, "a"].min_by { |x| x } }
probe("max_by mixed") { [1.0, Float::NAN, "a"].max_by { |x| x } }
probe("min mixed") { [1, "a"].min }
probe("max mixed") { [1, "a"].max }
# TWO elements, not three: with three, which pair the sort reaches first is
# libc's `qsort_r` decision, so the message names a different operand on glibc
# than on BSD. Two elements is one comparison, and both agree.
probe("sort NaN") { [1.0, Float::NAN].sort }
probe("<=> nil compare") { 1 < "a" }
probe("Array#<=> mixed") { ([1] <=> ["a"]).inspect }

# --- Quoting: ruby 3.4 moved from backtick-quote to straight quotes --------
probe("undefined method") { Object.new.no_such_method }
probe("undefined local") { eval("no_such_local_or_method", binding) }
probe("wrong arity") { ->(a) {}.call }
probe("wrong arity method") do
  c = Class.new { def m(a) = a }
  c.new.m
end
probe("private call") do
  c = Class.new do
    private def hidden = 1
  end
  c.new.hidden
end
probe("protected call") do
  c = Class.new do
    protected def guarded = 1
  end
  c.new.guarded
end
probe("unknown keyword") do
  c = Class.new { def m(a:) = a }
  c.new.m(a: 1, b: 2)
end
probe("missing keyword") do
  c = Class.new { def m(a:) = a }
  c.new.m
end
probe("uninitialized constant") { Object.const_get(:NoSuchConstant) }
probe("trap unknown signal") { Signal.trap("SIGNOPE") { } }
probe("kill unknown signal") { Process.kill("SIGNOPE", 0) }
probe("NoMethodError on nil") { nil.no_such }
probe("frozen String") { "x".freeze << "y" }
probe("frozen Array") { [].freeze << 1 }
probe("frozen Hash") { {}.freeze[:a] = 1 }
probe("frozen Class") { Class.new.freeze.const_set(:A, 1) }
probe("super without method") do
  c = Class.new { def inspect = super }
  Class.new(c).new.method(:inspect).owner.to_s
end

# --- Type coercion --------------------------------------------------------
probe("Array + Integer") { [] + 1 }
probe("String + Integer") { "" + 1 }
probe("Integer + String") { 1 + "" }
probe("Integer + nil") { 1 + nil }
probe("String * String") { "a" * "b" }
probe("Hash#[]= frozen key") { {}.merge!(1) }
probe("implicit to_str") { File.read(1) }
probe("implicit to_int") { [1][:x] }
probe("zero division") { 1 / 0 }
probe("Integer overflow shift") { 1 << -1 }

# --- Regexp ---------------------------------------------------------------
probe("bad property") { Regexp.new("\\p{NoSuchProperty}") }
probe("bad posix class") { Regexp.new("[[:nosuch:]]") }
probe("unterminated group") { Regexp.new("(") }
probe("bad repeat") { Regexp.new("a{2,1}") }
probe("bad backref") { Regexp.new("\\k<nope>") }

# --- Encoding -------------------------------------------------------------
probe("encode unknown") { "a".encode("NoSuchEncoding") }
probe("encode unknown pair") { "a".encode("NoSuchTarget", "NoSuchSource") }
probe("Encoding.find unknown") { Encoding.find("NoSuchEncoding") }
probe("Encoding.find symbol") { Encoding.find(:utf8) }
probe("force_encoding frozen") { "a".freeze.force_encoding("BINARY") }
probe("undefined conversion") { "é".encode("US-ASCII") }
probe("invalid byte sequence") { "\xff".dup.force_encoding("UTF-8").encode("UTF-16") }

# --- Misc -----------------------------------------------------------------
probe("negative sleep") { sleep(-1) }
probe("set_backtrace(bad)") { RuntimeError.new("x").set_backtrace(1) }
probe("Etc user missing") do
  require "etc"
  Etc.getpwnam("no-such-user-zeo")
end
probe("exec missing") { exec("/no/such/program-zeo") }
probe("backquote missing") { `/no/such/program-zeo` }
probe("system exception") { system("/no/such/program-zeo", exception: true) }

ROWS.each do |name, fn|
  next if SKIP.key?(name)

  r = begin
    v = fn.call
    "ok #{v.inspect}"
  rescue Exception => e
    "#{e.class}: #{e.message}"
  end
  puts "#{name}\t#{r.gsub(DIR, "<TMP>")}"
end
