
# The non-decimal conversions honour '+' and ' ': C's printf drops both on
# them, so every signed form was missing its sign
p(format("%+x", 255))
p(format("% x", 255))
p(format("%+X", 255))
p(format("%+o", 8))
p(format("% o", 8))
p(format("%+b", 5))
p(format("% b", 5))
p(format("%+5x", 255))
p(format("%+05x", 255))
p(format("%-+6x", 255) + "|")
p(format("% #x", 255))
p(format("%+#o", 8))
p(format("%+8b", 5))

# %u is %d, not a C-style unsigned conversion (it was rejected outright)
p(format("%u", 42))
p(format("%+u", 42))
p(format("%u", -42))

# the unsigned forms that already worked keep working, negatives included
p(format("%x", 255))
p(format("%+d", 42))
p(format("% d", 42))
p(format("%x", -255))
p(format("%o", -8))
p(format("%b", -5))
p(format("%#x", -255))
p(format(".8x: %.8x", -255))
p(format("%08x", -255))
p(format("%#o", -1))
p(format("%#.0o", 0))
p(format("%#b", 5))
p(format("%#X", 255))
p("%05.2f %x %s" % [1.5, 255, "s"])

# a later include wins, and its super reaches the earlier one
module A; def who; "A"; end; end
module B; def who; "B>" + super; end; end
module C; def who; "C>" + super; end; end
class K1; include A; include B; end
class K2; include A; include B; include C; end
class K3; include A; include B; def who; "K3>" + super; end; end
class K4; include A; include C; end
p K1.new.who
p K2.new.who
p K3.new.who
p K4.new.who
p K2.ancestors

# $! is the exception being handled: it goes back to nil when the handler
# exits, including when the handler exits by raising out of a modifier rescue
def m; raise "a"; rescue; raise; end
r = (m rescue 1)
p r
p $!
begin; raise "b"; rescue; p $!.message; end
p $!
def n; raise "c"; rescue; $!.message; end
p n
p $!
__END__
"+ff"
" ff"
"+FF"
"+10"
" 10"
"+101"
" 101"
"  +ff"
"+00ff"
"+ff   |"
" 0xff"
"+010"
"    +101"
"42"
"+42"
"-42"
"ff"
"+42"
" 42"
"..f01"
"..70"
"..1011"
"0x..f01"
".8x: ..ffff01"
"..ffff01"
"..7"
"0"
"0b101"
"0XFF"
"01.50 ff s"
"B>A"
"C>B>A"
"K3>B>A"
"C>A"
[K2, C, B, A, Object, Kernel, BasicObject]
1
nil
"b"
nil
"c"
nil
