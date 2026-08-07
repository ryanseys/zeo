# A Regexp literal's trailing letter may name an ENCODING -- `/n`, `/e`, `/s`,
# `/u` -- and that is observable three ways at once: `#options` reports it as a
# bit, `#encoding` names it, and `#fixed_encoding?` answers whether the pattern
# is pinned to one.
#
# `/e` and `/s` used to be a clean rejection and `/n` and `/u` were treated as
# harmless no-ops, which they are not: they reported 0 options, US-ASCII, and
# false. ruby2ruby and ruby_parser both OPEN by reading those bits back out of
# four throwaway patterns, and they are the reason the numbers here matter.

# `/n` sets NOENCODING (32); the other three set FIXEDENCODING (16). None of
# them touch the i/x/m bits, which stay 1/2/4 and combine.
p [/x/n.options, /x/e.options, /x/s.options, /x/u.options]
p [/x/.options, /x/i.options, /x/x.options, /x/m.options, /x/mix.options]
p [/x/in.options, /x/me.options, /x/xu.options]

# ruby2ruby's own four lines, verbatim in shape.
ENC_NONE = /x/n.options
ENC_EUC  = /x/e.options
ENC_SJIS = /x/s.options
ENC_UTF8 = /x/u.options
p [ENC_NONE, ENC_EUC, ENC_SJIS, ENC_UTF8]

# `#encoding` answers the pinned one. `/n` and a plain literal answer what their
# own source bytes compute to -- US-ASCII for an ASCII-only pattern.
p [/x/e.encoding, /x/s.encoding, /x/u.encoding, /x/n.encoding, /x/.encoding]

# `fixed_encoding?` is true for the three that PIN an encoding. `/n` declares
# the opposite -- encoding-agnostic -- so it is false.
p [/x/e.fixed_encoding?, /x/s.fixed_encoding?, /x/u.fixed_encoding?]
p [/x/n.fixed_encoding?, /x/.fixed_encoding?]

# A non-ASCII pattern with no flag at all is still fixed-encoding: its own bytes
# pin it.
p [/café/.fixed_encoding?, /café/.encoding]

# The flag changes nothing about MATCHING for an ASCII pattern. action_policy
# writes `/s` where it means ruby's `/m` -- ruby reads it as Windows-31J, so the
# dot does NOT cross a newline, and the pattern still matches ordinary text.
p(/^\s*binding\.(pry|irb)\s*$/s =~ "  binding.pry  ")
p(/a.b/s =~ "a\nb")
p(/a.b/m =~ "a\nb")

# ... and it composes with the flags that do change matching.
p(/A/ie =~ "a")
p(/A B/xu =~ "AB")
p(/a.b/mn =~ "a\nb")

# The letter survives onto an INTERPOLATED literal too.
part = "b"
re = /a#{part}c/u
p [re.options, re.encoding, re.fixed_encoding?, re =~ "abc"]

# `Regexp.new` has no spelling for these letters, so a runtime-built regexp is
# never encoding-pinned.
built = Regexp.new("x")
p [built.options, built.encoding, built.fixed_encoding?]

# `#source` is the pattern alone -- the flags are not part of it.
p [/x/e.source, /x/imx.source]

# `#inspect` puts back only `n`, and sorts it after m/i/x. The other three
# print bare: the encoding rides on the object, not on its printed source.
p [/x/n.inspect, /x/e.inspect, /x/s.inspect, /x/u.inspect]
p [/x/mix.inspect, /x/mixn.inspect, /x/nm.inspect]

# `#to_s` -- the embeddable `(?flags-flags:...)` form -- carries none of them.
p [/x/n.to_s, /x/e.to_s, /x/mi.to_s]
