# Array#pack base64 ("m") and quoted-printable ("M") directives.

# m0: single base64 string, "=" padding, no line breaks
p ["hi"].pack("m0")
p ["hello world"].pack("m0")
p [""].pack("m0")
p ["a"].pack("m0")
p ["ab"].pack("m0")
p ["abc"].pack("m0")

# bare m: base64 with a trailing newline; wraps every 45 input bytes (60 cols)
p ["hi"].pack("m")
p ["hello world"].pack("m")
p ["Base64 quickly wraps Matz's joyful Ruby gems in vivid hex for the wire."].pack("m")

# binary input with an embedded NUL must survive (byte length, not strlen)
p ["a\x00b".dup].pack("m0")

# M: quoted-printable, 72-col soft wrap
p ["hello"].pack("M")
p ["caf\xC3\xA9".dup].pack("M")
p ["x=y"].pack("M")
# CRuby's M is NOT strict RFC 2045: trailing space/tab is emitted literally (not
# =20/=09), and the soft wrap fires only once a line exceeds the limit (n > line,
# so a full 72-char line stays unwrapped). These lock that byte-for-byte parity.
p ["abc   "].pack("M")             # trailing spaces kept literal
p ["a\t"].pack("M")                # trailing tab kept literal
p [("A" * 72)].pack("M")           # exactly 72: no soft wrap
p [("A" * 73)].pack("M")           # 73: still one line, wrap is > not >=
p [("x" * 80)].pack("M")           # wraps after 72
p ["ab   "].pack("M10")            # explicit column count

# poly-array receiver (routed through a param to defeat const-folding)
def via_poly(a) = a.pack("m0")
p via_poly(["hi", 1])
