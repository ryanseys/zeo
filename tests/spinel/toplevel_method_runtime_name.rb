# A top-level method is emitted as sp_<name>, which is the runtime's own
# namespace. The mangler carries an rb_ infix for names that would land in it,
# but it only looked at the segment in front of an underscore -- so `def sym_x`
# was protected while `def sym` collided with the sp_sym TYPEDEF and the
# program failed to build on a C diagnostic that never named `sym`.
def sym(v) = "sym:#{v}"
def int(v) = "int:#{v}"
def float(v) = "float:#{v}"
def queue(v) = "queue:#{v}"
def thread(v) = "thread:#{v}"
def argv(v) = "argv:#{v}"
def fmod(v) = "fmod:#{v}"
def safepoint(v) = "safepoint:#{v}"
def sym_x(v) = "sym_x:#{v}"
def plain(v) = "plain:#{v}"

p sym(1)
p int(2)
p float(3)
p queue(4)
p thread(5)
p argv(6)
p fmod(7)
p safepoint(8)
p sym_x(9)
p plain(10)

# names the list did not carry: a bare runtime function is just as much a
# collision as a runtime typedef, and `gcd` / `gets` / `readlines` are names a
# program writes without a second thought
def gcd(a, b) = "gcd:#{a}:#{b}"
def lcm(a, b) = "lcm:#{a}:#{b}"
def gets(v) = "gets:#{v}"
def readlines(v) = "readlines:#{v}"
def bool(v) = "bool:#{v}"
def mutex(v) = "mutex:#{v}"
def backtick(v) = "backtick:#{v}"
def idiv(a, b) = "idiv:#{a}:#{b}"

p gcd(1, 2)
p lcm(3, 4)
p gets(5)
p readlines(6)
p bool(7)
p mutex(8)
p backtick(9)
p idiv(10, 11)

# names only the generated set carries: the reserved set is read from the
# runtime sources now, so a name that exists there is covered whether or not
# anyone remembered to list it
def ceildiv(a, b) = "ceildiv:#{a}:#{b}"
def powmod(v) = "powmod:#{v}"
def unsentinel(v) = "unsentinel:#{v}"
def condvar(v) = "condvar:#{v}"

p ceildiv(1, 2)
p powmod(3)
p unsentinel(4)
p condvar(5)

# an infixed name and a user name that already spells the infix stay apart
def rb_sym(v) = "rb_sym:#{v}"
p sym(90)
p rb_sym(91)

# and the mangling does not leak into Ruby: the names still call each other
def outer(v) = sym(v) + "/" + int(v)
p outer(11)
