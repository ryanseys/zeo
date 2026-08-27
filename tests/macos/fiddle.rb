require "fiddle"

# -- the TYPE_* constant table (values are the C extension's, verified) --
puts [Fiddle::TYPE_VOID, Fiddle::TYPE_VOIDP, Fiddle::TYPE_CHAR, Fiddle::TYPE_UCHAR,
      Fiddle::TYPE_SHORT, Fiddle::TYPE_USHORT, Fiddle::TYPE_INT, Fiddle::TYPE_UINT,
      Fiddle::TYPE_LONG, Fiddle::TYPE_ULONG, Fiddle::TYPE_LONG_LONG, Fiddle::TYPE_ULONG_LONG,
      Fiddle::TYPE_FLOAT, Fiddle::TYPE_DOUBLE, Fiddle::TYPE_VARIADIC, Fiddle::TYPE_CONST_STRING,
      Fiddle::TYPE_BOOL].join(",")
puts [Fiddle::TYPE_INT8_T, Fiddle::TYPE_UINT8_T, Fiddle::TYPE_INT16_T, Fiddle::TYPE_UINT16_T,
      Fiddle::TYPE_INT32_T, Fiddle::TYPE_UINT32_T, Fiddle::TYPE_INT64_T, Fiddle::TYPE_UINT64_T,
      Fiddle::TYPE_SSIZE_T, Fiddle::TYPE_SIZE_T, Fiddle::TYPE_PTRDIFF_T, Fiddle::TYPE_INTPTR_T,
      Fiddle::TYPE_UINTPTR_T].join(",")
puts [Fiddle::SIZEOF_VOIDP, Fiddle::SIZEOF_CHAR, Fiddle::SIZEOF_SHORT, Fiddle::SIZEOF_INT,
      Fiddle::SIZEOF_LONG, Fiddle::SIZEOF_LONG_LONG, Fiddle::SIZEOF_FLOAT, Fiddle::SIZEOF_DOUBLE,
      Fiddle::SIZEOF_BOOL, Fiddle::SIZEOF_SIZE_T, Fiddle::SIZEOF_INT64_T,
      Fiddle::SIZEOF_CONST_STRING].join(",")
puts [Fiddle::ALIGN_VOIDP, Fiddle::ALIGN_CHAR, Fiddle::ALIGN_SHORT, Fiddle::ALIGN_INT,
      Fiddle::ALIGN_LONG, Fiddle::ALIGN_LONG_LONG, Fiddle::ALIGN_FLOAT, Fiddle::ALIGN_DOUBLE,
      Fiddle::ALIGN_BOOL, Fiddle::ALIGN_SIZE_T, Fiddle::ALIGN_INT64_T].join(",")
p [Fiddle::RTLD_LAZY, Fiddle::RTLD_NOW, Fiddle::RTLD_GLOBAL]
p Fiddle::WINDOWS

# -- Handle + sym --
h = Fiddle.dlopen(nil)
p h.class
p h.sym("strlen").class
p !!Fiddle::Handle.sym_defined?("strlen")
p !!Fiddle::Handle.sym_defined?("zzz_no_such_symbol")
begin
  h.sym("zzz_no_such_symbol")
rescue Fiddle::DLError => e
  p [e.class, e.message]
end
p Fiddle::Handle::DEFAULT.class
h2 = Fiddle::Handle.new
p h2.close
begin
  h2.sym("strlen")
rescue Fiddle::DLError => e
  p [e.class, e.message]
end

# -- Function: fixed-arg calls across the scalar types --
strlen = Fiddle::Function.new(h.sym("strlen"), [Fiddle::TYPE_VOIDP], Fiddle::TYPE_SIZE_T)
p strlen.call("hello, fiddle")
pow = Fiddle::Function.new(h.sym("pow"), [Fiddle::TYPE_DOUBLE, Fiddle::TYPE_DOUBLE], Fiddle::TYPE_DOUBLE)
p pow.call(2.0, 10.0)
labs = Fiddle::Function.new(h.sym("labs"), [Fiddle::TYPE_LONG], Fiddle::TYPE_LONG)
p labs.call(-42)
strcpy = Fiddle::Function.new(h.sym("strcpy"), [Fiddle::TYPE_VOIDP, Fiddle::TYPE_VOIDP], Fiddle::TYPE_VOIDP)
buf = Fiddle::Pointer.malloc(32, Fiddle::RUBY_FREE)
r = strcpy.call(buf, "copied!")
p r.class
p r.to_s
p buf.to_s

# -- Pointer surface --
p Fiddle::NULL.class
p Fiddle::NULL.null?
p Fiddle::NULL.to_i
p Fiddle::NULL.size
p (Fiddle::NULL + 5).to_i
s = Fiddle::Pointer["hello world"]
p s.class
p s.size
p s.to_s
p s.to_s(5)
p s.to_str
p s[0]
p s[0, 5]
s[0] = 72
p s.to_s
q = s + 6
p q.to_s
p q.size
p (q - 6) == s
p [s <=> q, q <=> s, s <=> s]
p s.eql?(s)
p s == 12
mem = Fiddle::Pointer.malloc(16, Fiddle::RUBY_FREE)
mem[0, 8] = "abcdefgh"
p mem[0, 8]
p mem.size
ref = mem.ref
p ref.class
p ref.ptr.to_i == mem.to_i
p (-mem).ptr.to_i == mem.to_i
p (+ref).to_i == mem.to_i
p mem.freed?
mem2 = Fiddle::Pointer.malloc(4)
p mem2.free
src = Fiddle::Pointer["ABCDEF"]
p Fiddle::Pointer.read(src.to_i, 3)
Fiddle::Pointer.write(src.to_i, "xy")
p src.to_s

# -- module malloc/free (C semantics: a raw address Integer) --
a = Fiddle.malloc(8)
p a.class
p Fiddle.free(a)

# -- NULL access error shapes --
begin
  Fiddle::NULL.to_s
rescue ArgumentError => e
  p [e.class, e.message]
end
p Fiddle::NULL.to_str
begin
  Fiddle::NULL[0]
rescue Fiddle::DLError => e
  p [e.class, e.message]
end
begin
  Fiddle::NULL[0] = 65
rescue Fiddle::DLError => e
  p [e.class, e.message]
end

# -- errors hierarchy --
p Fiddle::DLError.ancestors[0, 3]
p Fiddle::ClearedReferenceError.ancestors[0, 3]

# -- last_error --
Fiddle.last_error = 0
p Fiddle.last_error

# -- closures: qsort through a BlockCaller --
cmp = Fiddle::Closure::BlockCaller.new(Fiddle::TYPE_INT, [Fiddle::TYPE_VOIDP, Fiddle::TYPE_VOIDP]) do |x, y|
  x[0] - y[0]
end
qsort = Fiddle::Function.new(h.sym("qsort"),
  [Fiddle::TYPE_VOIDP, Fiddle::TYPE_SIZE_T, Fiddle::TYPE_SIZE_T, Fiddle::TYPE_VOIDP],
  Fiddle::TYPE_VOID)
data = Fiddle::Pointer["edcba"]
qsort.call(data, 5, 1, cmp)
p data.to_s
p cmp.freed?
p cmp.args
p cmp.ctype
cmp2 = Fiddle::Closure::BlockCaller.new(Fiddle::TYPE_INT, [Fiddle::TYPE_INT]) { |x| x * 2 }
p cmp2.to_i.class
cmp2.free
p cmp2.freed?

# -- a closure as a Function target --
dbl = Fiddle::Closure::BlockCaller.new(Fiddle::TYPE_INT, [Fiddle::TYPE_INT]) { |x| x + x }
f = Fiddle::Function.new(dbl, [Fiddle::TYPE_INT], Fiddle::TYPE_INT)
p f.call(21)

# -- Function odds --
p strlen.to_proc.call("abc")
named = Fiddle::Function.new(h.sym("strlen"), [Fiddle::TYPE_VOIDP], Fiddle::TYPE_INT, name: "strlen")
p named.name
p named.abi == Fiddle::Function::DEFAULT
p strlen.to_i.class
begin
  Fiddle::Function.new(h.sym("strlen"), Fiddle::TYPE_VOIDP, Fiddle::TYPE_INT)
rescue TypeError => e
  p [e.class, e.message]
end
begin
  Fiddle::Function.new(h.sym("strlen"), [Fiddle::TYPE_VOIDP], 99)
rescue RuntimeError => e
  p [e.class, e.message]
end

# -- variadic --
snprintf = Fiddle::Function.new(h.sym("snprintf"),
  [Fiddle::TYPE_VOIDP, Fiddle::TYPE_SIZE_T, Fiddle::TYPE_CONST_STRING, Fiddle::TYPE_VARIADIC],
  Fiddle::TYPE_INT)
out = Fiddle::Pointer.malloc(64, Fiddle::RUBY_FREE)
n = snprintf.call(out, 64, "d=%d s=%s f=%.2f", Fiddle::TYPE_INT, 42, :const_string, "hi", Fiddle::TYPE_DOUBLE, 2.5)
p n
p out.to_s

# -- Pinned --
pin = Fiddle::Pinned.new("obj")
p pin.ref
p pin.cleared?
pin.clear
p pin.cleared?
begin
  pin.ref
rescue Fiddle::ClearedReferenceError => e
  p [e.class, e.message]
end
